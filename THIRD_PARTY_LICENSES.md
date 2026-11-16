# Third-Party Licenses

Oleafly is distributed under the [GNU AGPL v3 or later](LICENSE). It bundles
third-party open-source components, each under its own license, listed below.
The build accepts the licenses listed in `src-tauri/deny.toml`, including
permissive licenses and specifically reviewed licenses such as MPL-2.0.

This file lists the **direct** dependencies Oleafly ships. Their transitive
dependencies are checked by the automated license policy. The full, authoritative license
text for every JavaScript package is under `node_modules/<pkg>/LICENSE`, and for
every Rust crate under its source in the Cargo registry.

To regenerate a complete, transitive report:

```sh
pnpm licenses list --prod          # JavaScript / frontend
cargo install cargo-about && cargo about generate about.hbs   # Rust / backend
```

---

## Bundled binary

| Component | Purpose | License |
|---|---|---|
| [Tectonic](https://tectonic-typesetting.github.io/) | LaTeX compiler (sidecar) | MIT |
| [Biber 2.17](https://sourceforge.net/projects/biblatex-biber/) | Bibliography backend for biblatex (`tectonic-biber` sidecar, version-pinned to Tectonic’s biblatex). Modified: `scripts/biber/harden-biber.mjs` lists every change, and the Linux ARM64 build also carries upstream commit d9e96171. | Artistic-2.0 / GPL-1.0-or-later |
| Libraries packed into the Linux ARM64 Biber by `scripts/biber/build-linux.sh` | Perl 5.32 and the CPAN modules in `scripts/biber/cpan-lock.txt`, btparse, libxml2, libxslt, ICU 67, OpenSSL 1.1, libgcrypt, libgpg-error, zlib, bzip2, liblzma, Berkeley DB 5.3, GDBM, libcrypt, libstdc++ | Each under its own license; see the Debian 11 package copyright files |
| [Pandoc 3.9.0.2](https://github.com/jgm/pandoc/releases/tag/3.9.0.2) | Document converter (separate sidecar process) | GPL-2.0-or-later |
| [Typst](https://github.com/typst/typst) | Typst compiler 0.15.1 (sidecar) | Apache-2.0 |
| [Tinymist 0.15.8](https://github.com/Myriad-Dreamin/tinymist/tree/v0.15.8) | Typst language server (checksum-pinned upstream archive resource), © 2023–2025 Myriad Dreamin and Nathan Varner | Apache-2.0 |

Pandoc is an unmodified program that Oleafly starts as a separate process. Its
complete license is included at
`resources/licenses/pandoc-3.9.0.2-COPYING.md`. The corresponding source for
the exact bundled version is available from the
[3.9.0.2 source tag](https://github.com/jgm/pandoc/tree/3.9.0.2) and the
[source archive](https://github.com/jgm/pandoc/archive/refs/tags/3.9.0.2.tar.gz).
`scripts/fetch-pandoc.sh` downloads the release archive for each target and
checks it against these SHA-256 digests before extracting the executable:

| File | SHA-256 |
|---|---|
| `pandoc-3.9.0.2-arm64-macOS.zip` | `6e9eca844076bcbb599bbeebbba78a70f93b5307782b85c2c272872812c88875` |
| `pandoc-3.9.0.2-linux-arm64.tar.gz` | `b6d21e8f9c3b15744f5a7ab40248019157ed7793875dbe0383d4c82ff572b528` |
| `pandoc-3.9.0.2-linux-amd64.tar.gz` | `a69abfababda8a56969a254b09f9553a7be89ddec00d4e0fe9fd585d71a67508` |
| `pandoc-3.9.0.2-windows-x86_64.zip` | `c97542f2800f446e788d9f74237856d995421ad1bb3cc8324286840c5f272d3a` |
| `COPYING.md` (license text) | `9d56cac92294e206af026a5502bee0fed77200b08b51ec28aa63c9efda4dcfdd` |

The exact Tinymist 0.15.8 license is shipped in every application bundle at
`resources/licenses/tinymist-0.15.8-LICENSE`, alongside exactly one
target-specific unmodified release archive under
`resources/language-servers/tinymist/0.15.8/`. Upstream has no `NOTICE` file
at the pinned tag.

## Backend (Rust crates)

| Crate | License |
|---|---|
| tauri, tauri-build | Apache-2.0 OR MIT |
| tauri-plugin-shell / -dialog / -updater / -process / -single-instance / -notification | Apache-2.0 OR MIT |
| serde, serde_json | MIT OR Apache-2.0 |
| reqwest | MIT OR Apache-2.0 |
| base64 | MIT OR Apache-2.0 |
| flate2 | MIT OR Apache-2.0 |
| biblatex | MIT OR Apache-2.0 |
| statrs | MIT |
| rapidfuzz | MIT |
| zip | MIT |
| agent-client-protocol, agent-client-protocol-schema | Apache-2.0 |
| harper-core | Apache-2.0 |
| lopdf, pdf-extract | MIT |
| fancy-regex | MIT |
| dom_query | MIT |
| encoding_rs | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| notify-rust | MIT OR Apache-2.0 |
| rpassword | Apache-2.0 |
| similar | Apache-2.0 |
| toml_edit, roxmltree | MIT OR Apache-2.0 |
| anstream, anstyle | MIT OR Apache-2.0 |

## Browser app (Rust crates compiled to WebAssembly)

The browser app that the collaboration server serves runs the desktop's
SyncTeX code compiled to WebAssembly (`oleafly_synctex.wasm`). These crates
are compiled into it:

| Crate | License |
|---|---|
| flate2, with miniz_oxide | MIT OR Apache-2.0 (miniz_oxide: MIT OR Zlib OR Apache-2.0) |
| serde, serde_json | MIT OR Apache-2.0 |
| unicode-normalization | MIT OR Apache-2.0 |

It also runs Typst compiled to WebAssembly (`oleafly_typst.wasm`), which
compiles Typst projects in the browser. It loads on a Typst project's first
compile. These crates are compiled into it:

| Crate | License |
|---|---|
| arrayvec, az, base64, biblatex, bitflags, bumpalo, by_address, cfg-if, citationberg, cobs, comemo, comemo-macros, crc32fast, crossbeam-deque, crossbeam-epoch, crossbeam-utils, data-url, deranged, displaydoc, ecow, either, embedded-io, euclid, fdeflate, filetime, flate2, font-types, form_urlencoded, futures-channel, futures-core, futures-io, futures-macro, futures-sink, futures-task, futures-util, getrandom, gif, glidesort, guillotiere, half, hash32, hashbrown, hayagriva, heapless, heck, hypher, idna, image, image-webp, itoa, js-sys, krilla, krilla-svg, linked-hash-map, lock_api, log, memmap2, num-bigint, num-conv, num-integer, num-traits, once_cell, palette, palette_derive, palette_math, parking_lot, parking_lot_core, paste, pdf-writer, percent-encoding, png, polycool, postcard, powerfmt, ppv-lite86, proc-macro-hack, proc-macro2, quick-error, quote, rand, rand_chacha, rand_core, rand_distr, rayon, rayon-core, read-fonts, regex, regex-automata, regex-syntax, roxmltree, scopeguard, semver, serde, serde_core, serde_derive, serde_json, serde_path_to_error, serde_spanned, serde_yaml, siphasher, skrifa, smallvec, stable_deref_trait, subsetter, syn, tar, thin-vec, thiserror, thiserror-impl, time, time-core, time-macros, toml, toml_datetime, toml_edit, ttf-parser, two-face, unic-langid, unic-langid-impl, unic-langid-macros, unic-langid-macros-impl, unicode-bidi, unicode-bidi-mirroring, unicode-ccc, unicode-math-class, unicode-normalization, unicode-properties, unicode-script, unicode-segmentation, unicode-vo, unscanny, url, wasm-bindgen, wasm-bindgen-macro, wasm-bindgen-macro-support, wasm-bindgen-shared, wasmi, wasmi_collections, wasmi_core, wasmi_ir, weezl, write-fonts, xmp-writer, yaml-rust | MIT OR Apache-2.0 |
| bincode, chinese-number, chinese-variant, color_quant, core_maths, enum-ordinalize, enum-ordinalize-derive, fancy-regex, fax, float-cmp, fontdb, imagesize, infer, libm, lipsum, phf, phf_generator, phf_macros, phf_shared, pico-args, plist, quick-xml, rgb, rust_decimal, rustybuzz, simd-adler32, slab, spin, strict-num, strum, strum_macros, synstructure, syntect, tiff, typed-arena, unsafe-libyaml, winnow, xmlwriter, zmij | MIT |
| bit-set, bit-vec, color, equivalent, fastrand, fearless_simd, glifo, hayro, hayro-ccitt, hayro-cmap, hayro-interpret, hayro-jbig2, hayro-jpeg2000, hayro-postscript, hayro-svg, hayro-syntax, hayro-write, idna_adapter, indexmap, kurbo, linebender_resource_handle, peniko, pin-project-lite, portable-atomic, resvg, rustc-hash, simplecss, svgtypes, usvg, utf16_iter, utf8_iter, utf_types, vello_common, vello_cpu, write16 | Apache-2.0 OR MIT |
| icu_collator, icu_collator_data, icu_collections, icu_locale, icu_locale_core, icu_locale_data, icu_normalizer, icu_normalizer_data, icu_properties, icu_properties_data, icu_provider, icu_provider_blob, icu_segmenter, icu_segmenter_data, litemap, potential_utf, tinystr, writeable, yoke, yoke-derive, zerofrom, zerofrom-derive, zerotrie, zerovec, zerovec-derive | Unicode-3.0 |
| approx, ciborium, ciborium-io, ciborium-ll, codex, typst, typst-assets, typst-eval, typst-html, typst-kit, typst-layout, typst-library, typst-macros, typst-pdf, typst-realize, typst-svg, typst-syntax, typst-timing, typst-utils | Apache-2.0 |
| aho-corasick, byteorder, byteorder-lite, csv, csv-core, memchr, same-file, walkdir | Unlicense OR MIT |
| arrayref, kamadak-exif, mutate_once | BSD-2-Clause |
| foldhash, slotmap, zlib-rs | Zlib |
| tinyvec_macros, zune-core, zune-jpeg | MIT OR Apache-2.0 OR Zlib |
| moxcms, pic-scale, pxfm | BSD-3-Clause OR Apache-2.0 |
| bytemuck, bytemuck_derive, tinyvec | Zlib OR Apache-2.0 OR MIT |
| tiny-skia, tiny-skia-path | BSD-3-Clause |
| zerocopy, zerocopy-derive | BSD-2-Clause OR Apache-2.0 OR MIT |
| roman-numerals-rs | 0BSD OR CC0-1.0 |
| miniz_oxide | MIT OR Zlib OR Apache-2.0 |
| wasmparser | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| unicode-ident | (MIT OR Apache-2.0) AND Unicode-3.0 |
| ryu | Apache-2.0 OR BSL-1.0 |
| fnv | Apache-2.0  OR  MIT |
| adler2 | 0BSD OR MIT OR Apache-2.0 |

`oleafly_typst.wasm` embeds Typst's default fonts, from `typst-assets`:

| Font | License |
|---|---|
| New Computer Modern, New Computer Modern Math | GUST Font License |
| Libertinus Serif | SIL Open Font License 1.1 |
| DejaVu Sans Mono | Bitstream Vera Fonts License, with DejaVu changes in the public domain |

## Frontend (JavaScript / npm, bundled into the app)

| Package | License |
|---|---|
| @ai-sdk/anthropic, @ai-sdk/openai, @ai-sdk/react, ai | Apache-2.0 |
| @tauri-apps/api | Apache-2.0 OR MIT |
| @tauri-apps/plugin-dialog / -process / -shell / -updater | MIT OR Apache-2.0 |
| @codemirror/* (autocomplete, commands, lang-*, language, legacy-modes, lint, search, state, view) | MIT |
| @xterm/xterm, @xterm/addon-fit / -search / -unicode11 / -webgl | MIT |
| @lezer/highlight | MIT |
| @citation-js/core, @citation-js/plugin-bibtex, @citation-js/plugin-csl | MIT |
| @replit/codemirror-vim | MIT |
| @radix-ui/react-context-menu / -select / -slot | MIT |
| react, react-dom | MIT |
| react-markdown, remark-gfm | MIT |
| react-resizable-panels | MIT |
| zustand | MIT |
| zod | MIT |
| katex | MIT |
| mathjax-full | Apache-2.0 |
| xlsx (SheetJS) | Apache-2.0 |
| harper.js | Apache-2.0 |
| hunspell-asm | MIT |
| pdfjs-dist | Apache-2.0 |
| class-variance-authority | Apache-2.0 |
| clsx, tailwind-merge, cmdk | MIT |
| canvas-confetti | ISC |
| lucide-react | ISC |

## Fonts

| Font | Where | License |
|---|---|---|
| KaTeX fonts | math rendering (via `katex`) | MIT |
| Geist | UI typeface | SIL Open Font License 1.1 |

## Bundled data

| Data | Where | License |
|---|---|---|
| [unimathsymbols.txt](http://milde.users.sourceforge.net/LUCR/Math/), © 2011 Günter Milde | `public/latex-intelligence/unimath.json`, a modified form of that file | LPPL 1.3+ |
| [LaTeX Workshop](https://github.com/James-Yu/LaTeX-Workshop), © James Yu | compile-log parsers ported to TypeScript in `packages/latex/src/compile-log/` | MIT |
| [Citation Style Language styles](https://github.com/citation-style-language/styles) | eight bundled styles through Citation.js and `src/assets/csl/` | CC BY-SA 3.0 |

The LaTeX completion catalogs in `public/latex-intelligence` are generated by
`scripts/latex-corpus-build.mjs`, which reads each package's own source from a
TeX Live installation. They are our own output; `manifest.json` records the TeX
Live release they were built from.

## Adapted text

| Source | Where | License |
|---|---|---|
| [OpenResearch](https://github.com/alphaXiv/OpenResearch), Copyright (c) 2026 alphaXiv | ideas in the bundled `oleafly-experiment-loop` and `oleafly-literature-sweep` skills (`src-tauri/resources/skills/`) | MIT |

The skills are written in our own words. The MIT notice for OpenResearch:

```
MIT License

Copyright (c) 2026 alphaXiv

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

Attribution notices for Oleafly itself are in [NOTICE](NOTICE). If you
redistribute Oleafly or a derivative, keep this file and the notices it
references, per the terms of each component's license.
