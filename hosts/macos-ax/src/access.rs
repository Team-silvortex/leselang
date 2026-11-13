//! Thread-affine Accessibility connection to one exact TextEdit document.
//! Neither object references nor native pointers are exported as language data.
use crate::{Backend, Error, Snapshot};
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;
use std::{ffi::c_void, marker::PhantomData, path::Path, ptr::NonNull, rc::Rc};

#[repr(C)]
#[derive(Default)]
struct RawSnapshot {
    identity: u64,
    document_length: usize,
    value_length: usize,
    editable: i32,
}
unsafe extern "C" {
    fn ll_ax_trusted() -> i32;
    fn ll_ax_connect(
        path: *const u8,
        length: usize,
        out: *mut *mut c_void,
        detail: *mut i32,
    ) -> i32;
    fn ll_ax_destroy(context: *mut c_void) -> i32;
    fn ll_ax_permission(context: *mut c_void, detail: *mut i32) -> i32;
    fn ll_ax_snapshot(
        context: *mut c_void,
        bytes: *mut u8,
        capacity: usize,
        out: *mut RawSnapshot,
        detail: *mut i32,
    ) -> i32;
    fn ll_ax_set_text(
        context: *mut c_void,
        identity: u64,
        document: *const u8,
        document_length: usize,
        expected_value: *const u8,
        expected_length: usize,
        expected_editable: i32,
        text: *const u8,
        text_length: usize,
        detail: *mut i32,
    ) -> i32;
}
fn checked(status: i32, detail: i32) -> Result<(), Error> {
    match status {
        0 => Ok(()),
        1 => Err(Error::PermissionDenied),
        2 => Err(Error::Unavailable(
            "the exact TextEdit document or text area is unavailable",
        )),
        3 => Err(Error::Invalid(
            "invalid or ambiguous Accessibility metadata",
        )),
        4 => Err(Error::Stale),
        5 => Err(Error::NotEditable),
        6 => Err(Error::Bounds),
        7 => Err(Error::WrongThread),
        8 => Err(Error::Api(detail)),
        9 => Err(Error::Uncertain(detail)),
        _ => Err(Error::Invalid("unknown Accessibility bridge status")),
    }
}
fn bounded(text: &str) -> Result<(), Error> {
    if text.len() > MAX_SCALAR_STRING_BYTES {
        Err(Error::Bounds)
    } else {
        Ok(())
    }
}

/// A pinned process, window and text area. Native calls require the macOS main
/// thread. Each operation rechecks live permission and process identity.
///
/// ```compile_fail
/// use leselang_macos_ax_host::access::Access;
/// fn transfer(access: Access) { std::thread::spawn(move || drop(access)); }
/// ```
pub struct Access {
    raw: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl Access {
    /// Report current trust without prompting or granting system permission.
    pub fn trusted() -> bool {
        // SAFETY: this query takes no pointers and never prompts or mutates UI.
        unsafe { ll_ax_trusted() == 1 }
    }
    /// Connect only to the already-open document at this canonical local path.
    /// No launch, focus fallback, permission prompt or automatic retry occurs.
    pub fn connect(path: &Path) -> Result<Self, Error> {
        let path = path
            .canonicalize()
            .map_err(|_| Error::Unavailable("document path does not resolve"))?;
        if !path.is_file() {
            return Err(Error::Invalid("document path is not a file"));
        }
        let text = path
            .to_str()
            .ok_or(Error::Invalid("document path is not UTF-8"))?;
        bounded(text)?;
        let mut raw = std::ptr::null_mut();
        let mut detail = 0;
        // SAFETY: inputs stay borrowed for this synchronous call; success
        // transfers exactly one retained context to this thread-affine owner.
        let status = unsafe { ll_ax_connect(text.as_ptr(), text.len(), &mut raw, &mut detail) };
        checked(status, detail)?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::Invalid("missing Accessibility connection"))?,
            _main_thread: PhantomData,
        })
    }
}
impl Backend for Access {
    fn check_permission(&self) -> Result<(), Error> {
        let mut detail = 0;
        // SAFETY: this is a live context held exclusively on its main thread.
        let status = unsafe { ll_ax_permission(self.raw.as_ptr(), &mut detail) };
        checked(status, detail)
    }
    fn snapshot(&self) -> Result<Snapshot, Error> {
        let mut bytes = [0u8; MAX_SCALAR_STRING_BYTES * 2];
        let mut raw = RawSnapshot::default();
        let mut detail = 0;
        // SAFETY: bounded writable buffer and valid repr(C) outputs; live owner.
        let status = unsafe {
            ll_ax_snapshot(
                self.raw.as_ptr(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut raw,
                &mut detail,
            )
        };
        checked(status, detail)?;
        if raw.identity == 0
            || raw.document_length == 0
            || raw.document_length > MAX_SCALAR_STRING_BYTES
            || raw.value_length > MAX_SCALAR_STRING_BYTES
            || !matches!(raw.editable, 0 | 1)
        {
            return Err(Error::Invalid("invalid Accessibility snapshot"));
        }
        let decode = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| Error::Invalid("invalid Accessibility UTF-8"))
        };
        let document = decode(&bytes[..raw.document_length])?;
        if !document.starts_with("file://") || document.contains('\0') {
            return Err(Error::Invalid("invalid Accessibility document URL"));
        }
        Ok(Snapshot {
            identity: raw.identity,
            document,
            role: "AXTextArea".into(),
            value: decode(&bytes[raw.document_length..raw.document_length + raw.value_length])?,
            editable: raw.editable != 0,
        })
    }
    fn set_text(&self, expected: &Snapshot, text: &str) -> Result<(), Error> {
        bounded(text)?;
        bounded(&expected.document)?;
        bounded(&expected.value)?;
        if expected.role != "AXTextArea" {
            return Err(Error::Invalid("expected an AXTextArea"));
        }
        let mut detail = 0;
        // SAFETY: every borrowed buffer remains alive for this synchronous call;
        // bridge revalidates pinned identities and full expected state before set.
        let status = unsafe {
            ll_ax_set_text(
                self.raw.as_ptr(),
                expected.identity,
                expected.document.as_ptr(),
                expected.document.len(),
                expected.value.as_ptr(),
                expected.value.len(),
                i32::from(expected.editable),
                text.as_ptr(),
                text.len(),
                &mut detail,
            )
        };
        checked(status, detail)
    }
}
impl Drop for Access {
    fn drop(&mut self) {
        // SAFETY: exactly one release on the main thread; no native mutation.
        let _ = unsafe { ll_ax_destroy(self.raw.as_ptr()) };
    }
}
