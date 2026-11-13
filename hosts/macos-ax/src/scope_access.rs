//! Pin a running application and explicitly select one of its windows.
//! Application paths resolve to exact main executables; a PID only refines that
//! match. Window keys are connection-local metadata. A selected ControlsAccess
//! owns its process/window references independently and survives this handle's
//! destruction, while every call still rechecks process identity and permission.
use crate::{
    Error,
    controls_access::ControlsAccess,
    scope::{
        ApplicationSpec, AuxiliaryGrant, AuxiliaryProcessSpec, MAX_AUXILIARY_PROCESSES,
        MAX_WINDOW_BYTES, MAX_WINDOWS, WindowInfo, WindowSelector, auxiliary_grant_active,
        validate_windows,
    },
    transport::PackedStrings,
};
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;
use std::{
    ffi::c_void,
    marker::PhantomData,
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawWindow {
    identity: u64,
    key_length: usize,
    title_length: usize,
    identifier_length: usize,
    document_length: usize,
    document_present: i32,
}
#[repr(C)]
#[derive(Default)]
struct RawInventory {
    count: usize,
    bytes_length: usize,
}
#[repr(C)]
struct RawAuxiliarySpec {
    pid: i32,
    start_seconds: u64,
    start_micros: u32,
    executable: *const u8,
    executable_length: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawAuxiliaryRow {
    pid: i32,
    start_seconds: u64,
    start_micros: u32,
    executable_length: usize,
    edge_count: usize,
}
unsafe extern "C" {
    fn ll_application_connect(
        bundle: *const u8,
        length: usize,
        selected_pid: i32,
        out: *mut *mut c_void,
        actual_pid: *mut i32,
        detail: *mut i32,
    ) -> i32;
    fn ll_application_windows(
        context: *mut c_void,
        rows: *mut RawWindow,
        row_capacity: usize,
        bytes: *mut u8,
        byte_capacity: usize,
        out: *mut RawInventory,
        detail: *mut i32,
    ) -> i32;
    fn ll_application_controls(
        context: *mut c_void,
        key: *const u8,
        key_length: usize,
        title: *const u8,
        title_length: usize,
        identifier: *const u8,
        identifier_length: usize,
        document: *const u8,
        document_length: usize,
        out: *mut *mut c_void,
        detail: *mut i32,
    ) -> i32;
    fn ll_application_widgets(
        context: *mut c_void,
        key: *const u8,
        key_length: usize,
        title: *const u8,
        title_length: usize,
        identifier: *const u8,
        identifier_length: usize,
        document: *const u8,
        document_length: usize,
        out: *mut *mut c_void,
        detail: *mut i32,
    ) -> i32;
    fn ll_application_widgets_with_auxiliary(
        context: *mut c_void,
        key: *const u8,
        key_length: usize,
        title: *const u8,
        title_length: usize,
        identifier: *const u8,
        identifier_length: usize,
        document: *const u8,
        document_length: usize,
        processes: *const RawAuxiliarySpec,
        process_count: usize,
        grant_check: unsafe extern "C" fn(*const c_void) -> i32,
        grant_context: *const c_void,
        out: *mut *mut c_void,
        detail: *mut i32,
    ) -> i32;
    fn ll_application_auxiliary_processes(
        context: *mut c_void,
        key: *const u8,
        key_length: usize,
        title: *const u8,
        title_length: usize,
        identifier: *const u8,
        identifier_length: usize,
        document: *const u8,
        document_length: usize,
        rows: *mut RawAuxiliaryRow,
        row_capacity: usize,
        bytes: *mut u8,
        byte_capacity: usize,
        out: *mut RawInventory,
        detail: *mut i32,
    ) -> i32;
    fn ll_ax_destroy(context: *mut c_void) -> i32;
}
fn checked(status: i32, detail: i32) -> Result<(), Error> {
    match status {
        0 => Ok(()),
        1 => Err(Error::PermissionDenied),
        2 => Err(Error::Unavailable(
            "the requested application instance or window is unavailable",
        )),
        3 => Err(Error::Invalid(
            "invalid or ambiguous application/window selection or metadata",
        )),
        4 => Err(Error::Stale),
        5 => Err(Error::NotEditable),
        6 => Err(Error::Bounds),
        7 => Err(Error::WrongThread),
        8 => Err(Error::Api(detail)),
        9 => Err(Error::Uncertain(detail)),
        _ => Err(Error::Invalid("unknown Accessibility scope bridge status")),
    }
}
fn unpack(raw: RawInventory, rows: &[RawWindow], bytes: &[u8]) -> Result<Vec<WindowInfo>, Error> {
    if raw.count > MAX_WINDOWS
        || raw.count > rows.len()
        || raw.bytes_length > MAX_WINDOW_BYTES
        || raw.bytes_length > bytes.len()
    {
        return Err(Error::Bounds);
    }
    let mut strings = PackedStrings::new(
        &bytes[..raw.bytes_length],
        "truncated window metadata",
        "invalid window metadata UTF-8",
    );
    let mut windows = Vec::with_capacity(raw.count);
    let mut previous = 0_u64;
    for row in &rows[..raw.count] {
        if row.identity <= previous
            || !matches!(row.document_present, 0 | 1)
            || (row.document_present == 0 && row.document_length != 0)
        {
            return Err(Error::Invalid("invalid window metadata row"));
        }
        previous = row.identity;
        let key = strings.take(row.key_length)?;
        let title = strings.take(row.title_length)?;
        let identifier = strings.take(row.identifier_length)?;
        let document = strings.take(row.document_length)?;
        windows.push(WindowInfo {
            key,
            title,
            identifier,
            document: (row.document_present == 1).then_some(document),
        });
    }
    strings.finish("trailing window metadata bytes")?;
    validate_windows(&windows)?;
    Ok(windows)
}
fn path_text(path: &Path) -> Result<&str, Error> {
    let text = path
        .to_str()
        .ok_or(Error::Invalid("scope path is not UTF-8"))?;
    if text.is_empty() || text.contains('\0') {
        return Err(Error::Invalid("invalid scope path"));
    }
    if text.len() > MAX_SCALAR_STRING_BYTES {
        return Err(Error::Bounds);
    }
    Ok(text)
}

fn unpack_auxiliary(
    raw: RawInventory,
    rows: &[RawAuxiliaryRow],
    bytes: &[u8],
) -> Result<Vec<AuxiliaryProcessSpec>, Error> {
    if raw.count > MAX_AUXILIARY_PROCESSES
        || raw.count > rows.len()
        || raw.bytes_length > MAX_AUXILIARY_PROCESSES * MAX_SCALAR_STRING_BYTES
        || raw.bytes_length > bytes.len()
    {
        return Err(Error::Bounds);
    }
    let mut strings = PackedStrings::new(
        &bytes[..raw.bytes_length],
        "truncated auxiliary executable",
        "invalid auxiliary executable UTF-8",
    );
    let mut processes: Vec<AuxiliaryProcessSpec> = Vec::with_capacity(raw.count);
    for row in &rows[..raw.count] {
        if row.pid <= 0 || row.edge_count == 0 || row.edge_count > 512 {
            return Err(Error::Invalid("invalid auxiliary process discovery row"));
        }
        let path = strings.take(row.executable_length)?;
        let process = AuxiliaryProcessSpec {
            pid: row.pid as u32,
            executable_path: path.into(),
            start_seconds: row.start_seconds,
            start_micros: row.start_micros,
        };
        process.validate()?;
        if processes.iter().any(|prior| prior.pid == process.pid) {
            return Err(Error::Invalid("duplicate auxiliary process discovery PID"));
        }
        processes.push(process);
    }
    strings.finish("trailing auxiliary executable bytes")?;
    Ok(processes)
}

fn canonical_document(path: &Path) -> Result<PathBuf, Error> {
    let canonical = path
        .canonicalize()
        .map_err(|_| Error::Unavailable("window document path does not resolve"))?;
    // macOS documents can be directory packages, including Stickies .rtfd.
    if !canonical.is_file() && !canonical.is_dir() {
        return Err(Error::Invalid(
            "window document path is not a file or directory package",
        ));
    }
    path_text(&canonical)?;
    Ok(canonical)
}

/// Own the normalized document path while borrowing the selector's other
/// fields. Every native constructor reads these strings synchronously.
struct PreparedWindowSelector<'a> {
    key: &'a str,
    title: &'a str,
    identifier: &'a str,
    document: String,
}
impl<'a> PreparedWindowSelector<'a> {
    fn new(selector: &'a WindowSelector) -> Result<Self, Error> {
        selector.validate()?;
        let (key, title, identifier, document) = match selector {
            WindowSelector::Key(key) => (key.as_str(), "", "", None),
            WindowSelector::Exact {
                title,
                identifier,
                document,
            } => (
                "",
                title.as_deref().unwrap_or(""),
                identifier.as_deref().unwrap_or(""),
                document.as_deref(),
            ),
        };
        let canonical = document.map(canonical_document).transpose()?;
        let document = canonical
            .as_deref()
            .map(path_text)
            .transpose()?
            .unwrap_or("")
            .to_owned();
        Ok(Self {
            key,
            title,
            identifier,
            document,
        })
    }

    fn parts(&self) -> (&str, &str, &str, &str) {
        (self.key, self.title, self.identifier, &self.document)
    }
}

/// Thread-affine connection to one already-running application instance.
/// Listing windows reads only their metadata, not their control trees. A failed
/// complete native listing retires its registry, preventing key resurrection.
///
/// ```compile_fail
/// use leselang_macos_ax_host::scope_access::ApplicationAccess;
/// fn transfer(access: ApplicationAccess) { std::thread::spawn(move || drop(access)); }
/// ```
pub struct ApplicationAccess {
    raw: NonNull<c_void>,
    pid: u32,
    _main_thread: PhantomData<Rc<()>>,
}
impl ApplicationAccess {
    /// Pin one same-user process running this bundle's exact main executable.
    /// Multiple matches require a PID. No application is launched or focused.
    pub fn connect(spec: &ApplicationSpec) -> Result<Self, Error> {
        spec.validate()?;
        let path = spec
            .bundle_path
            .canonicalize()
            .map_err(|_| Error::Unavailable("application bundle path does not resolve"))?;
        if !path.is_dir() {
            return Err(Error::Invalid("application bundle is not a directory"));
        }
        let text = path_text(&path)?;
        let mut raw = std::ptr::null_mut();
        let mut actual_pid = 0_i32;
        let mut detail = 0;
        // SAFETY: all borrowed input and output pointers are valid for this
        // synchronous call; success transfers exactly one retained context.
        let status = unsafe {
            ll_application_connect(
                text.as_ptr(),
                text.len(),
                spec.pid.map_or(0, |pid| pid as i32),
                &mut raw,
                &mut actual_pid,
                &mut detail,
            )
        };
        checked(status, detail)?;
        let raw = NonNull::new(raw).ok_or(Error::Invalid("missing application connection"))?;
        if actual_pid <= 0 {
            // SAFETY: release the just-transferred owner on invalid FFI output.
            unsafe {
                ll_ax_destroy(raw.as_ptr());
            }
            return Err(Error::Invalid("invalid connected process identity"));
        }
        Ok(Self {
            raw,
            pid: actual_pid as u32,
            _main_thread: PhantomData,
        })
    }
    /// Select a fresh native window scope with the opt-in widget catalog.
    /// This does not pass through the narrower controls inventory first.
    pub fn widgets(
        &self,
        selector: &WindowSelector,
    ) -> Result<crate::widgets_access::WidgetsAccess, Error> {
        self.widget_connection(selector, None)
    }
    /// Explicitly admit fixed auxiliary identities only along the selected
    /// window's reachable object tree. This never grants their other windows.
    pub fn widgets_with_auxiliary(
        &self,
        selector: &WindowSelector,
        grant: &AuxiliaryGrant,
    ) -> Result<crate::widgets_access::WidgetsAccess, Error> {
        grant.check_active()?;
        if grant
            .processes()
            .iter()
            .any(|process| process.pid == self.pid)
        {
            return Err(Error::Invalid(
                "the primary application is not an auxiliary process",
            ));
        }
        self.widget_connection(selector, Some(grant))
    }
    fn widget_connection(
        &self,
        selector: &WindowSelector,
        grant: Option<&AuxiliaryGrant>,
    ) -> Result<crate::widgets_access::WidgetsAccess, Error> {
        let prepared = PreparedWindowSelector::new(selector)?;
        let (key, title, identifier, document) = prepared.parts();
        let mut raw = std::ptr::null_mut();
        let mut detail = 0;
        // SAFETY: native factory uniquely resolves a window in this pinned
        // process and transfers one independent widget owner on success.
        let status = if let Some(grant) = grant {
            grant.check_active()?;
            let processes = grant
                .processes()
                .iter()
                .map(|process| {
                    let path = path_text(&process.executable_path)?;
                    Ok(RawAuxiliarySpec {
                        pid: process.pid as i32,
                        start_seconds: process.start_seconds,
                        start_micros: process.start_micros,
                        executable: path.as_ptr(),
                        executable_length: path.len(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            // SAFETY: bounded paths/rows and the grant stay live for this call.
            // Success retains the callback pointer; WidgetsAccess retains its
            // Rc state until the native owner has been destroyed.
            unsafe {
                ll_application_widgets_with_auxiliary(
                    self.raw.as_ptr(),
                    key.as_ptr(),
                    key.len(),
                    title.as_ptr(),
                    title.len(),
                    identifier.as_ptr(),
                    identifier.len(),
                    document.as_ptr(),
                    document.len(),
                    processes.as_ptr(),
                    processes.len(),
                    auxiliary_grant_active,
                    grant.callback_context(),
                    &mut raw,
                    &mut detail,
                )
            }
        } else {
            unsafe {
                ll_application_widgets(
                    self.raw.as_ptr(),
                    key.as_ptr(),
                    key.len(),
                    title.as_ptr(),
                    title.len(),
                    identifier.as_ptr(),
                    identifier.len(),
                    document.as_ptr(),
                    document.len(),
                    &mut raw,
                    &mut detail,
                )
            }
        };
        checked(status, detail)?;
        // SAFETY: only successful native ownership transfer reaches this point.
        unsafe {
            crate::widgets_access::WidgetsAccess::from_owned_raw_with_grant(raw, grant.cloned())
        }
    }
    /// Read identity metadata at foreign-process edges in this exact window.
    /// No foreign AX attributes or descendants are queried and nothing is
    /// approved; callers must explicitly construct an AuxiliaryGrant afterward.
    pub fn auxiliary_processes(
        &self,
        selector: &WindowSelector,
    ) -> Result<Vec<AuxiliaryProcessSpec>, Error> {
        let prepared = PreparedWindowSelector::new(selector)?;
        let (key, title, identifier, document) = prepared.parts();
        let mut rows = [RawAuxiliaryRow::default(); MAX_AUXILIARY_PROCESSES];
        let mut bytes = vec![0_u8; MAX_AUXILIARY_PROCESSES * MAX_SCALAR_STRING_BYTES];
        let mut inventory = RawInventory::default();
        let mut detail = 0;
        // SAFETY: borrowed selector and correctly bounded buffers remain live
        // throughout this synchronous metadata-only native traversal.
        let status = unsafe {
            ll_application_auxiliary_processes(
                self.raw.as_ptr(),
                key.as_ptr(),
                key.len(),
                title.as_ptr(),
                title.len(),
                identifier.as_ptr(),
                identifier.len(),
                document.as_ptr(),
                document.len(),
                rows.as_mut_ptr(),
                rows.len(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut inventory,
                &mut detail,
            )
        };
        checked(status, detail)?;
        unpack_auxiliary(inventory, &rows, &bytes)
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn windows(&self) -> Result<Vec<WindowInfo>, Error> {
        let mut rows = [RawWindow::default(); MAX_WINDOWS];
        let mut bytes = vec![0_u8; MAX_WINDOW_BYTES];
        let mut inventory = RawInventory::default();
        let mut detail = 0;
        // SAFETY: one live thread-affine owner, correctly sized repr(C) arrays
        // and outputs remain writable for the complete synchronous call.
        let status = unsafe {
            ll_application_windows(
                self.raw.as_ptr(),
                rows.as_mut_ptr(),
                rows.len(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut inventory,
                &mut detail,
            )
        };
        checked(status, detail)?;
        unpack(inventory, &rows, &bytes)
    }
    /// Resolve exactly one live window, then retain that window's native
    /// identity. Later metadata changes do not rerun the selector or redirect
    /// this connection to a newly appearing window with the original title.
    pub fn controls(&self, selector: &WindowSelector) -> Result<ControlsAccess, Error> {
        let prepared = PreparedWindowSelector::new(selector)?;
        let (key, title, identifier, document) = prepared.parts();
        let mut raw = std::ptr::null_mut();
        let mut detail = 0;
        // SAFETY: native selection freshly lists and uniquely resolves a window
        // in this pinned process; successful output transfers one independent
        // retained ControlsAccess owner with its own window/process checks.
        let status = unsafe {
            ll_application_controls(
                self.raw.as_ptr(),
                key.as_ptr(),
                key.len(),
                title.as_ptr(),
                title.len(),
                identifier.as_ptr(),
                identifier.len(),
                document.as_ptr(),
                document.len(),
                &mut raw,
                &mut detail,
            )
        };
        checked(status, detail)?;
        // SAFETY: only a successful native constructor reaches this transfer.
        unsafe { ControlsAccess::from_owned_raw(raw) }
    }
}
impl Drop for ApplicationAccess {
    fn drop(&mut self) {
        // SAFETY: exactly one release of this retained, main-thread owner.
        let _ = unsafe { ll_ax_destroy(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn auxiliary_row() -> RawAuxiliaryRow {
        RawAuxiliaryRow {
            pid: 42,
            start_seconds: 1_800_000_000,
            start_micros: 123,
            executable_length: 8,
            edge_count: 1,
        }
    }
    #[test]
    fn auxiliary_discovery_decodes_complete_identity_without_creating_approval() {
        let rows = [auxiliary_row()];
        let process = unpack_auxiliary(inventory(8), &rows, b"/Service")
            .unwrap()
            .remove(0);
        assert_eq!(process.pid, 42);
        assert_eq!(process.start_seconds, 1_800_000_000);
        assert_eq!(process.start_micros, 123);
        assert_eq!(process.executable_path, PathBuf::from("/Service"));
        assert!(
            unpack_auxiliary(RawInventory::default(), &[], &[])
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn malformed_auxiliary_inventory_fails_before_publishing_any_process() {
        for row in [
            RawAuxiliaryRow {
                pid: 0,
                ..auxiliary_row()
            },
            RawAuxiliaryRow {
                pid: -1,
                ..auxiliary_row()
            },
            RawAuxiliaryRow {
                start_seconds: 0,
                ..auxiliary_row()
            },
            RawAuxiliaryRow {
                start_micros: 1_000_000,
                ..auxiliary_row()
            },
            RawAuxiliaryRow {
                executable_length: 4097,
                ..auxiliary_row()
            },
            RawAuxiliaryRow {
                edge_count: 0,
                ..auxiliary_row()
            },
            RawAuxiliaryRow {
                edge_count: 513,
                ..auxiliary_row()
            },
        ] {
            assert!(unpack_auxiliary(inventory(8), &[row], b"/Service").is_err());
        }
        for bytes in [
            b"relative".as_slice(),
            b"/bad\0exe".as_slice(),
            b"/bad\xffexe".as_slice(),
        ] {
            assert!(unpack_auxiliary(inventory(8), &[auxiliary_row()], bytes).is_err());
        }
        assert!(unpack_auxiliary(inventory(7), &[auxiliary_row()], b"/Servic").is_err());
        assert!(unpack_auxiliary(inventory(9), &[auxiliary_row()], b"/ServiceX").is_err());
        assert!(
            unpack_auxiliary(
                RawInventory {
                    count: 5,
                    bytes_length: 0
                },
                &[auxiliary_row(); 5],
                &[]
            )
            .is_err()
        );
        assert!(
            unpack_auxiliary(
                RawInventory {
                    count: 2,
                    bytes_length: 16
                },
                &[auxiliary_row(); 2],
                b"/Service/Service"
            )
            .is_err()
        );
    }
    fn row() -> RawWindow {
        RawWindow {
            identity: 1,
            key_length: 3,
            title_length: 4,
            ..RawWindow::default()
        }
    }
    fn inventory(bytes_length: usize) -> RawInventory {
        RawInventory {
            count: 1,
            bytes_length,
        }
    }
    #[test]
    fn local_document_selection_accepts_files_and_directory_packages() {
        let directory = std::env::temp_dir().join(format!(
            "leselang-scope-document-{}.rtfd",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("document.rtf");
        std::fs::write(&file, b"test fixture").unwrap();
        for document in [&directory, &file] {
            let selector = WindowSelector::Exact {
                title: Some("Fixture".into()),
                identifier: Some("fixture-window".into()),
                document: Some(document.clone()),
            };
            let prepared = PreparedWindowSelector::new(&selector).unwrap();
            assert_eq!(
                prepared.parts(),
                (
                    "",
                    "Fixture",
                    "fixture-window",
                    document.canonicalize().unwrap().to_str().unwrap()
                )
            );
        }
        let key = WindowSelector::Key("fixture/key".into());
        assert_eq!(
            PreparedWindowSelector::new(&key).unwrap().parts(),
            ("fixture/key", "", "", "")
        );
        let invalid = WindowSelector::Exact {
            title: Some("invalid\0title".into()),
            identifier: None,
            document: Some(directory.join("missing")),
        };
        assert!(matches!(
            PreparedWindowSelector::new(&invalid),
            Err(Error::Invalid(_))
        ));
        assert!(canonical_document(&directory.join("missing")).is_err());
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn packed_inventory_preserves_missing_fields_and_document_urls() {
        let rows = [row()];
        let decoded = unpack(inventory(7), &rows, b"a/1Note").unwrap();
        assert_eq!(decoded[0].title, "Note");
        assert_eq!(decoded[0].document, None);
        let document = "https://example.test/document";
        let bytes = format!("a/1Note{document}");
        let rows = [RawWindow {
            document_present: 1,
            document_length: document.len(),
            ..row()
        }];
        assert_eq!(
            unpack(inventory(bytes.len()), &rows, bytes.as_bytes()).unwrap()[0]
                .document
                .as_deref(),
            Some(document)
        );
    }
    #[test]
    fn packed_inventory_rejects_truncation_flags_order_and_duplicate_keys() {
        for bad in [
            RawWindow {
                identity: 0,
                ..row()
            },
            RawWindow {
                document_present: 2,
                ..row()
            },
            RawWindow {
                document_length: 1,
                ..row()
            },
            RawWindow {
                key_length: usize::MAX,
                ..row()
            },
        ] {
            assert!(unpack(inventory(7), &[bad], b"a/1Note").is_err());
        }
        assert!(unpack(inventory(7), &[row()], b"a/1Not").is_err());
        assert!(unpack(inventory(8), &[row()], b"a/1Notes").is_err());
        let rows = [
            row(),
            RawWindow {
                identity: 2,
                ..row()
            },
        ];
        assert!(
            unpack(
                RawInventory {
                    count: 2,
                    bytes_length: 14
                },
                &rows,
                b"a/1Notea/1Note"
            )
            .is_err()
        );
        assert!(
            unpack(
                RawInventory {
                    count: 2,
                    bytes_length: 14
                },
                &[row(), row()],
                b"a/1Noteb/1Note"
            )
            .is_err()
        );
    }
    #[test]
    fn packed_inventory_rejects_invalid_text_and_bounds_before_publication() {
        assert!(unpack(inventory(7), &[row()], b"a/1Not\xff").is_err());
        assert!(unpack(inventory(7), &[row()], b"a/1No\0e").is_err());
        assert!(
            unpack(
                RawInventory {
                    count: MAX_WINDOWS + 1,
                    bytes_length: 0
                },
                &[],
                &[]
            )
            .is_err()
        );
        assert!(
            unpack(
                RawInventory {
                    count: 0,
                    bytes_length: MAX_WINDOW_BYTES + 1
                },
                &[],
                &[]
            )
            .is_err()
        );
        assert!(
            unpack(
                inventory(7),
                &[RawWindow {
                    document_present: 1,
                    ..row()
                }],
                b"a/1Note"
            )
            .is_err()
        );
        assert!(
            unpack(
                inventory(7),
                &[RawWindow {
                    key_length: 4097,
                    ..row()
                }],
                b"a/1Note"
            )
            .is_err()
        );
        assert!(
            unpack(RawInventory::default(), &[], &[])
                .unwrap()
                .is_empty()
        );
    }
}
