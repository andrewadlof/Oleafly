//! A Typst [`World`] that lives in memory. The page pushes the project's
//! files in before each compile, and package archives once it has fetched
//! them; nothing here touches a file system or the network.
//!
//! One world serves every compile of a session, so Typst's incremental cache
//! carries over: [`BrowserWorld::compile`] resets the file store, which
//! re-reads changed files in place, instead of building a new world.
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use typst::diag::{FileError, FileResult, PackageError, Severity, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::package::PackageSpec;
use typst::syntax::{DiagSpan, FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_kit::files::{FileLoader, FileStore};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;

/// The one package namespace the browser can download from.
pub const PREVIEW_NAMESPACE: &str = "preview";

/// A package archive can't unpack to more than this, so a hostile or broken
/// archive can't exhaust the page's memory.
pub const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Nor hold more files than this.
pub const MAX_PACKAGE_FILES: usize = 10_000;

/// Font files in the project that Typst can read.
const FONT_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];

/// What a compile produced.
#[derive(Debug)]
pub enum Outcome {
    /// A PDF, with any warnings.
    Pdf {
        pdf: Vec<u8>,
        diagnostics: Vec<Diagnostic>,
    },
    /// No PDF: the errors and warnings that stopped it.
    Failed { diagnostics: Vec<Diagnostic> },
    /// The document imports packages that haven't been added yet. Add them
    /// and compile again.
    NeedsPackages { packages: Vec<PackageRef> },
}

/// One error or warning, located in the project where possible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub message: String,
    /// The project path, or `None` when the problem is in a package or has
    /// no place in a file.
    pub file: Option<String>,
    /// 1-based, like the editor's lines.
    pub line: Option<usize>,
    /// Where it is when that isn't a project file, such as a package file.
    pub location: Option<String>,
    pub hints: Vec<String>,
}

/// A package the page should download, from
/// `<universe>/<namespace>/<name>-<version>.tar.gz`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PackageRef {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

impl PackageRef {
    fn of(spec: &PackageSpec) -> Self {
        Self {
            namespace: spec.namespace.to_string(),
            name: spec.name.to_string(),
            version: spec.version.to_string(),
        }
    }

    /// The `@namespace/name:version` form `import` uses.
    pub fn spec(&self) -> String {
        format!("@{}/{}:{}", self.namespace, self.name, self.version)
    }
}

/// The date the page gives for a compile: its local date for `datetime.today()`
/// and the current UTC time, for `datetime.today(offset: ..)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Today {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub utc_ms: f64,
}

/// Serves project and package files to Typst's file store.
struct Loader {
    main: FileId,
    files: BTreeMap<String, Bytes>,
    packages: BTreeMap<PackageRef, BTreeMap<String, Bytes>>,
    /// Packages Typst asked for during the current compile that aren't here.
    missing: Mutex<BTreeSet<PackageRef>>,
}

impl FileLoader for Loader {
    fn load(&self, id: FileId) -> FileResult<Bytes> {
        let path = id.vpath().get_without_slash();
        match id.root() {
            VirtualRoot::Project => self
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| FileError::NotFound(PathBuf::from(path))),
            VirtualRoot::Package(spec) => {
                if spec.namespace != PREVIEW_NAMESPACE {
                    return Err(FileError::Package(PackageError::Other(Some(
                        format!(
                            "only @{PREVIEW_NAMESPACE} packages can be used in the browser, not {spec}"
                        )
                        .into(),
                    ))));
                }
                let wanted = PackageRef::of(spec);
                match self.packages.get(&wanted) {
                    Some(files) => files.get(path).cloned().ok_or_else(|| {
                        FileError::NotFound(PathBuf::from(format!("{spec}/{path}")))
                    }),
                    None => {
                        if let Ok(mut missing) = self.missing.lock() {
                            missing.insert(wanted);
                        }
                        Err(FileError::Package(PackageError::NetworkFailed(Some(
                            "the package is still downloading".into(),
                        ))))
                    }
                }
            }
        }
    }
}

pub struct BrowserWorld {
    library: LazyHash<Library>,
    fonts: FontStore,
    /// Project fonts already in `fonts`, by path and content, so a font isn't
    /// added again on every compile.
    project_fonts: HashSet<(String, u64)>,
    store: FileStore<Loader>,
    today: Option<Today>,
}

impl Default for BrowserWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserWorld {
    pub fn new() -> Self {
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        let main = project_id("main.typ").unwrap_or_else(|| {
            FileId::unique(RootedPath::new(
                VirtualRoot::Project,
                VirtualPath::new("main.typ").expect("a plain file name is a valid path"),
            ))
        });
        Self {
            library: LazyHash::new(Library::builder().build()),
            fonts,
            project_fonts: HashSet::new(),
            store: FileStore::new(Loader {
                main,
                files: BTreeMap::new(),
                packages: BTreeMap::new(),
                missing: Mutex::new(BTreeSet::new()),
            }),
            today: None,
        }
    }

    /// Forget the project's files before the page pushes the next set.
    pub fn clear_files(&mut self) {
        self.store.loader_mut().files.clear();
    }

    /// Add or replace a project file. Fonts in the project become usable
    /// fonts too, as `--font-path` would make them.
    pub fn put_file(&mut self, path: &str, data: Vec<u8>) -> Result<(), String> {
        let path = clean_path(path).ok_or_else(|| format!("{path:?} isn't a project path"))?;
        let bytes = Bytes::new(data);
        if is_font(&path) {
            self.add_project_font(&path, &bytes);
        }
        self.store.loader_mut().files.insert(path, bytes);
        Ok(())
    }

    fn add_project_font(&mut self, path: &str, bytes: &Bytes) {
        let mut hasher = DefaultHasher::new();
        bytes.as_slice().hash(&mut hasher);
        if !self
            .project_fonts
            .insert((path.to_string(), hasher.finish()))
        {
            return;
        }
        for font in Font::iter(bytes.clone()) {
            let info = font.info().clone();
            self.fonts.push((font, info));
        }
    }

    pub fn has_package(&self, package: &PackageRef) -> bool {
        self.store.loader().packages.contains_key(package)
    }

    /// Unpack a package's `.tar.gz` and keep it for every later compile.
    pub fn add_package(&mut self, package: PackageRef, archive: &[u8]) -> Result<usize, String> {
        let files = unpack(archive)?;
        let count = files.len();
        self.store.loader_mut().packages.insert(package, files);
        Ok(count)
    }

    /// Compile `main` to a PDF.
    pub fn compile(&mut self, main: &str, today: Option<Today>) -> Outcome {
        let main = match clean_path(main).and_then(|path| project_id(&path)) {
            Some(id) => id,
            None => {
                return Outcome::Failed {
                    diagnostics: vec![plain_error(format!("{main:?} isn't a project path"))],
                }
            }
        };
        self.store.reset();
        self.today = today;
        {
            let loader = self.store.loader_mut();
            loader.main = main;
            if let Ok(missing) = loader.missing.get_mut() {
                missing.clear();
            }
        }

        let Warned { output, warnings } = typst::compile::<PagedDocument>(&*self);
        let outcome = match output {
            Ok(document) => match typst_pdf::pdf(&document, &PdfOptions::default()) {
                Ok(pdf) => Outcome::Pdf {
                    pdf,
                    diagnostics: self.diagnostics(&warnings),
                },
                Err(errors) => Outcome::Failed {
                    diagnostics: self.diagnostics(errors.iter().chain(&warnings)),
                },
            },
            Err(errors) => {
                let missing: Vec<PackageRef> = self
                    .store
                    .loader_mut()
                    .missing
                    .get_mut()
                    .map(|missing| std::mem::take(missing).into_iter().collect())
                    .unwrap_or_default();
                if missing.is_empty() {
                    Outcome::Failed {
                        diagnostics: self.diagnostics(errors.iter().chain(&warnings)),
                    }
                } else {
                    Outcome::NeedsPackages { packages: missing }
                }
            }
        };
        // Keep Typst's cache from growing without bound over a long session,
        // as `typst watch` does.
        comemo::evict(10);
        outcome
    }

    fn diagnostics<'a>(
        &self,
        list: impl IntoIterator<Item = &'a SourceDiagnostic>,
    ) -> Vec<Diagnostic> {
        list.into_iter()
            .map(|diagnostic| self.diagnostic(diagnostic))
            .collect()
    }

    fn diagnostic(&self, diagnostic: &SourceDiagnostic) -> Diagnostic {
        let (file, line, location) = self.locate(diagnostic.span);
        Diagnostic {
            severity: match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            message: diagnostic.message.to_string(),
            file,
            line,
            location,
            hints: diagnostic
                .hints
                .iter()
                .map(|hint| hint.v.to_string())
                .collect(),
        }
    }

    fn locate(&self, span: DiagSpan) -> (Option<String>, Option<usize>, Option<String>) {
        let Some(id) = span.id() else {
            return (None, None, None);
        };
        let line = self
            .range(span)
            .and_then(|range| self.source(id).ok()?.lines().byte_to_line(range.start))
            .map(|line| line + 1);
        match id.root() {
            VirtualRoot::Project => (Some(id.vpath().get_without_slash().to_string()), line, None),
            VirtualRoot::Package(spec) => {
                let place = format!("{spec}{}", id.vpath().get_with_slash());
                let place = match line {
                    Some(line) => format!("{place}:{line}"),
                    None => place,
                };
                (None, None, Some(place))
            }
        }
    }
}

impl World for BrowserWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.store.loader().main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.store.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.store.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let today = self.today?;
        match offset {
            None => Datetime::from_ymd(today.year, today.month, today.day),
            Some(offset) => {
                let shifted_ms = today.utc_ms + offset.seconds() * 1000.0;
                let (year, month, day) =
                    civil_from_days((shifted_ms / 86_400_000.0).floor() as i64);
                Datetime::from_ymd(year, month, day)
            }
        }
    }
}

fn project_id(path: &str) -> Option<FileId> {
    let vpath = VirtualPath::new(path).ok()?;
    Some(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

/// A project path as the page names it, without a leading slash or `./`.
fn clean_path(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./").trim_start_matches('/');
    if path.is_empty() || path.split('/').any(|part| part == ".." || part.is_empty()) {
        return None;
    }
    Some(path.to_string())
}

fn is_font(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        FONT_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
    })
}

fn plain_error(message: String) -> Diagnostic {
    Diagnostic {
        severity: "error",
        message,
        file: None,
        line: None,
        location: None,
        hints: Vec::new(),
    }
}

/// Unpack a Typst Universe `.tar.gz` into its files, keyed by path.
fn unpack(archive: &[u8]) -> Result<BTreeMap<String, Bytes>, String> {
    let decoder = flate2::read::GzDecoder::new(archive).take(MAX_PACKAGE_BYTES + 1);
    let mut tar = tar::Archive::new(decoder);
    let mut files = BTreeMap::new();
    let mut total: u64 = 0;
    let entries = tar
        .entries()
        .map_err(|error| format!("the package archive is unreadable: {error}"))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| format!("the package archive is unreadable: {error}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .map_err(|error| format!("a path in the package archive is unreadable: {error}"))?
            .to_string_lossy()
            .into_owned();
        let Some(path) = clean_path(&path) else {
            continue;
        };
        total = total.saturating_add(entry.size());
        if total > MAX_PACKAGE_BYTES || files.len() >= MAX_PACKAGE_FILES {
            return Err("the package is too large to use in the browser".to_string());
        }
        let mut data = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut data)
            .map_err(|error| format!("{path} in the package archive is unreadable: {error}"))?;
        files.insert(path, Bytes::new(data));
    }
    if files.is_empty() {
        return Err("the package archive holds no files".to_string());
    }
    Ok(files)
}

/// The proleptic Gregorian date of a day count since 1970-01-01, after
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with(files: &[(&str, &str)]) -> BrowserWorld {
        let mut world = BrowserWorld::new();
        for (path, text) in files {
            world.put_file(path, text.as_bytes().to_vec()).unwrap();
        }
        world
    }

    fn today() -> Option<Today> {
        Some(Today {
            year: 2026,
            month: 11,
            day: 16,
            utc_ms: 1_794_855_600_000.0,
        })
    }

    #[test]
    fn compiles_a_document_to_a_pdf() {
        let mut world = world_with(&[("main.typ", "= Hello\nSome *text* and $x^2$.")]);
        match world.compile("main.typ", today()) {
            Outcome::Pdf { pdf, diagnostics } => {
                assert!(pdf.starts_with(b"%PDF-"));
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
            }
            other => panic!("expected a PDF, got {other:?}"),
        }
    }

    #[test]
    fn includes_other_project_files() {
        let mut world = world_with(&[
            ("main.typ", "#include \"chapters/one.typ\""),
            ("chapters/one.typ", "= One"),
        ]);
        assert!(matches!(
            world.compile("main.typ", today()),
            Outcome::Pdf { .. }
        ));
    }

    #[test]
    fn locates_an_error_on_its_line() {
        let mut world = world_with(&[("main.typ", "ok\n\n#nonexistent()\n")]);
        let Outcome::Failed { diagnostics } = world.compile("main.typ", today()) else {
            panic!("expected a failure");
        };
        let error = &diagnostics[0];
        assert_eq!(error.severity, "error");
        assert_eq!(error.file.as_deref(), Some("main.typ"));
        assert_eq!(error.line, Some(3));
        assert!(error.message.contains("nonexistent"), "{}", error.message);
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_crash() {
        let mut world = world_with(&[("main.typ", "#include \"gone.typ\"")]);
        assert!(matches!(
            world.compile("main.typ", today()),
            Outcome::Failed { .. }
        ));
    }

    #[test]
    fn asks_for_a_package_then_uses_it() {
        let mut world = world_with(&[(
            "main.typ",
            "#import \"@preview/hello:0.1.0\": greet\n#greet()",
        )]);
        let Outcome::NeedsPackages { packages } = world.compile("main.typ", today()) else {
            panic!("expected a package request");
        };
        let wanted = PackageRef {
            namespace: "preview".into(),
            name: "hello".into(),
            version: "0.1.0".into(),
        };
        assert_eq!(packages, vec![wanted.clone()]);

        let archive = package_archive(&[
            (
                "typst.toml",
                "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\nauthors = [\"A\"]\nlicense = \"MIT\"\ndescription = \"Hi\"\n",
            ),
            ("lib.typ", "#let greet() = [Hello from a package]"),
        ]);
        assert_eq!(world.add_package(wanted.clone(), &archive), Ok(2));
        assert!(world.has_package(&wanted));
        match world.compile("main.typ", today()) {
            Outcome::Pdf { .. } => {}
            other => panic!("expected a PDF, got {other:?}"),
        }
    }

    #[test]
    fn refuses_packages_outside_preview() {
        let mut world = world_with(&[("main.typ", "#import \"@local/mine:0.1.0\": *")]);
        let Outcome::Failed { diagnostics } = world.compile("main.typ", today()) else {
            panic!("expected a failure");
        };
        assert!(
            diagnostics[0].message.contains("@preview"),
            "{:?}",
            diagnostics
        );
    }

    #[test]
    fn recompiles_after_an_edit() {
        let mut world = world_with(&[("main.typ", "#nonexistent")]);
        assert!(matches!(
            world.compile("main.typ", today()),
            Outcome::Failed { .. }
        ));
        world.clear_files();
        world.put_file("main.typ", b"Fixed".to_vec()).unwrap();
        assert!(matches!(
            world.compile("main.typ", today()),
            Outcome::Pdf { .. }
        ));
    }

    #[test]
    fn knows_today() {
        let mut world = world_with(&[(
            "main.typ",
            "#assert.eq(datetime.today().display(), \"2026-11-16\")",
        )]);
        assert!(matches!(
            world.compile("main.typ", today()),
            Outcome::Pdf { .. }
        ));
    }

    #[test]
    fn rejects_paths_that_leave_the_project() {
        let mut world = BrowserWorld::new();
        assert!(world.put_file("../etc/passwd", Vec::new()).is_err());
        assert!(world.put_file("a//b.typ", Vec::new()).is_err());
        assert_eq!(clean_path("./a/b.typ").as_deref(), Some("a/b.typ"));
    }

    #[test]
    fn unpacking_refuses_garbage() {
        assert!(unpack(b"not a gzip").is_err());
    }

    #[test]
    fn counts_days_like_the_calendar() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_773), (2026, 11, 16));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    fn package_archive(files: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, text) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, text.as_bytes())
                .unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar).unwrap();
        gz.finish().unwrap()
    }
}
