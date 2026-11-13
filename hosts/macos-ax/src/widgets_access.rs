//! Opt-in, thread-affine native widget operations in one pinned application
//! window. Menu authority is limited to descendants of a scoped popup owner;
//! no application menu bar or coordinate/event-synthesis fallback is used.
use crate::{
    Error,
    controls::{ControlInfo, MAX_CONTROLS, MAX_SCOPE_BYTES},
    scope::AuxiliaryGrant,
    transport::{PackedStrings, RawControl, RawScope},
    widgets::{
        CheckState, WidgetAction, WidgetCapabilities, WidgetInfo, WidgetScope, WidgetsBackend,
    },
};
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawWidget {
    control: RawControl,
    parent: u64,
    checked: i32,
    selected: i32,
    position: i32,
    capabilities: u32,
    orientation_length: usize,
}
unsafe extern "C" {
    fn ll_widgets_permission(context: *mut c_void, detail: *mut i32) -> i32;
    fn ll_widgets_revoke(context: *mut c_void, detail: *mut i32) -> i32;
    fn ll_widgets_snapshot(
        context: *mut c_void,
        rows: *mut RawWidget,
        row_capacity: usize,
        bytes: *mut u8,
        byte_capacity: usize,
        out: *mut RawScope,
        detail: *mut i32,
    ) -> i32;
    fn ll_widgets_perform(
        context: *mut c_void,
        scope: u64,
        revision: u64,
        target: u64,
        operation: u32,
        item: u64,
        number: i32,
        text: *const u8,
        text_length: usize,
        detail: *mut i32,
    ) -> i32;
    fn ll_ax_destroy(context: *mut c_void) -> i32;
}
fn checked(status: i32, detail: i32) -> Result<(), Error> {
    match status {
        0 => Ok(()),
        1 => Err(Error::PermissionDenied),
        2 => Err(Error::Unavailable(
            "the pinned widget window is unavailable",
        )),
        3 => Err(Error::Invalid(
            "invalid widget metadata, state or membership",
        )),
        4 => Err(Error::Stale),
        5 => Err(Error::Invalid(
            "widget is disabled or does not support this operation",
        )),
        6 => Err(Error::Bounds),
        7 => Err(Error::WrongThread),
        8 => Err(Error::Api(detail)),
        9 => Err(Error::Uncertain(detail)),
        _ => Err(Error::Invalid("unknown native widget status")),
    }
}
fn unpack(raw: RawScope, rows: &[RawWidget], bytes: &[u8]) -> Result<WidgetScope, Error> {
    if raw.identity == 0 || raw.revision == 0 {
        return Err(Error::Invalid("invalid widget scope identity"));
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
        "truncated widget strings",
        "invalid widget UTF-8",
    );
    let mut widgets = Vec::with_capacity(raw.count);
    let mut previous = 0;
    for row in &rows[..raw.count] {
        let control = &row.control;
        if control.identity <= previous
            || control.identity == raw.identity
            || ![
                control.value_present,
                control.enabled,
                control.editable,
                control.pressable,
            ]
            .into_iter()
            .all(|flag| matches!(flag, 0 | 1))
            || (control.value_present == 0 && control.value_length != 0)
            || !(-1..=2).contains(&row.checked)
            || !(-1..=1).contains(&row.selected)
            || !(-1..=1000).contains(&row.position)
            || row.capabilities & !63 != 0
        {
            return Err(Error::Invalid("invalid native widget row"));
        }
        previous = control.identity;
        let key = strings.take(control.key_length)?;
        let role = strings.take(control.role_length)?;
        let identifier = strings.take(control.identifier_length)?;
        let label = strings.take(control.label_length)?;
        let value = strings.take(control.value_length)?;
        let orientation = strings.take(row.orientation_length)?;
        widgets.push(WidgetInfo {
            control: ControlInfo {
                identity: control.identity,
                key,
                role,
                identifier,
                label,
                value: (control.value_present == 1).then_some(value),
                enabled: control.enabled == 1,
                editable: control.editable == 1,
                pressable: control.pressable == 1,
            },
            checked: match row.checked {
                0 => Some(CheckState::Unchecked),
                1 => Some(CheckState::Checked),
                2 => Some(CheckState::Mixed),
                _ => None,
            },
            selected: match row.selected {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            },
            position: (row.position >= 0).then_some(row.position as u16),
            orientation: (!orientation.is_empty()).then_some(orientation),
            capabilities: WidgetCapabilities {
                open_menu: row.capabilities & 1 != 0,
                choose_menu_item: row.capabilities & 2 != 0,
                cancel_menu: row.capabilities & 4 != 0,
                set_checked: row.capabilities & 8 != 0,
                select_item: row.capabilities & 16 != 0,
                scroll: row.capabilities & 32 != 0,
            },
            parent: (row.parent != 0).then_some(row.parent),
        });
    }
    strings.finish("trailing widget metadata bytes")?;
    WidgetScope {
        identity: raw.identity,
        revision: raw.revision,
        widgets,
    }
    .normalize()
}

/// Construct with `ApplicationAccess::widgets`. Every call runs on the macOS
/// main thread. Snapshots own bounded scalar data; native authority stays here.
///
/// ```compile_fail
/// use leselang_macos_ax_host::widgets_access::WidgetsAccess;
/// fn transfer(access: WidgetsAccess) { std::thread::spawn(move || drop(access)); }
/// ```
pub struct WidgetsAccess {
    raw: NonNull<c_void>,
    auxiliary_grant: Option<AuxiliaryGrant>,
    _main_thread: PhantomData<Rc<()>>,
}
impl WidgetsAccess {
    /// SAFETY: exactly one retained native widget owner from a successful
    /// constructor must be transferred, on the macOS main thread.
    pub(crate) unsafe fn from_owned_raw_with_grant(
        raw: *mut c_void,
        auxiliary_grant: Option<AuxiliaryGrant>,
    ) -> Result<Self, Error> {
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::Invalid("missing native widget connection"))?,
            auxiliary_grant,
            _main_thread: PhantomData,
        })
    }
    /// Permanently retire this auxiliary scope and all other connections that
    /// share its grant. Already delivered effects are not undone.
    pub fn revoke_auxiliary(&self) -> Result<(), Error> {
        let grant = self
            .auxiliary_grant
            .as_ref()
            .ok_or(Error::Invalid("this widget scope has no auxiliary grant"))?;
        grant.revoke();
        let mut detail = 0;
        // SAFETY: this thread-affine native owner remains live. Revocation never
        // reads foreign objects and cannot grant or restore authority.
        checked(
            unsafe { ll_widgets_revoke(self.raw.as_ptr(), &mut detail) },
            detail,
        )
    }
    fn check_grant(&self) -> Result<(), Error> {
        if let Some(grant) = &self.auxiliary_grant {
            grant.check_active()?;
        }
        Ok(())
    }
}
impl WidgetsBackend for WidgetsAccess {
    fn snapshot(&self) -> Result<WidgetScope, Error> {
        self.check_grant()?;
        let mut rows = [RawWidget::default(); MAX_CONTROLS];
        let mut bytes = vec![0_u8; MAX_SCOPE_BYTES];
        let mut raw = RawScope::default();
        let mut detail = 0;
        // SAFETY: live thread-affine owner and correctly sized repr(C) buffers
        // stay writable throughout this synchronous native call.
        let status = unsafe {
            ll_widgets_snapshot(
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
        self.check_grant()?;
        let mut detail = 0;
        // SAFETY: live thread-affine owner; permission is queried, never granted.
        let status = unsafe { ll_widgets_permission(self.raw.as_ptr(), &mut detail) };
        checked(status, detail)
    }
    fn perform(
        &self,
        expected: &WidgetScope,
        target: u64,
        action: &WidgetAction,
    ) -> Result<(), Error> {
        self.check_grant()?;
        expected.admit(target, action)?;
        let (operation, item, number, text) = match action {
            WidgetAction::Press => (1, 0, 0, ""),
            WidgetAction::SetText(text) => (2, 0, 0, text.as_str()),
            WidgetAction::OpenMenu => (3, 0, 0, ""),
            WidgetAction::ChooseMenuItem { item } => (4, *item, 0, ""),
            WidgetAction::CancelMenu => (5, 0, 0, ""),
            WidgetAction::SetChecked(checked) => (6, 0, i32::from(*checked), ""),
            WidgetAction::SelectItem { item } => (7, *item, 0, ""),
            WidgetAction::Scroll(position) => (8, 0, i32::from(*position), ""),
        };
        let mut detail = 0;
        // SAFETY: borrowed text remains live; the native call re-enumerates the
        // complete scope and rechecks epoch, capabilities and exact membership
        // immediately before one AX mutation. Post-entry errors are uncertain.
        let status = unsafe {
            ll_widgets_perform(
                self.raw.as_ptr(),
                expected.identity,
                expected.revision,
                target,
                operation,
                item,
                number,
                text.as_ptr(),
                text.len(),
                &mut detail,
            )
        };
        checked(status, detail)
    }
}
impl Drop for WidgetsAccess {
    fn drop(&mut self) {
        // SAFETY: release this unique retained, thread-affine owner exactly once,
        // before Rust drops the grant retaining its native callback context.
        let _ = unsafe { ll_ax_destroy(self.raw.as_ptr()) };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row() -> RawWidget {
        RawWidget {
            control: RawControl {
                identity: 2,
                key_length: 1,
                role_length: 10,
                enabled: 1,
                ..RawControl::default()
            },
            checked: 1,
            selected: -1,
            position: -1,
            capabilities: 8,
            ..RawWidget::default()
        }
    }
    fn raw() -> RawScope {
        RawScope {
            identity: 1,
            revision: 1,
            count: 1,
            bytes_length: 11,
        }
    }
    #[test]
    fn widget_flags_and_typed_state_decode_without_coercion() {
        let scope = unpack(raw(), &[row()], b"kAXCheckBox").unwrap();
        assert_eq!(scope.widgets[0].checked, Some(CheckState::Checked));
        assert!(scope.widgets[0].capabilities.set_checked);
        assert_eq!(scope.widgets[0].control.value, None);
        assert!(scope.widgets[0].selected.is_none());
    }
    #[test]
    fn malformed_widget_rows_refuse_before_publication() {
        for bad in [
            RawWidget {
                checked: 3,
                ..row()
            },
            RawWidget {
                position: 1001,
                ..row()
            },
            RawWidget {
                selected: 2,
                ..row()
            },
            RawWidget {
                capabilities: 64,
                ..row()
            },
            RawWidget {
                parent: 99,
                ..row()
            },
            RawWidget {
                orientation_length: usize::MAX,
                ..row()
            },
        ] {
            assert!(unpack(raw(), &[bad], b"kAXCheckBox").is_err());
        }
        assert!(unpack(raw(), &[row()], b"kAXCheckBo").is_err());
        assert!(unpack(raw(), &[row()], b"kAXCheckBo\xff").is_err());
        assert!(
            unpack(
                RawScope {
                    bytes_length: 12,
                    ..raw()
                },
                &[row()],
                b"kAXCheckBoxx"
            )
            .is_err()
        );
        assert!(
            unpack(
                RawScope {
                    count: MAX_CONTROLS + 1,
                    ..raw()
                },
                &[row()],
                b"kAXCheckBox"
            )
            .is_err()
        );
    }
    #[test]
    fn malformed_scope_and_duplicate_id_rows_refuse() {
        assert!(
            unpack(
                RawScope {
                    revision: 0,
                    ..raw()
                },
                &[row()],
                b"kAXCheckBox"
            )
            .is_err()
        );
        assert!(
            unpack(
                RawScope {
                    count: 2,
                    bytes_length: 22,
                    ..raw()
                },
                &[row(), row()],
                b"kAXCheckBoxkAXCheckBox"
            )
            .is_err()
        );
        assert!(
            unpack(
                raw(),
                &[RawWidget {
                    control: RawControl {
                        identity: 1,
                        ..row().control
                    },
                    ..row()
                }],
                b"kAXCheckBox"
            )
            .is_err()
        );
    }
}
