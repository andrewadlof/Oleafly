//! The browser app's Typst compiler: Typst built for WebAssembly behind a
//! small C ABI, which `web/src/typst` drives from a worker without a bindings
//! generator, the same way `oleafly-synctex-wasm` works.
//!
//! The page copies bytes in through `typst_alloc`. A compile goes:
//!
//! 1. `typst_clear_files`, then `typst_put_file` for each project file;
//! 2. `typst_compile` with the main file and today's date;
//! 3. on [`STATUS_NEEDS_PACKAGES`], fetch each listed package, hand it to
//!    `typst_add_package`, and compile again.
//!
//! Every call returns a status. A compile leaves the PDF, or the packages it
//! needs, in the output buffer (`typst_output_ptr`, `typst_output_len`) and
//! its errors and warnings as JSON in the diagnostics buffer
//! (`typst_diagnostics_ptr`, `typst_diagnostics_len`). A failure leaves its
//! message in the output buffer. The next call replaces both.
//!
//! The module is built with `panic = "abort"`, so a panic inside Typst is a
//! trap; the page then starts a fresh module. Nothing in this file panics on
//! bad input.
use std::cell::RefCell;

pub mod world;

use world::{BrowserWorld, Outcome, PackageRef, Today};

/// The compile failed: the diagnostics say why.
pub const STATUS_FAILED: i32 = 0;
/// The compile made a PDF, now in the output buffer.
pub const STATUS_PDF: i32 = 1;
/// The document imports packages that aren't here yet. The output buffer
/// holds them as a JSON list of `{namespace, name, version}`.
pub const STATUS_NEEDS_PACKAGES: i32 = 2;
pub const STATUS_BAD_INPUT: i32 = -1;
pub const STATUS_BAD_PACKAGE: i32 = -2;
pub const STATUS_INTERNAL: i32 = -6;

struct State {
    world: Option<BrowserWorld>,
    output: Vec<u8>,
    diagnostics: Vec<u8>,
}

impl State {
    const fn new() -> Self {
        Self {
            world: None,
            output: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// The world, made on first use: loading the bundled fonts takes a
    /// moment, and a reader who never compiles shouldn't pay for it.
    fn world(&mut self) -> &mut BrowserWorld {
        self.world.get_or_insert_with(BrowserWorld::new)
    }

    fn fail(&mut self, status: i32, message: impl Into<String>) -> i32 {
        self.output = message.into().into_bytes();
        self.diagnostics.clear();
        status
    }

    fn ok(&mut self) -> i32 {
        self.output.clear();
        self.diagnostics.clear();
        STATUS_PDF
    }
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
}

fn with_state(call: impl FnOnce(&mut State) -> i32) -> i32 {
    STATE
        .try_with(|cell| {
            cell.try_borrow_mut()
                .map_or(STATUS_INTERNAL, |mut state| call(&mut state))
        })
        .unwrap_or(STATUS_INTERNAL)
}

fn json<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"[]".to_vec())
}

pub fn clear_files() -> i32 {
    with_state(|state| {
        state.world().clear_files();
        state.ok()
    })
}

pub fn put_file(path: &[u8], data: &[u8]) -> i32 {
    with_state(|state| match std::str::from_utf8(path) {
        Err(_) => state.fail(STATUS_BAD_INPUT, "The file path isn't UTF-8."),
        Ok(path) => match state.world().put_file(path, data.to_vec()) {
            Ok(()) => state.ok(),
            Err(message) => state.fail(STATUS_BAD_INPUT, message),
        },
    })
}

fn package_ref(spec: &[u8]) -> Option<PackageRef> {
    let spec = std::str::from_utf8(spec).ok()?;
    let mut parts = spec.split('/');
    let (namespace, name, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || [namespace, name, version]
            .iter()
            .any(|part| part.is_empty())
    {
        return None;
    }
    Some(PackageRef {
        namespace: namespace.into(),
        name: name.into(),
        version: version.into(),
    })
}

/// `spec` is `namespace/name/version`.
pub fn has_package(spec: &[u8]) -> i32 {
    with_state(|state| match package_ref(spec) {
        None => state.fail(
            STATUS_BAD_INPUT,
            "The package isn't namespace/name/version.",
        ),
        Some(package) => i32::from(state.world().has_package(&package)),
    })
}

/// `spec` is `namespace/name/version`; `archive` is the package's `.tar.gz`.
pub fn add_package(spec: &[u8], archive: &[u8]) -> i32 {
    with_state(|state| match package_ref(spec) {
        None => state.fail(
            STATUS_BAD_INPUT,
            "The package isn't namespace/name/version.",
        ),
        Some(package) => {
            let spec = package.spec();
            match state.world().add_package(package, archive) {
                Ok(_) => state.ok(),
                Err(message) => state.fail(STATUS_BAD_PACKAGE, format!("{spec}: {message}")),
            }
        }
    })
}

pub fn compile(main: &[u8], today: Option<Today>) -> i32 {
    with_state(|state| {
        let Ok(main) = std::str::from_utf8(main) else {
            return state.fail(STATUS_BAD_INPUT, "The main file's path isn't UTF-8.");
        };
        match state.world().compile(main, today) {
            Outcome::Pdf { pdf, diagnostics } => {
                state.output = pdf;
                state.diagnostics = json(&diagnostics);
                STATUS_PDF
            }
            Outcome::Failed { diagnostics } => {
                state.output.clear();
                state.diagnostics = json(&diagnostics);
                STATUS_FAILED
            }
            Outcome::NeedsPackages { packages } => {
                state.output = json(&packages);
                state.diagnostics.clear();
                STATUS_NEEDS_PACKAGES
            }
        }
    })
}

fn bad_pointer() -> i32 {
    with_state(|state| state.fail(STATUS_BAD_INPUT, "A buffer pointer was null."))
}

/// # Safety
/// `ptr` is null or points to `len` readable bytes that outlive the call.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller promises `len` readable bytes at `ptr`.
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

#[no_mangle]
pub extern "C" fn typst_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::NonNull::dangling().as_ptr();
    }
    match std::alloc::Layout::array::<u8>(len) {
        // SAFETY: the layout has a non-zero size.
        Ok(layout) => unsafe { std::alloc::alloc(layout) },
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `ptr` and `len` come from one `typst_alloc` call, freed once.
#[no_mangle]
pub unsafe extern "C" fn typst_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = std::alloc::Layout::array::<u8>(len) {
        // SAFETY: the caller passes back what `typst_alloc` gave out.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

#[no_mangle]
pub extern "C" fn typst_clear_files() -> i32 {
    clear_files()
}

/// # Safety
/// Each pointer is null or points to its length in readable bytes.
#[no_mangle]
pub unsafe extern "C" fn typst_put_file(
    path: *const u8,
    path_len: usize,
    data: *const u8,
    data_len: usize,
) -> i32 {
    // SAFETY: passed on from the caller.
    match unsafe { (bytes(path, path_len), bytes(data, data_len)) } {
        (Some(path), Some(data)) => put_file(path, data),
        _ => bad_pointer(),
    }
}

/// # Safety
/// `spec` is null or points to `spec_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn typst_has_package(spec: *const u8, spec_len: usize) -> i32 {
    // SAFETY: passed on from the caller.
    match unsafe { bytes(spec, spec_len) } {
        Some(spec) => has_package(spec),
        None => bad_pointer(),
    }
}

/// # Safety
/// Each pointer is null or points to its length in readable bytes.
#[no_mangle]
pub unsafe extern "C" fn typst_add_package(
    spec: *const u8,
    spec_len: usize,
    archive: *const u8,
    archive_len: usize,
) -> i32 {
    // SAFETY: passed on from the caller.
    match unsafe { (bytes(spec, spec_len), bytes(archive, archive_len)) } {
        (Some(spec), Some(archive)) => add_package(spec, archive),
        _ => bad_pointer(),
    }
}

/// `year` 0 means the page gave no date, and `datetime.today()` fails.
///
/// # Safety
/// `main` is null or points to `main_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn typst_compile(
    main: *const u8,
    main_len: usize,
    year: i32,
    month: i32,
    day: i32,
    utc_ms: f64,
) -> i32 {
    let today = match (u8::try_from(month), u8::try_from(day)) {
        (Ok(month), Ok(day)) if year != 0 && utc_ms.is_finite() => Some(Today {
            year,
            month,
            day,
            utc_ms,
        }),
        _ => None,
    };
    // SAFETY: passed on from the caller.
    match unsafe { bytes(main, main_len) } {
        Some(main) => compile(main, today),
        None => bad_pointer(),
    }
}

#[no_mangle]
pub extern "C" fn typst_output_ptr() -> *const u8 {
    STATE
        .try_with(|cell| {
            cell.try_borrow()
                .map_or(std::ptr::null(), |state| state.output.as_ptr())
        })
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn typst_output_len() -> usize {
    STATE
        .try_with(|cell| cell.try_borrow().map_or(0, |state| state.output.len()))
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn typst_diagnostics_ptr() -> *const u8 {
    STATE
        .try_with(|cell| {
            cell.try_borrow()
                .map_or(std::ptr::null(), |state| state.diagnostics.as_ptr())
        })
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn typst_diagnostics_len() -> usize {
    STATE
        .try_with(|cell| cell.try_borrow().map_or(0, |state| state.diagnostics.len()))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> Vec<u8> {
        STATE.with(|cell| cell.borrow().output.clone())
    }

    fn diagnostics() -> serde_json::Value {
        STATE.with(|cell| serde_json::from_slice(&cell.borrow().diagnostics).unwrap())
    }

    #[test]
    fn compiles_through_the_abi() {
        assert_eq!(clear_files(), STATUS_PDF);
        assert_eq!(put_file(b"main.typ", b"Hello"), STATUS_PDF);
        assert_eq!(compile(b"main.typ", None), STATUS_PDF);
        assert!(output().starts_with(b"%PDF-"));
        assert_eq!(diagnostics(), serde_json::json!([]));
    }

    #[test]
    fn reports_a_failure_as_json() {
        clear_files();
        put_file(b"main.typ", b"#oops");
        assert_eq!(compile(b"main.typ", None), STATUS_FAILED);
        assert_eq!(diagnostics()[0]["file"], "main.typ");
        assert_eq!(diagnostics()[0]["line"], 1);
    }

    #[test]
    fn lists_the_packages_it_needs() {
        clear_files();
        put_file(b"main.typ", b"#import \"@preview/cetz:0.3.4\"");
        assert_eq!(compile(b"main.typ", None), STATUS_NEEDS_PACKAGES);
        let wanted: serde_json::Value = serde_json::from_slice(&output()).unwrap();
        assert_eq!(
            wanted,
            serde_json::json!([{"namespace": "preview", "name": "cetz", "version": "0.3.4"}])
        );
        assert_eq!(has_package(b"preview/cetz/0.3.4"), 0);
    }

    #[test]
    fn refuses_a_bad_package_spec_and_archive() {
        assert_eq!(has_package(b"cetz"), STATUS_BAD_INPUT);
        assert_eq!(
            add_package(b"preview/cetz/0.3.4", b"junk"),
            STATUS_BAD_PACKAGE
        );
        assert!(String::from_utf8(output())
            .unwrap()
            .starts_with("@preview/cetz:0.3.4:"));
    }

    #[test]
    fn rejects_a_path_outside_the_project() {
        assert_eq!(put_file(b"../x.typ", b""), STATUS_BAD_INPUT);
        assert_eq!(put_file(&[0xff], b""), STATUS_BAD_INPUT);
    }
}
