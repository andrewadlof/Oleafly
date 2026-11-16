import type { BrowserEngine } from "@oleafly/compile-port";

export async function browserEngines(): Promise<readonly BrowserEngine[]> {
  if (import.meta.env.DEV && import.meta.env.VITE_OLEAFLY_FAKE_ENGINE === "1") {
    const { createFakeEngine } = await import("@oleafly/compile-port/testing");
    return [createFakeEngine()];
  }
  // Only the engine's small page-side half loads here. The compiler itself
  // is a worker that loads on the first compile.
  const { createTypstEngine } = await import("../typst");
  return [createTypstEngine()];
}
