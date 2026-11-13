//! A thread-affine native connection to the supported Calculator basic layout.
use crate::{
    Error,
    calculator::{ButtonState, CalculatorBackend, CalculatorSnapshot},
};
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

const BUTTONS: [&str; 4] = ["one", "add", "two", "equals"];
#[repr(C)]
#[derive(Default)]
struct RawSnapshot {
    identity: u64,
    display_length: usize,
    last_expression_length: usize,
    last_expression_present: i32,
    button_identities: [u64; 4],
    button_enabled: [i32; 4],
}
unsafe extern "C" {
    fn ll_calc_connect(out: *mut *mut c_void, detail: *mut i32) -> i32;
    fn ll_calc_snapshot(
        context: *mut c_void,
        bytes: *mut u8,
        capacity: usize,
        out: *mut RawSnapshot,
        detail: *mut i32,
    ) -> i32;
    fn ll_calc_press(
        context: *mut c_void,
        expected: *const RawSnapshot,
        display: *const u8,
        display_length: usize,
        last_expression: *const u8,
        last_expression_length: usize,
        button: u32,
        detail: *mut i32,
    ) -> i32;
    fn ll_ax_destroy(context: *mut c_void) -> i32;
    fn ll_ax_permission(context: *mut c_void, detail: *mut i32) -> i32;
}
fn checked(status: i32, detail: i32) -> Result<(), Error> {
    match status {
        0 => Ok(()),
        1 => Err(Error::PermissionDenied),
        2 => Err(Error::Unavailable(
            "the supported Calculator basic window is unavailable",
        )),
        3 => Err(Error::Invalid(
            "unsupported or ambiguous Calculator Accessibility layout",
        )),
        4 => Err(Error::Stale),
        5 => Err(Error::Invalid(
            "Calculator button is disabled or does not support AXPress",
        )),
        6 => Err(Error::Bounds),
        7 => Err(Error::WrongThread),
        8 => Err(Error::Api(detail)),
        9 => Err(Error::Uncertain(detail)),
        _ => Err(Error::Invalid("unknown Calculator bridge status")),
    }
}
fn validate_raw(raw: &RawSnapshot) -> Result<(), Error> {
    if raw.display_length > MAX_SCALAR_STRING_BYTES
        || raw.last_expression_length > MAX_SCALAR_STRING_BYTES
    {
        return Err(Error::Bounds);
    }
    if raw.identity == 0
        || !matches!(raw.last_expression_present, 0 | 1)
        || (raw.last_expression_present == 0 && raw.last_expression_length != 0)
        || raw
            .button_identities
            .iter()
            .enumerate()
            .any(|(index, value)| {
                *value == 0
                    || *value == raw.identity
                    || raw.button_identities[..index].contains(value)
            })
        || raw
            .button_enabled
            .iter()
            .any(|value| !matches!(*value, 0 | 1))
    {
        return Err(Error::Invalid("invalid Calculator snapshot metadata"));
    }
    Ok(())
}
fn expected_raw(snapshot: &CalculatorSnapshot) -> Result<RawSnapshot, Error> {
    if snapshot.buttons.len() != BUTTONS.len() {
        return Err(Error::Invalid("invalid Calculator button set"));
    }
    let mut raw = RawSnapshot {
        identity: snapshot.identity,
        display_length: snapshot.display.len(),
        last_expression_length: snapshot.last_expression.as_ref().map_or(0, String::len),
        last_expression_present: i32::from(snapshot.last_expression.is_some()),
        ..RawSnapshot::default()
    };
    for (index, name) in BUTTONS.iter().enumerate() {
        let mut matches = snapshot
            .buttons
            .iter()
            .filter(|button| button.name == *name);
        let button = matches
            .next()
            .ok_or(Error::Invalid("missing Calculator button"))?;
        if matches.next().is_some() {
            return Err(Error::Invalid("duplicate Calculator button"));
        }
        raw.button_identities[index] = button.identity;
        raw.button_enabled[index] = i32::from(button.enabled);
    }
    validate_raw(&raw)?;
    Ok(raw)
}

/// A pinned Calculator process, basic-mode window, display and button set.
/// Reads and explicit presses remain on the process's main thread.
///
/// ```compile_fail
/// use leselang_macos_ax_host::calculator_access::CalculatorAccess;
/// fn transfer(access: CalculatorAccess) { std::thread::spawn(move || drop(access)); }
/// ```
pub struct CalculatorAccess {
    raw: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl CalculatorAccess {
    /// Connect to the already-running system Calculator. No launch, focus
    /// fallback, permission grant or automatic retry is performed.
    pub fn connect() -> Result<Self, Error> {
        let mut raw = std::ptr::null_mut();
        let mut detail = 0;
        // SAFETY: writable outputs are valid, and success transfers exactly one
        // retained native owner which cannot be moved to another thread.
        let status = unsafe { ll_calc_connect(&mut raw, &mut detail) };
        checked(status, detail)?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::Invalid("missing Calculator connection"))?,
            _main_thread: PhantomData,
        })
    }
}
impl CalculatorBackend for CalculatorAccess {
    fn check_permission(&self) -> Result<(), Error> {
        let mut detail = 0;
        // SAFETY: the live context is held exclusively on the main thread.
        checked(
            unsafe { ll_ax_permission(self.raw.as_ptr(), &mut detail) },
            detail,
        )
    }
    fn snapshot(&self) -> Result<CalculatorSnapshot, Error> {
        let mut bytes = [0u8; MAX_SCALAR_STRING_BYTES * 2];
        let mut raw = RawSnapshot::default();
        let mut detail = 0;
        // SAFETY: bounded writable bytes, valid repr(C) output and live owner.
        let status = unsafe {
            ll_calc_snapshot(
                self.raw.as_ptr(),
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut raw,
                &mut detail,
            )
        };
        checked(status, detail)?;
        validate_raw(&raw)?;
        let display = std::str::from_utf8(&bytes[..raw.display_length])
            .map_err(|_| Error::Invalid("invalid Calculator display UTF-8"))?
            .to_owned();
        let last_expression = if raw.last_expression_present == 1 {
            Some(
                std::str::from_utf8(
                    &bytes[raw.display_length..raw.display_length + raw.last_expression_length],
                )
                .map_err(|_| Error::Invalid("invalid Calculator last expression UTF-8"))?
                .to_owned(),
            )
        } else {
            None
        };
        Ok(CalculatorSnapshot {
            identity: raw.identity,
            display,
            last_expression,
            buttons: BUTTONS
                .iter()
                .enumerate()
                .map(|(index, name)| ButtonState {
                    name: (*name).into(),
                    identity: raw.button_identities[index],
                    enabled: raw.button_enabled[index] != 0,
                })
                .collect(),
        })
    }
    fn press(&self, expected: &CalculatorSnapshot, button: &str) -> Result<(), Error> {
        let button = BUTTONS
            .iter()
            .position(|name| *name == button)
            .ok_or(Error::Invalid("unknown Calculator button"))? as u32;
        let raw = expected_raw(expected)?;
        let last_expression = expected.last_expression.as_deref().unwrap_or("");
        let mut detail = 0;
        // SAFETY: every borrowed input lives for the synchronous native call;
        // native code checks all pinned state again before entering AXPress.
        let status = unsafe {
            ll_calc_press(
                self.raw.as_ptr(),
                &raw,
                expected.display.as_ptr(),
                expected.display.len(),
                last_expression.as_ptr(),
                last_expression.len(),
                button,
                &mut detail,
            )
        };
        checked(status, detail)
    }
}
impl Drop for CalculatorAccess {
    fn drop(&mut self) {
        // SAFETY: exactly one main-thread release; destruction does not press UI.
        let _ = unsafe { ll_ax_destroy(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> CalculatorSnapshot {
        CalculatorSnapshot {
            identity: 1,
            display: "0".into(),
            last_expression: None,
            buttons: BUTTONS
                .iter()
                .enumerate()
                .map(|(index, name)| ButtonState {
                    name: (*name).into(),
                    identity: index as u64 + 2,
                    enabled: true,
                })
                .collect(),
        }
    }
    #[test]
    fn bridge_inputs_require_the_exact_named_button_set() {
        let mut value = snapshot();
        value.buttons.reverse();
        let raw = expected_raw(&value).unwrap();
        assert_eq!(raw.button_identities, [2, 3, 4, 5]);
        value.buttons[0].name = "one".into();
        assert!(expected_raw(&value).is_err());
        let mut value = snapshot();
        value.buttons[1].identity = value.buttons[0].identity;
        assert!(expected_raw(&value).is_err());
        let mut value = snapshot();
        value.display = "x".repeat(MAX_SCALAR_STRING_BYTES + 1);
        assert!(matches!(expected_raw(&value), Err(Error::Bounds)));
    }
    #[test]
    fn malformed_native_metadata_and_uncertain_status_are_preserved() {
        let mut raw = expected_raw(&snapshot()).unwrap();
        raw.button_enabled[0] = 2;
        assert!(validate_raw(&raw).is_err());
        raw.button_enabled[0] = 1;
        raw.button_identities[0] = raw.identity;
        assert!(validate_raw(&raw).is_err());
        assert!(matches!(checked(9, -25204), Err(Error::Uncertain(-25204))));
        assert!(matches!(checked(8, -25204), Err(Error::Api(-25204))));
    }
    #[test]
    fn optional_expression_presence_and_bounds_are_preserved() {
        let mut value = snapshot();
        value.last_expression = Some(String::new());
        let raw = expected_raw(&value).unwrap();
        assert_eq!(raw.last_expression_present, 1);
        assert_eq!(raw.last_expression_length, 0);
        value.last_expression = Some("1 + 2".into());
        let mut raw = expected_raw(&value).unwrap();
        assert_eq!(raw.last_expression_length, 5);
        raw.last_expression_present = 0;
        assert!(validate_raw(&raw).is_err());
        raw.last_expression_present = 2;
        assert!(validate_raw(&raw).is_err());
        value.last_expression = Some("x".repeat(MAX_SCALAR_STRING_BYTES + 1));
        assert!(matches!(expected_raw(&value), Err(Error::Bounds)));
    }
}
