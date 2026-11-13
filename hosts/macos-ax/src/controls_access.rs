//! Bounded, thread-affine discovery and control of an explicitly pinned window.
//! Only the four documented roles are admitted. AXPress is available only on
//! AXButton; AXValue writes only on settable AXTextArea/AXTextField. Labels use
//! AXTitle, falling back to AXDescription only on buttons/static
//! text. AXSubrole and editor/container descriptions are outside the projection.
//! If AXEnabled is absent, only text controls infer enabled from a successful
//! settable AXValue query; an explicit false always blocks mutation.
//! String values are copied without coercion. Other scalar AX values remain native revision
//! dependencies and are represented by `None` in the public optional string.
use crate::{
    Error,
    controls::{ControlInfo, ControlsBackend, MAX_CONTROLS, MAX_SCOPE_BYTES, ScopeSnapshot},
    transport::{PackedStrings, RawControl, RawScope},
};
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;
use std::{ffi::c_void, marker::PhantomData, path::Path, ptr::NonNull, rc::Rc};

unsafe extern "C" {
    fn ll_controls_connect(
        kind: u32,
        path: *const u8,
        length: usize,
        out: *mut *mut c_void,
        detail: *mut i32,
    ) -> i32;
    fn ll_controls_permission(context: *mut c_void, detail: *mut i32) -> i32;
    fn ll_controls_snapshot(
        context: *mut c_void,
        rows: *mut RawControl,
        row_capacity: usize,
        bytes: *mut u8,
        byte_capacity: usize,
        out: *mut RawScope,
        detail: *mut i32,
    ) -> i32;
    fn ll_controls_press(
        context: *mut c_void,
        scope: u64,
        revision: u64,
        target: u64,
        detail: *mut i32,
    ) -> i32;
    fn ll_controls_set_text(
        context: *mut c_void,
        scope: u64,
        revision: u64,
        target: u64,
        text: *const u8,
        length: usize,
        detail: *mut i32,
    ) -> i32;
    fn ll_ax_destroy(context: *mut c_void) -> i32;
}

fn checked(status: i32, detail: i32) -> Result<(), Error> {
    match status {
        0 => Ok(()),
        1 => Err(Error::PermissionDenied),
        2 => Err(Error::Unavailable(
            "the explicitly selected window is unavailable",
        )),
        3 => Err(Error::Invalid(match detail {
            -101 => "AX text control has a non-string value",
            -102 => "AX numeric value is not a finite supported number",
            -103 => "AX value has an unsupported Core Foundation type",
            -104 => "AXEnabled is not a Boolean",
            _ => "invalid or ambiguous Accessibility control metadata",
        })),
        4 => Err(Error::Stale),
        5 => Err(Error::NotEditable),
        6 => Err(Error::Bounds),
        7 => Err(Error::WrongThread),
        8 => Err(Error::Api(detail)),
        9 => Err(Error::Uncertain(detail)),
        _ => Err(Error::Invalid("unknown Accessibility bridge status")),
    }
}

fn unpack(raw: RawScope, rows: &[RawControl], bytes: &[u8]) -> Result<ScopeSnapshot, Error> {
    if raw.identity == 0 || raw.revision == 0 {
        return Err(Error::Invalid(
            "invalid Accessibility scope identity or revision",
        ));
    }
    if raw.count > MAX_CONTROLS
        || raw.count > rows.len()
        || raw.bytes_length > MAX_SCOPE_BYTES
        || raw.bytes_length > bytes.len()
    {
        return Err(Error::Bounds);
    }
    let mut strings = PackedStrings::new(
        &bytes[..raw.bytes_length],
        "truncated Accessibility strings",
        "invalid Accessibility UTF-8",
    );
    let mut controls = Vec::with_capacity(raw.count);
    let mut previous = 0;
    for row in &rows[..raw.count] {
        if row.identity == 0
            || row.identity == raw.identity
            || row.identity <= previous
            || ![row.value_present, row.enabled, row.editable, row.pressable]
                .into_iter()
                .all(|flag| matches!(flag, 0 | 1))
            || (row.value_present == 0 && row.value_length != 0)
        {
            return Err(Error::Invalid("invalid Accessibility control row"));
        }
        previous = row.identity;
        let key = strings.take(row.key_length)?;
        let role = strings.take(row.role_length)?;
        let identifier = strings.take(row.identifier_length)?;
        let label = strings.take(row.label_length)?;
        let value = strings.take(row.value_length)?;
        controls.push(ControlInfo {
            identity: row.identity,
            key,
            role,
            identifier,
            label,
            value: (row.value_present == 1).then_some(value),
            enabled: row.enabled == 1,
            editable: row.editable == 1,
            pressable: row.pressable == 1,
        });
    }
    strings.finish("trailing Accessibility snapshot bytes")?;
    ScopeSnapshot {
        identity: raw.identity,
        revision: raw.revision,
        controls,
    }
    .normalize()
}

/// A connection to one running process and one exact window. Each native call
/// must occur on the macOS main thread. The connection has a lifetime limit of
/// 128 identities (including retired controls), with 64 simultaneously present.
/// A failed complete observation retires the connection; create a new one to
/// discover a new scope, rather than reusing earlier references or keys.
///
/// ```compile_fail
/// use leselang_macos_ax_host::controls_access::ControlsAccess;
/// fn transfer(access: ControlsAccess) { std::thread::spawn(move || drop(access)); }
/// ```
pub struct ControlsAccess {
    raw: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl ControlsAccess {
    /// Take one retained native control context returned by the scope bridge.
    /// SAFETY: `raw` must be the sole transferred owner from a successful native
    /// constructor, and the call must occur on the macOS main thread.
    pub(crate) unsafe fn from_owned_raw(raw: *mut c_void) -> Result<Self, Error> {
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::Invalid("missing Accessibility connection"))?,
            _main_thread: PhantomData,
        })
    }

    /// Pin the already-open TextEdit window whose canonical AXDocument matches
    /// this existing local file. No title/focus fallback or application launch.
    pub fn document(path: &Path) -> Result<Self, Error> {
        let path = path
            .canonicalize()
            .map_err(|_| Error::Unavailable("document path does not resolve"))?;
        if !path.is_file() {
            return Err(Error::Invalid("document path is not a file"));
        }
        let text = path
            .to_str()
            .ok_or(Error::Invalid("document path is not UTF-8"))?;
        if text.len() > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        Self::connect(1, text.as_bytes())
    }
    /// Pin the sole AXWindow identified as `main` in the running system
    /// Calculator. No button whitelist, label language or Basic-mode profile.
    pub fn calculator() -> Result<Self, Error> {
        Self::connect(2, &[])
    }
    fn connect(kind: u32, path: &[u8]) -> Result<Self, Error> {
        let mut raw = std::ptr::null_mut();
        let mut detail = 0;
        // SAFETY: borrowed input and writable outputs remain live throughout
        // this synchronous call. Success transfers exactly one retained owner.
        let status =
            unsafe { ll_controls_connect(kind, path.as_ptr(), path.len(), &mut raw, &mut detail) };
        checked(status, detail)?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::Invalid("missing Accessibility connection"))?,
            _main_thread: PhantomData,
        })
    }
}
impl ControlsBackend for ControlsAccess {
    fn snapshot(&self) -> Result<ScopeSnapshot, Error> {
        let mut rows = [RawControl::default(); MAX_CONTROLS];
        let mut bytes = vec![0_u8; MAX_SCOPE_BYTES];
        let mut raw = RawScope::default();
        let mut detail = 0;
        // SAFETY: the retained owner is thread-affine and all repr(C) output
        // arrays are bounded, writable and alive for the synchronous call.
        let status = unsafe {
            ll_controls_snapshot(
                self.raw.as_ptr(),
                rows.as_mut_ptr(),
                rows.len(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut raw,
                &mut detail,
            )
        };
        checked(status, detail)?;
        unpack(raw, &rows, &bytes)
    }
    fn check_permission(&self) -> Result<(), Error> {
        let mut detail = 0;
        // SAFETY: live thread-affine owner; no permissions are requested.
        checked(
            unsafe { ll_controls_permission(self.raw.as_ptr(), &mut detail) },
            detail,
        )
    }
    fn press(&self, expected: &ScopeSnapshot, target: u64) -> Result<(), Error> {
        expected.validate()?;
        expected.control(target)?;
        let mut detail = 0;
        // SAFETY: the native bridge re-enumerates the complete pinned scope and
        // compares its checked epoch and the live target before AXPress.
        let status = unsafe {
            ll_controls_press(
                self.raw.as_ptr(),
                expected.identity,
                expected.revision,
                target,
                &mut detail,
            )
        };
        checked(status, detail)
    }
    fn set_text(&self, expected: &ScopeSnapshot, target: u64, text: &str) -> Result<(), Error> {
        expected.validate()?;
        expected.control(target)?;
        if text.len() > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        let mut detail = 0;
        // SAFETY: text remains borrowed throughout this synchronous call;
        // native preflight checks full scope, target role and settable AXValue.
        let status = unsafe {
            ll_controls_set_text(
                self.raw.as_ptr(),
                expected.identity,
                expected.revision,
                target,
                text.as_ptr(),
                text.len(),
                &mut detail,
            )
        };
        checked(status, detail)
    }
}
impl Drop for ControlsAccess {
    fn drop(&mut self) {
        // SAFETY: exactly one release of this retained, thread-affine owner.
        let _ = unsafe { ll_ax_destroy(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(bytes_length: usize) -> RawScope {
        RawScope {
            identity: 1,
            revision: 1,
            count: 1,
            bytes_length,
        }
    }
    fn button() -> RawControl {
        RawControl {
            identity: 2,
            key_length: 3,
            role_length: 8,
            label_length: 1,
            enabled: 1,
            pressable: 1,
            ..RawControl::default()
        }
    }
    #[test]
    fn bounded_rows_decode_without_coercing_absent_or_empty_values() {
        let row = button();
        let bytes = b"x/2AXButton8";
        let result = unpack(scope(bytes.len()), &[row], bytes).unwrap();
        assert_eq!(result.controls[0].label, "8");
        assert_eq!(result.controls[0].value, None);
        let present = RawControl {
            value_present: 1,
            ..row
        };
        assert_eq!(
            unpack(scope(bytes.len()), &[present], bytes)
                .unwrap()
                .controls[0]
                .value,
            Some(String::new())
        );
    }
    #[test]
    fn refuses_truncation_invalid_flags_ids_and_invalid_utf8() {
        let bytes = b"x/2AXButton8";
        assert!(unpack(scope(bytes.len()), &[button()], &bytes[..bytes.len() - 1]).is_err());
        for row in [
            RawControl {
                enabled: 2,
                ..button()
            },
            RawControl {
                identity: 1,
                ..button()
            },
            RawControl {
                value_length: 1,
                ..button()
            },
            RawControl {
                key_length: usize::MAX,
                ..button()
            },
        ] {
            assert!(unpack(scope(bytes.len()), &[row], bytes).is_err());
        }
        assert!(unpack(scope(bytes.len() + 1), &[button()], b"x/2AXButton8x").is_err());
        assert!(unpack(scope(bytes.len()), &[button()], b"x/2AXButton\xff").is_err());
        assert!(
            unpack(
                RawScope {
                    revision: 0,
                    ..scope(bytes.len())
                },
                &[button()],
                bytes
            )
            .is_err()
        );
        assert!(
            unpack(
                RawScope {
                    count: MAX_CONTROLS + 1,
                    ..scope(bytes.len())
                },
                &[button()],
                bytes
            )
            .is_err()
        );
    }
    #[test]
    fn refuses_unsorted_or_duplicate_native_identity_rows() {
        let bytes = b"x/2AXButton8x/2AXButton8";
        let raw = RawScope {
            count: 2,
            ..scope(bytes.len())
        };
        assert!(unpack(raw, &[button(), button()], bytes).is_err());
    }
}
