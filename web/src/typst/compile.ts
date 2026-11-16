// One Typst compile, from the project's files to a PDF: push the files in,
// compile, and fetch any @preview packages the document imports, compiling
// again until it has them all. Runs in the compile worker; the network and the
// clock come in as arguments so tests can stand in for them.
import { packageSpec, type TypstDiagnostic, type TypstModule, type TypstPackage, type TypstToday } from "./engine";

export const TYPST_UNIVERSE = "https://packages.typst.org";

/** Packages import packages, but not endlessly. */
const MAX_PACKAGE_ROUNDS = 12;

export interface TypstFile {
  path: string;
  data: Uint8Array;
}

export interface TypstJob {
  main: string;
  files: readonly TypstFile[];
  today: TypstToday | null;
}

export interface TypstJobResult {
  ok: boolean;
  pdf: Uint8Array | null;
  diagnostics: TypstDiagnostic[];
  log: string;
}

export type FetchArchive = (url: string) => Promise<Uint8Array>;

export function packageUrl(pkg: TypstPackage, universe = TYPST_UNIVERSE): string {
  return `${universe}/${encodeURIComponent(pkg.namespace)}/${encodeURIComponent(pkg.name)}-${encodeURIComponent(pkg.version)}.tar.gz`;
}

export async function fetchArchive(url: string): Promise<Uint8Array> {
  let response: Response;
  try {
    response = await fetch(url, { credentials: "omit" });
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new Error(
      `${reason}. If this keeps happening, the server may not allow ${new URL(url).origin}: add it to OLEAFLY_WEB_CONNECT_SRC.`,
    );
  }
  if (response.status === 404) throw new Error("it isn't in Typst Universe");
  if (!response.ok) throw new Error(`Typst Universe answered ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}

function packageFailure(pkg: TypstPackage, reason: string): TypstJobResult {
  const message = `Couldn't download the package ${packageSpec(pkg)}: ${reason}`;
  return {
    ok: false,
    pdf: null,
    diagnostics: [{ severity: "error", message, file: null, line: null, location: null, hints: [] }],
    log: `error: ${message}\n`,
  };
}

export async function runTypst(module: TypstModule, job: TypstJob, fetchPackage: FetchArchive = fetchArchive): Promise<TypstJobResult> {
  module.clearFiles();
  for (const file of job.files) module.putFile(file.path, file.data);

  for (let round = 0; round <= MAX_PACKAGE_ROUNDS; round++) {
    const result = module.compile(job.main, job.today);
    if (result.kind === "pdf") {
      return { ok: true, pdf: result.pdf, diagnostics: result.diagnostics, log: formatLog(result.diagnostics) };
    }
    if (result.kind === "failed") {
      return { ok: false, pdf: null, diagnostics: result.diagnostics, log: formatLog(result.diagnostics) };
    }
    const wanted = result.packages.filter((pkg) => !module.hasPackage(pkg));
    if (wanted.length === 0) break;
    const fetched = await Promise.allSettled(wanted.map((pkg) => fetchPackage(packageUrl(pkg))));
    for (const [index, outcome] of fetched.entries()) {
      const pkg = wanted[index];
      if (outcome.status === "rejected") {
        return packageFailure(pkg, outcome.reason instanceof Error ? outcome.reason.message : String(outcome.reason));
      }
      try {
        module.addPackage(pkg, outcome.value);
      } catch (error) {
        return packageFailure(pkg, error instanceof Error ? error.message : String(error));
      }
    }
  }
  const message = "The document's packages kept asking for more packages, so the compile stopped.";
  return {
    ok: false,
    pdf: null,
    diagnostics: [{ severity: "error", message, file: null, line: null, location: null, hints: [] }],
    log: `error: ${message}\n`,
  };
}

/** The diagnostics as the log pane shows them, close to `typst compile`'s own output. */
export function formatLog(diagnostics: readonly TypstDiagnostic[]): string {
  return diagnostics
    .map((diagnostic) => {
      const place = diagnostic.file
        ? `${diagnostic.file}${diagnostic.line ? `:${diagnostic.line}` : ""}: `
        : diagnostic.location
          ? `${diagnostic.location}: `
          : "";
      const hints = diagnostic.hints.map((hint) => `\n  hint: ${hint}`).join("");
      return `${diagnostic.severity}: ${place}${diagnostic.message}${hints}\n`;
    })
    .join("");
}

export function todayOf(now: Date): TypstToday {
  return { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate(), utcMs: now.getTime() };
}
