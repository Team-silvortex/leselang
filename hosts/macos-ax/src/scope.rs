//! Explicit application and window selectors. These values describe a lookup;
//! only a live native connection can turn a match into control authority.
use crate::Error;
use leselang_vm::native::core::MAX_SCALAR_STRING_BYTES;
#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};

pub const MAX_WINDOWS: usize = 64;
pub const MAX_WINDOW_BYTES: usize = 64 * 1024;
pub const MAX_AUXILIARY_PROCESSES: usize = 4;

/// An exact already-running process identity, never a native object capability.
/// The native connector additionally verifies same-user ownership, the canonical
/// executable and start time, then admits only edges from the selected window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuxiliaryProcessSpec {
    pub pid: u32,
    pub executable_path: PathBuf,
    pub start_seconds: u64,
    pub start_micros: u32,
}
impl AuxiliaryProcessSpec {
    pub fn validate(&self) -> Result<(), Error> {
        validate_path(&self.executable_path)?;
        if !self.executable_path.is_absolute() {
            return Err(Error::Invalid("auxiliary executable path must be absolute"));
        }
        if self.pid == 0 || self.pid > i32::MAX as u32 {
            return Err(Error::Invalid(
                "auxiliary PID must be a positive signed 32-bit identifier",
            ));
        }
        if self.start_seconds == 0 || self.start_micros >= 1_000_000 {
            return Err(Error::Invalid("invalid auxiliary process start time"));
        }
        Ok(())
    }
}

/// Host approval for a fixed auxiliary-process list. Discovery alone grants
/// nothing. Clones share one monotonic revocation state on the local thread;
/// revocation retires all widget connections that use this grant. Reapproval
/// requires an explicit new grant and fresh native connection.
///
/// ```compile_fail
/// use leselang_macos_ax_host::scope::AuxiliaryGrant;
/// fn transfer(grant: AuxiliaryGrant) { std::thread::spawn(move || grant.revoke()); }
/// ```
#[derive(Clone, Debug)]
pub struct AuxiliaryGrant {
    processes: Rc<[AuxiliaryProcessSpec]>,
    revoked: Rc<Cell<bool>>,
}
impl AuxiliaryGrant {
    pub fn new(processes: Vec<AuxiliaryProcessSpec>) -> Result<Self, Error> {
        if processes.is_empty() {
            return Err(Error::Invalid(
                "an auxiliary grant requires an explicit process",
            ));
        }
        if processes.len() > MAX_AUXILIARY_PROCESSES {
            return Err(Error::Bounds);
        }
        for (index, process) in processes.iter().enumerate() {
            process.validate()?;
            if processes[..index]
                .iter()
                .any(|other| other.pid == process.pid)
            {
                return Err(Error::Invalid("duplicate auxiliary process PID"));
            }
        }
        Ok(Self {
            processes: processes.into(),
            revoked: Rc::new(Cell::new(false)),
        })
    }
    pub fn processes(&self) -> &[AuxiliaryProcessSpec] {
        &self.processes
    }
    /// Returns true only for the first revocation shared by all clones.
    pub fn revoke(&self) -> bool {
        !self.revoked.replace(true)
    }
    pub fn is_revoked(&self) -> bool {
        self.revoked.get()
    }
    /// Check host approval only; native process/window identity remains the
    /// backend's responsibility at every observation and operation boundary.
    pub fn check_active(&self) -> Result<(), Error> {
        if self.is_revoked() {
            Err(Error::PermissionDenied)
        } else {
            Ok(())
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn callback_context(&self) -> *const c_void {
        Rc::as_ptr(&self.revoked).cast()
    }
}

/// The native bridge calls this synchronously on the owning thread, while a
/// WidgetsAccess retains the Rc. It never allocates, unwinds or invokes a host.
#[cfg(target_os = "macos")]
pub(crate) unsafe extern "C" fn auxiliary_grant_active(context: *const c_void) -> i32 {
    if context.is_null() {
        return 0;
    }
    // SAFETY: constructor passes Rc<Cell<bool>>'s stable address and its owning
    // WidgetsAccess destroys the native context before releasing that Rc.
    let revoked = unsafe { &*context.cast::<Cell<bool>>() };
    i32::from(!revoked.get())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationSpec {
    /// Absolute `.app` path. The native connector resolves and pins its executable.
    pub bundle_path: PathBuf,
    /// Refine an exact executable match when several instances are running.
    pub pid: Option<u32>,
}
impl ApplicationSpec {
    pub fn validate(&self) -> Result<(), Error> {
        validate_path(&self.bundle_path)?;
        if !self.bundle_path.is_absolute()
            || self
                .bundle_path
                .extension()
                .is_none_or(|extension| extension != "app")
        {
            return Err(Error::Invalid(
                "application requires an absolute .app bundle path",
            ));
        }
        if self
            .pid
            .is_some_and(|pid| pid == 0 || pid > i32::MAX as u32)
        {
            return Err(Error::Invalid(
                "PID must be a positive signed 32-bit process identifier",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowInfo {
    /// Connection-local discovery metadata; not a serialized native capability.
    pub key: String,
    pub title: String,
    pub identifier: String,
    /// Canonical local file URL or the observed non-file document URL.
    pub document: Option<String>,
}
impl WindowInfo {
    pub fn validate(&self) -> Result<(), Error> {
        validate_text(&self.key, false)?;
        validate_text(&self.title, true)?;
        validate_text(&self.identifier, true)?;
        if let Some(document) = &self.document {
            validate_text(document, false)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowSelector {
    /// Resolve a key from the same live ApplicationAccess connection.
    Key(String),
    /// Every supplied field must match exactly; none means no condition.
    /// At least one nonempty field is required. Multiple matches are an error.
    Exact {
        title: Option<String>,
        identifier: Option<String>,
        document: Option<PathBuf>,
    },
}
impl WindowSelector {
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Key(key) => validate_text(key, false),
            Self::Exact {
                title,
                identifier,
                document,
            } => {
                if title.is_none() && identifier.is_none() && document.is_none() {
                    return Err(Error::Invalid(
                        "window selection requires at least one exact field",
                    ));
                }
                for value in [title, identifier].into_iter().flatten() {
                    validate_text(value, false)?;
                }
                if let Some(path) = document {
                    validate_path(path)?;
                }
                Ok(())
            }
        }
    }
}

/// Validate the complete bounded inventory before publishing any of its rows.
pub fn validate_windows(windows: &[WindowInfo]) -> Result<(), Error> {
    if windows.len() > MAX_WINDOWS {
        return Err(Error::Bounds);
    }
    let mut total = 0_usize;
    for (index, window) in windows.iter().enumerate() {
        window.validate()?;
        if windows[..index].iter().any(|other| other.key == window.key) {
            return Err(Error::Invalid("duplicate window discovery key"));
        }
        for value in [&window.key, &window.title, &window.identifier]
            .into_iter()
            .chain(window.document.iter())
        {
            total = total.checked_add(value.len()).ok_or(Error::Bounds)?;
        }
    }
    if total > MAX_WINDOW_BYTES {
        return Err(Error::Bounds);
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), Error> {
    validate_text(
        path.to_str()
            .ok_or(Error::Invalid("scope path is not UTF-8"))?,
        false,
    )
}
fn validate_text(value: &str, empty_allowed: bool) -> Result<(), Error> {
    if value.len() > MAX_SCALAR_STRING_BYTES {
        return Err(Error::Bounds);
    }
    if (!empty_allowed && value.is_empty()) || value.contains('\0') {
        return Err(Error::Invalid(
            "scope fields must be nonempty and contain no NUL",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn native_approval_callback_uses_shared_state_retained_by_a_clone() {
        let grant = AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
            pid: 42,
            executable_path: "/System/Library/Service".into(),
            start_seconds: 1,
            start_micros: 0,
        }])
        .unwrap();
        let retained = grant.clone();
        let context = grant.callback_context();
        drop(grant);
        // SAFETY: retained holds the same Rc on this thread for both calls.
        assert_eq!(unsafe { auxiliary_grant_active(context) }, 1);
        retained.revoke();
        assert_eq!(unsafe { auxiliary_grant_active(context) }, 0);
        assert_eq!(unsafe { auxiliary_grant_active(std::ptr::null()) }, 0);
    }
    #[test]
    fn application_admission_bounds_explicit_authority() {
        let valid = ApplicationSpec {
            bundle_path: "/Applications/Test.app".into(),
            pid: Some(42),
        };
        valid.validate().unwrap();
        for path in ["Test.app", "/Applications/Test", "/Applications/Bad\0.app"] {
            assert!(
                ApplicationSpec {
                    bundle_path: path.into(),
                    ..valid.clone()
                }
                .validate()
                .is_err()
            );
        }
        for pid in [0, i32::MAX as u32 + 1, u32::MAX] {
            assert!(
                ApplicationSpec {
                    pid: Some(pid),
                    ..valid.clone()
                }
                .validate()
                .is_err()
            );
        }
    }
    #[test]
    fn selectors_reject_empty_unbounded_and_nul_fields() {
        for title in [
            None,
            Some("".into()),
            Some("bad\0title".into()),
            Some("x".repeat(4097)),
        ] {
            assert!(
                WindowSelector::Exact {
                    title,
                    identifier: None,
                    document: None
                }
                .validate()
                .is_err()
            );
        }
        for key in ["".into(), "bad\0key".into(), "x".repeat(4097)] {
            assert!(WindowSelector::Key(key).validate().is_err());
        }
        WindowSelector::Exact {
            title: Some("独立窗口 ✓".into()),
            identifier: Some("editor".into()),
            document: None,
        }
        .validate()
        .unwrap();
    }
    #[test]
    fn inventory_is_bounded_and_keys_are_unique() {
        let window = WindowInfo {
            key: "connection/1".into(),
            title: String::new(),
            identifier: String::new(),
            document: None,
        };
        validate_windows(&[]).unwrap();
        validate_windows(std::slice::from_ref(&window)).unwrap();
        assert!(validate_windows(&[window.clone(), window.clone()]).is_err());
        let many: Vec<_> = (0..65)
            .map(|n| WindowInfo {
                key: format!("connection/{n}"),
                ..window.clone()
            })
            .collect();
        assert!(validate_windows(&many).is_err());
        let bytes: Vec<_> = (0..17)
            .map(|n| WindowInfo {
                key: format!("connection/{n}"),
                title: "x".repeat(4096),
                ..window.clone()
            })
            .collect();
        assert!(validate_windows(&bytes).is_err());
    }
}
