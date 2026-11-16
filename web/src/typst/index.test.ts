import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { type CompileSources, CompileStopped, type CompileTarget } from "@oleafly/compile-port";
import { describe, expect, it, vi } from "vitest";
import { runTypst } from "./compile";
import { loadTypst } from "./engine";
import { createTypstEngine, type TypstWorker } from "./index";
import type { WorkerRequest, WorkerResponse } from "./worker";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasm = readFileSync(path.join(here, "oleafly_typst.wasm"));
const target: CompileTarget = { mainDocument: "main.typ", engine: "typst", texFlavor: null };

/** Runs the worker's job in this thread, against the real module. */
function inProcessWorker(seen: WorkerRequest[] = []): () => TypstWorker {
  return () => {
    const listeners: ((event: MessageEvent<WorkerResponse>) => void)[] = [];
    const module = loadTypst(wasm);
    return {
      postMessage(message) {
        seen.push(message);
        void module
          .then((typst) => runTypst(typst, message.job, async () => Promise.reject(new Error("offline"))))
          .then((result) => {
            for (const listener of listeners) listener({ data: { id: message.id, result } } as MessageEvent<WorkerResponse>);
          });
      },
      addEventListener(type: string, listener: (event: never) => void) {
        if (type === "message") listeners.push(listener as (event: MessageEvent<WorkerResponse>) => void);
      },
    };
  };
}

function sources(texts: Record<string, string>, blobs: Record<string, Uint8Array> = {}): CompileSources {
  return {
    files: [
      ...Object.keys(texts).map((path) => ({ path, kind: "text" as const })),
      ...Object.keys(blobs).map((path) => ({ path, kind: "blob" as const })),
    ],
    text: (path) => texts[path] ?? null,
    bytes: vi.fn(async (path: string) => blobs[path] ?? null),
  };
}

describe("the Typst engine", () => {
  it("takes Typst projects only", () => {
    const engine = createTypstEngine({ worker: inProcessWorker() });
    expect(engine.accepts(target)).toBe(true);
    expect(engine.accepts({ ...target, engine: "latex" })).toBe(false);
  });

  it("compiles in the worker and reports problems where they are", async () => {
    const engine = createTypstEngine({ worker: inProcessWorker() });
    const ok = await engine.run({ target, sources: sources({ "main.typ": "Hi" }), signal: new AbortController().signal });
    expect(ok).toMatchObject({ ok: true, synctex: null, diagnostics: [] });

    const failed = await engine.run({
      target,
      sources: sources({ "main.typ": "#oops" }),
      signal: new AbortController().signal,
    });
    expect(failed.ok).toBe(false);
    expect(failed.diagnostics?.[0]).toMatchObject({ severity: "error", file: "main.typ", line: 1, category: "error" });
  });

  it("sends images and data along, but not other files", async () => {
    const seen: WorkerRequest[] = [];
    const engine = createTypstEngine({ worker: inProcessWorker(seen) });
    const project = sources(
      { "main.typ": '#let data = csv("table.csv")' },
      { "table.csv": new TextEncoder().encode("a,b\n1,2"), "backup.zip": new Uint8Array([1, 2, 3]) },
    );
    const result = await engine.run({ target, sources: project, signal: new AbortController().signal });
    expect(result.ok, result.log).toBe(true);
    expect(seen[0].job.files.map((file) => file.path).sort()).toEqual(["main.typ", "table.csv"]);
    expect(project.bytes).not.toHaveBeenCalledWith("backup.zip");
  });

  it("stops when a newer compile took over", async () => {
    const engine = createTypstEngine({ worker: inProcessWorker() });
    const controller = new AbortController();
    const running = engine.run({ target, sources: sources({ "main.typ": "Hi" }), signal: controller.signal });
    controller.abort();
    await expect(running).rejects.toBeInstanceOf(CompileStopped);
  });

  it("fails the compile when the worker dies", async () => {
    let fail: ((event: ErrorEvent) => void) | null = null;
    const engine = createTypstEngine({
      worker: () => ({
        postMessage() {
          queueMicrotask(() => fail?.({ message: "boom" } as ErrorEvent));
        },
        addEventListener(type: string, listener: (event: never) => void) {
          if (type === "error") fail = listener as (event: ErrorEvent) => void;
        },
      }),
    });
    await expect(
      engine.run({ target, sources: sources({ "main.typ": "Hi" }), signal: new AbortController().signal }),
    ).rejects.toThrow("boom");
  });
});
