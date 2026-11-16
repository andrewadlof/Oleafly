// The browser app's Typst compiler: a thin, synchronous wrapper around the
// exports of crates/oleafly-typst-wasm. It runs inside the compile worker
// (./worker.ts); nothing here knows about the page or the network.

export type TypstErrorCode = "bad-input" | "bad-package" | "internal" | "out-of-memory" | "crashed" | "load-failed";

export class TypstError extends Error {
  readonly code: TypstErrorCode;

  constructor(code: TypstErrorCode, message: string) {
    super(message);
    this.name = "TypstError";
    this.code = code;
  }
}

export interface TypstDiagnostic {
  severity: "error" | "warning";
  message: string;
  /** The project path, or null when the problem is in a package or nowhere in particular. */
  file: string | null;
  /** 1-based. */
  line: number | null;
  /** Where it is when that isn't a project file, such as `@preview/cetz:0.3.4/src/lib.typ:12`. */
  location: string | null;
  hints: string[];
}

export interface TypstPackage {
  namespace: string;
  name: string;
  version: string;
}

export interface TypstToday {
  year: number;
  month: number;
  day: number;
  utcMs: number;
}

export type TypstCompileResult =
  | { kind: "pdf"; pdf: Uint8Array; diagnostics: TypstDiagnostic[] }
  | { kind: "failed"; diagnostics: TypstDiagnostic[] }
  | { kind: "needs-packages"; packages: TypstPackage[] };

export interface TypstModule {
  clearFiles(): void;
  putFile(path: string, data: Uint8Array): void;
  hasPackage(pkg: TypstPackage): boolean;
  addPackage(pkg: TypstPackage, archive: Uint8Array): void;
  compile(main: string, today: TypstToday | null): TypstCompileResult;
}

export interface TypstExports {
  memory: WebAssembly.Memory;
  typst_alloc(len: number): number;
  typst_free(ptr: number, len: number): void;
  typst_clear_files(): number;
  typst_put_file(path: number, pathLen: number, data: number, dataLen: number): number;
  typst_has_package(spec: number, specLen: number): number;
  typst_add_package(spec: number, specLen: number, archive: number, archiveLen: number): number;
  typst_compile(main: number, mainLen: number, year: number, month: number, day: number, utcMs: number): number;
  typst_output_ptr(): number;
  typst_output_len(): number;
  typst_diagnostics_ptr(): number;
  typst_diagnostics_len(): number;
}

// Mirrors the STATUS_* constants in crates/oleafly-typst-wasm/src/lib.rs.
const STATUS_FAILED = 0;
const STATUS_PDF = 1;
const STATUS_NEEDS_PACKAGES = 2;
const ERROR_CODES: ReadonlyMap<number, TypstErrorCode> = new Map([
  [-1, "bad-input"],
  [-2, "bad-package"],
  [-6, "internal"],
]);

export function packageSpec(pkg: TypstPackage): string {
  return `@${pkg.namespace}/${pkg.name}:${pkg.version}`;
}

export function createTypstModule(exports: TypstExports): TypstModule {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  let crashed = false;

  function call<T>(run: () => T): T {
    if (crashed) throw new TypstError("crashed", "The Typst compiler stopped after an earlier error.");
    try {
      return run();
    } catch (error) {
      if (error instanceof WebAssembly.RuntimeError) {
        crashed = true;
        throw new TypstError("crashed", `The Typst compiler stopped: ${error.message}`);
      }
      throw error;
    }
  }

  // Pointers and lengths are unsigned; JavaScript sees them as signed.
  const view = (ptr: number, len: number) => new Uint8Array(exports.memory.buffer, ptr >>> 0, len >>> 0);
  const output = () => view(exports.typst_output_ptr(), exports.typst_output_len());
  const outputText = () => decoder.decode(output());
  const diagnostics = (): TypstDiagnostic[] => {
    const bytes = view(exports.typst_diagnostics_ptr(), exports.typst_diagnostics_len());
    return bytes.length === 0 ? [] : (JSON.parse(decoder.decode(bytes)) as TypstDiagnostic[]);
  };

  function check(status: number): number {
    if (status >= 0) return status;
    throw new TypstError(ERROR_CODES.get(status) ?? "internal", outputText() || `Typst failed with status ${status}.`);
  }

  function withBuffers(buffers: readonly Uint8Array[], run: (pointers: number[]) => number): number {
    const pointers: number[] = [];
    try {
      for (const buffer of buffers) {
        const ptr = exports.typst_alloc(buffer.length) >>> 0;
        if (ptr === 0) throw new TypstError("out-of-memory", "The Typst compiler ran out of memory.");
        pointers.push(ptr);
        // Read memory.buffer after each allocation: growing memory detaches the old buffer.
        view(ptr, buffer.length).set(buffer);
      }
      return run(pointers);
    } finally {
      for (const [index, ptr] of pointers.entries()) exports.typst_free(ptr, buffers[index].length);
    }
  }

  const specBytes = (pkg: TypstPackage) => encoder.encode(`${pkg.namespace}/${pkg.name}/${pkg.version}`);

  return {
    clearFiles() {
      call(() => check(exports.typst_clear_files()));
    },
    putFile(path, data) {
      const name = encoder.encode(path);
      call(() => check(withBuffers([name, data], ([p, d]) => exports.typst_put_file(p, name.length, d, data.length))));
    },
    hasPackage(pkg) {
      const spec = specBytes(pkg);
      return call(() => check(withBuffers([spec], ([s]) => exports.typst_has_package(s, spec.length))) === 1);
    },
    addPackage(pkg, archive) {
      const spec = specBytes(pkg);
      call(() =>
        check(withBuffers([spec, archive], ([s, a]) => exports.typst_add_package(s, spec.length, a, archive.length))),
      );
    },
    compile(main, today) {
      const path = encoder.encode(main);
      return call(() => {
        const status = check(
          withBuffers([path], ([p]) =>
            exports.typst_compile(p, path.length, today?.year ?? 0, today?.month ?? 0, today?.day ?? 0, today?.utcMs ?? 0),
          ),
        );
        switch (status) {
          case STATUS_PDF:
            // Copy out: the next call reuses the buffer.
            return { kind: "pdf", pdf: output().slice(), diagnostics: diagnostics() };
          case STATUS_FAILED:
            return { kind: "failed", diagnostics: diagnostics() };
          case STATUS_NEEDS_PACKAGES:
            return { kind: "needs-packages", packages: JSON.parse(outputText()) as TypstPackage[] };
          default:
            throw new TypstError("internal", `Typst answered with an unknown status ${status}.`);
        }
      });
    },
  };
}

export async function loadTypst(wasm: BufferSource | Response | Promise<Response>): Promise<TypstModule> {
  let instance: WebAssembly.Instance;
  try {
    const source = await wasm;
    if (source instanceof Response) {
      if (!source.ok) throw new Error(`the server answered ${source.status}`);
      instance = (await WebAssembly.instantiate(await source.arrayBuffer(), {})).instance;
    } else {
      instance = (await WebAssembly.instantiate(source, {})).instance;
    }
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new TypstError("load-failed", `The Typst compiler couldn't load: ${reason}`);
  }
  return createTypstModule(instance.exports as unknown as TypstExports);
}
