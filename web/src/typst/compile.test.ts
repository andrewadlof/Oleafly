import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatLog, packageUrl, runTypst, todayOf, type TypstFile } from "./compile";
import { loadTypst, type TypstModule } from "./engine";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasm = readFileSync(path.join(here, "oleafly_typst.wasm"));
const encoder = new TextEncoder();
const today = { year: 2026, month: 11, day: 16, utcMs: Date.UTC(2026, 10, 16, 12) };

const text = (files: Record<string, string>): TypstFile[] =>
  Object.entries(files).map(([path, body]) => ({ path, data: encoder.encode(body) }));

/** A minimal ustar archive, enough for the module's unpacker. */
function tarGz(files: Record<string, string>): Uint8Array {
  const blocks: Uint8Array[] = [];
  const field = (block: Uint8Array, offset: number, length: number, value: string) =>
    block.set(encoder.encode(value).subarray(0, length), offset);
  for (const [name, body] of Object.entries(files)) {
    const data = encoder.encode(body);
    const header = new Uint8Array(512);
    field(header, 0, 100, name);
    field(header, 100, 8, "0000644\0");
    field(header, 108, 8, "0000000\0");
    field(header, 116, 8, "0000000\0");
    field(header, 124, 12, `${data.length.toString(8).padStart(11, "0")}\0`);
    field(header, 136, 12, "00000000000\0");
    header[156] = "0".charCodeAt(0);
    field(header, 257, 6, "ustar\0");
    field(header, 263, 2, "00");
    header.fill(32, 148, 156);
    const sum = header.reduce((total, byte) => total + byte, 0);
    field(header, 148, 8, `${sum.toString(8).padStart(6, "0")}\0 `);
    blocks.push(header, data, new Uint8Array((512 - (data.length % 512)) % 512));
  }
  blocks.push(new Uint8Array(1024));
  const tar = new Uint8Array(blocks.reduce((size, block) => size + block.length, 0));
  let offset = 0;
  for (const block of blocks) {
    tar.set(block, offset);
    offset += block.length;
  }
  return new Uint8Array(gzipSync(tar));
}

const HELLO = tarGz({
  "typst.toml":
    '[package]\nname = "hello"\nversion = "0.1.0"\nentrypoint = "lib.typ"\nauthors = ["A"]\nlicense = "MIT"\ndescription = "Hi"\n',
  "lib.typ": "#let greet() = [Hello from a package]",
});

describe("runTypst", () => {
  let module: TypstModule;

  beforeEach(async () => {
    module = await loadTypst(wasm);
  });

  it("compiles a project to a PDF", async () => {
    const result = await runTypst(module, {
      main: "main.typ",
      files: text({ "main.typ": '= Title\n#include "intro.typ"\n$ sum_(i=1)^n i $', "intro.typ": "Intro *text*." }),
      today,
    });
    expect(result.ok).toBe(true);
    expect(new TextDecoder().decode(result.pdf?.subarray(0, 5))).toBe("%PDF-");
    expect(result.log).toBe("");
  });

  it("reports an error on its line", async () => {
    const result = await runTypst(module, { main: "main.typ", files: text({ "main.typ": "fine\n#missing" }), today });
    expect(result.ok).toBe(false);
    expect(result.pdf).toBeNull();
    expect(result.diagnostics[0]).toMatchObject({ severity: "error", file: "main.typ", line: 2 });
    expect(result.log).toMatch(/^error: main\.typ:2: unknown variable: missing/u);
  });

  it("downloads the packages a document imports, once", async () => {
    const fetchPackage = vi.fn(async () => HELLO);
    const job = {
      main: "main.typ",
      files: text({ "main.typ": '#import "@preview/hello:0.1.0": greet\n#greet()' }),
      today,
    };
    expect((await runTypst(module, job, fetchPackage)).ok).toBe(true);
    expect((await runTypst(module, job, fetchPackage)).ok).toBe(true);
    expect(fetchPackage).toHaveBeenCalledTimes(1);
    expect(fetchPackage).toHaveBeenCalledWith("https://packages.typst.org/preview/hello-0.1.0.tar.gz");
  });

  it("says which package couldn't be downloaded", async () => {
    const result = await runTypst(
      module,
      { main: "main.typ", files: text({ "main.typ": '#import "@preview/nope:9.9.9": *' }), today },
      async () => Promise.reject(new Error("it isn't in Typst Universe")),
    );
    expect(result.ok).toBe(false);
    expect(result.log).toBe("error: Couldn't download the package @preview/nope:9.9.9: it isn't in Typst Universe\n");
  });

  it("refuses an archive that isn't one", async () => {
    const result = await runTypst(
      module,
      { main: "main.typ", files: text({ "main.typ": '#import "@preview/hello:0.1.0": *' }), today },
      async () => encoder.encode("<html>not a package</html>"),
    );
    expect(result.ok).toBe(false);
    expect(result.diagnostics[0].message).toMatch(/^Couldn't download the package @preview\/hello:0\.1\.0: /u);
  });

  it("uses fonts and images from the project", async () => {
    const svg = readFileSync(path.resolve(here, "../leaf.svg"));
    const result = await runTypst(module, {
      main: "main.typ",
      files: [...text({ "main.typ": '#image("leaf.svg", width: 2cm)' }), { path: "leaf.svg", data: new Uint8Array(svg) }],
      today,
    });
    expect(result.ok, result.log).toBe(true);
  });

  it("gives Typst today's date", async () => {
    const result = await runTypst(module, {
      main: "main.typ",
      files: text({ "main.typ": '#assert.eq(datetime.today().display(), "2026-11-16")' }),
      today,
    });
    expect(result.ok, result.log).toBe(true);
  });
});

describe("helpers", () => {
  it("builds Typst Universe URLs", () => {
    expect(packageUrl({ namespace: "preview", name: "cetz", version: "0.3.4" })).toBe(
      "https://packages.typst.org/preview/cetz-0.3.4.tar.gz",
    );
  });

  it("formats diagnostics like the command line", () => {
    expect(
      formatLog([
        { severity: "warning", message: "unused", file: "a.typ", line: 3, location: null, hints: ["remove it"] },
        { severity: "error", message: "bad", file: null, line: null, location: "@preview/x:1.0.0/lib.typ:4", hints: [] },
      ]),
    ).toBe("warning: a.typ:3: unused\n  hint: remove it\nerror: @preview/x:1.0.0/lib.typ:4: bad\n");
  });

  it("reads the local date", () => {
    expect(todayOf(new Date(2026, 0, 2, 3, 4))).toMatchObject({ year: 2026, month: 1, day: 2 });
  });
});
