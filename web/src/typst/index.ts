// The Typst engine for the browser app's compile port. It gathers the
// project's files on the page, where the sources live, and compiles them in
// the worker.
import type { LogDiagnostic } from "@oleafly/backend-port";
import { type BrowserEngine, type CompileSources, CompileStopped, type EngineResult } from "@oleafly/compile-port";
import { type TypstFile, type TypstJob, type TypstJobResult, todayOf } from "./compile";
import type { TypstDiagnostic } from "./engine";
import type { WorkerRequest, WorkerResponse } from "./worker";

/**
 * Files Typst can read besides text: images, fonts and data. Other blobs, such
 * as a PDF figure's source or an archive, are left out so a compile doesn't
 * download them.
 */
const READABLE_BLOBS = /\.(?:png|jpe?g|gif|webp|svg|pdf|ttf|otf|ttc|csv|json|ya?ml|toml|xml|bib|cbor|txt)$/iu;

export interface TypstWorker {
  postMessage(message: WorkerRequest, transfer: Transferable[]): void;
  addEventListener(type: "message", listener: (event: MessageEvent<WorkerResponse>) => void): void;
  addEventListener(type: "error", listener: (event: ErrorEvent) => void): void;
}

export interface TypstEngineOptions {
  worker?: () => TypstWorker;
  now?: () => Date;
}

function defaultWorker(): TypstWorker {
  return new Worker(new URL("./worker.ts", import.meta.url), { type: "module", name: "typst" });
}

export function toLogDiagnostic(diagnostic: TypstDiagnostic): LogDiagnostic {
  const message = diagnostic.location ? `${diagnostic.message} (in ${diagnostic.location})` : diagnostic.message;
  return {
    severity: diagnostic.severity,
    message: diagnostic.hints.length ? `${message}\n${diagnostic.hints.map((hint) => `hint: ${hint}`).join("\n")}` : message,
    file: diagnostic.file,
    line: diagnostic.line,
    category: "error",
  };
}

async function gather(sources: CompileSources, signal: AbortSignal): Promise<TypstFile[]> {
  const encoder = new TextEncoder();
  const files: TypstFile[] = [];
  const blobs: Promise<void>[] = [];
  for (const file of sources.files) {
    if (file.kind === "text") {
      const text = sources.text(file.path);
      if (text !== null) files.push({ path: file.path, data: encoder.encode(text) });
    } else if (READABLE_BLOBS.test(file.path)) {
      blobs.push(
        sources.bytes(file.path).then((data) => {
          if (data) files.push({ path: file.path, data });
        }),
      );
    }
  }
  await Promise.all(blobs);
  if (signal.aborted) throw new CompileStopped();
  return files;
}

export function createTypstEngine(options: TypstEngineOptions = {}): BrowserEngine {
  const now = options.now ?? (() => new Date());
  let worker: TypstWorker | null = null;
  let nextId = 1;
  const pending = new Map<number, { resolve(result: TypstJobResult): void; reject(error: Error): void }>();

  function started(): TypstWorker {
    if (worker) return worker;
    const created = (options.worker ?? defaultWorker)();
    created.addEventListener("message", (event) => {
      const answer = event.data;
      const waiting = pending.get(answer.id);
      if (!waiting) return;
      pending.delete(answer.id);
      if ("error" in answer) waiting.reject(new Error(answer.error));
      else waiting.resolve(answer.result);
    });
    created.addEventListener("error", (event) => {
      // The worker itself failed to start or threw: fail what's waiting and
      // start a new one next time.
      worker = null;
      for (const waiting of pending.values()) waiting.reject(new Error(event.message || "The Typst compiler stopped."));
      pending.clear();
    });
    worker = created;
    return created;
  }

  function compile(job: TypstJob): Promise<TypstJobResult> {
    const id = nextId++;
    const running = new Promise<TypstJobResult>((resolve, reject) => pending.set(id, { resolve, reject }));
    started().postMessage(
      { id, job },
      job.files.map((file) => file.data.buffer),
    );
    return running;
  }

  return {
    accepts: (target) => target.engine === "typst",
    async run({ target, sources, signal }): Promise<EngineResult> {
      const files = await gather(sources, signal);
      const result = await compile({ main: target.mainDocument, files, today: todayOf(now()) });
      if (signal.aborted) throw new CompileStopped();
      return {
        ok: result.ok,
        pdf: result.pdf,
        synctex: null,
        log: result.log,
        diagnostics: result.diagnostics.map(toLogDiagnostic),
      };
    },
  };
}
