// The compile worker: owns the Typst module, so a compile never blocks the
// editor. The module loads on the first compile, not when the page opens; a
// reader who never compiles never downloads it. It is kept between compiles,
// because Typst reuses its cache from the last compile.
import wasmUrl from "./oleafly_typst.wasm?url";
import { runTypst, type TypstJob, type TypstJobResult } from "./compile";
import { loadTypst, TypstError, type TypstModule } from "./engine";

export interface WorkerRequest {
  id: number;
  job: TypstJob;
}

export type WorkerResponse =
  | { id: number; result: TypstJobResult }
  | { id: number; error: string };

let loading: Promise<TypstModule> | null = null;

function module(): Promise<TypstModule> {
  loading ??= loadTypst(fetch(wasmUrl)).catch((error: unknown) => {
    loading = null;
    throw error;
  });
  return loading;
}

async function handle(job: TypstJob): Promise<TypstJobResult> {
  try {
    return await runTypst(await module(), job);
  } catch (error) {
    // A panic inside Typst traps the module. Start a fresh one for the next
    // compile, and say what happened now.
    if (error instanceof TypstError && error.code === "crashed") loading = null;
    throw error;
  }
}

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerResponse, transfer?: Transferable[]): void;
};

// One compile at a time: a compile waiting on a package download must not
// have its files replaced by the next one.
let queue: Promise<unknown> = Promise.resolve();

scope.onmessage = (event) => {
  const { id, job } = event.data;
  const run = queue.then(() => handle(job));
  queue = run.catch(() => undefined);
  run.then(
    (result) => scope.postMessage({ id, result }, result.pdf ? [result.pdf.buffer] : []),
    (error: unknown) => scope.postMessage({ id, error: error instanceof Error ? error.message : String(error) }),
  );
};
