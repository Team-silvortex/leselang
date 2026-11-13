//! External API and reference-source regressions; these tests never access a GUI.
use leselang_macos_ax_host::{
    Error,
    controls::{self, ControlInfo, ControlsBackend, ScopeSnapshot},
    scope::{ApplicationSpec, WindowInfo, WindowSelector, validate_windows},
};
use leselang_vm::native::core::ScalarValue;
use std::cell::{Cell, RefCell};

const PROGRAM: &str = include_str!("../examples/controls_stickies.lese");
const INITIAL: &str = "LeseLang scope initial 20261113\n";
const MIDDLE: &str = "LeseLang scope intermediate\n";
const FINAL: &str = "LeseLang generic app window ✓\n";

#[test]
fn external_callers_can_construct_explicit_application_and_window_selections() {
    ApplicationSpec {
        bundle_path: "/System/Applications/Stickies.app".into(),
        pid: Some(123),
    }
    .validate()
    .unwrap();
    let inventory = [WindowInfo {
        key: "connection/window-1".into(),
        title: "Dedicated test note".into(),
        identifier: String::new(),
        document: None,
    }];
    validate_windows(&inventory).unwrap();
    WindowSelector::Key(inventory[0].key.clone())
        .validate()
        .unwrap();
    WindowSelector::Exact {
        title: Some(inventory[0].title.clone()),
        identifier: None,
        document: None,
    }
    .validate()
    .unwrap();
    assert!(
        WindowSelector::Exact {
            title: None,
            identifier: None,
            document: None,
        }
        .validate()
        .is_err()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_application_entry_points_are_available_without_opening_a_connection() {
    use leselang_macos_ax_host::{
        controls_access::ControlsAccess, scope_access::ApplicationAccess,
    };
    // Function-pointer assignments verify public signatures without querying
    // permissions, constructing native state or launching an application.
    let _: fn(&ApplicationSpec) -> Result<ApplicationAccess, Error> = ApplicationAccess::connect;
    let _: fn(&ApplicationAccess) -> u32 = ApplicationAccess::pid;
    let _: fn(&ApplicationAccess) -> Result<Vec<WindowInfo>, Error> = ApplicationAccess::windows;
    let _: fn(&ApplicationAccess, &WindowSelector) -> Result<ControlsAccess, Error> =
        ApplicationAccess::controls;
}

struct NoteBackend {
    state: RefCell<ScopeSnapshot>,
    writes: RefCell<Vec<String>>,
    expected_epochs: RefCell<Vec<u64>>,
    corrupt_delivery: Option<usize>,
    race_before_second: Cell<bool>,
}
impl NoteBackend {
    fn new(text: &str) -> Self {
        let editor = ControlInfo {
            identity: 2,
            key: "fake-connection/editor".into(),
            role: "AXTextArea".into(),
            identifier: String::new(),
            label: String::new(),
            value: Some(text.into()),
            enabled: true,
            editable: true,
            pressable: false,
        };
        // A separate read-only dependency changes alongside each text edit.
        // This models a backend epoch; it is not a native window identity test.
        let dependency = ControlInfo {
            identity: 3,
            key: "fake-connection/dependency".into(),
            role: "AXStaticText".into(),
            identifier: "dependency".into(),
            value: Some("initial dependency".into()),
            editable: false,
            ..editor.clone()
        };
        Self {
            state: RefCell::new(ScopeSnapshot {
                identity: 1,
                revision: 1,
                controls: vec![editor, dependency],
            }),
            writes: RefCell::new(Vec::new()),
            expected_epochs: RefCell::new(Vec::new()),
            corrupt_delivery: None,
            race_before_second: Cell::new(false),
        }
    }
    fn current_text(&self) -> String {
        self.state.borrow().controls[0].value.clone().unwrap()
    }
}
impl ControlsBackend for NoteBackend {
    fn snapshot(&self) -> Result<ScopeSnapshot, Error> {
        Ok(self.state.borrow().clone())
    }
    fn check_permission(&self) -> Result<(), Error> {
        Ok(())
    }
    fn press(&self, _: &ScopeSnapshot, _: u64) -> Result<(), Error> {
        panic!("the note source must not press a button")
    }
    fn set_text(&self, expected: &ScopeSnapshot, target: u64, text: &str) -> Result<(), Error> {
        let mut actual = self.state.borrow_mut();
        if self.writes.borrow().len() == 1 && self.race_before_second.replace(false) {
            actual.controls[1].value = Some("changed between host preflight and delivery".into());
            actual.revision += 1;
        }
        // The public backend must do its own final full-scope comparison,
        // because the remote process can change after the session host checks.
        if &*actual != expected {
            return Err(Error::Stale);
        }
        assert_eq!(target, 2);
        assert!(actual.control(target)?.editable);
        let mut writes = self.writes.borrow_mut();
        let delivery = writes.len() + 1;
        writes.push(text.into());
        self.expected_epochs.borrow_mut().push(expected.revision);
        let observed = if self.corrupt_delivery == Some(delivery) {
            "a submitted edit produced another value\n"
        } else {
            text
        };
        actual.controls[0].value = Some(observed.into());
        actual.controls[1].value = Some(format!("dependency after edit {delivery}"));
        actual.revision += 1;
        Ok(())
    }
}

#[test]
fn stickies_reference_source_delivers_two_edits_and_observes_each_result() {
    let backend = NoteBackend::new(INITIAL);
    let result = controls::run(&backend, PROGRAM).unwrap();
    assert_eq!(result.effects, 6);
    assert_eq!(result.value, ScalarValue::String(FINAL.into()));
    assert_eq!(*backend.writes.borrow(), [MIDDLE, FINAL]);
    assert_eq!(*backend.expected_epochs.borrow(), [1, 2]);
    assert_eq!(backend.current_text(), FINAL);
    assert!(result.fuel_remaining > 0 && result.fuel_remaining < 10_000);
    let observed: Vec<_> = result
        .observations
        .iter()
        .map(|snapshot| {
            assert_eq!(snapshot.target, 2);
            (
                snapshot.scope.revision,
                snapshot.control().unwrap().value.as_deref().unwrap(),
            )
        })
        .collect();
    assert_eq!(observed, [(1, INITIAL), (2, MIDDLE), (3, FINAL)]);
}

#[test]
fn stickies_reference_source_refuses_unexpected_initial_content_without_editing() {
    let backend = NoteBackend::new("existing note content\n");
    let result = controls::run(&backend, PROGRAM).unwrap();
    assert_eq!(result.effects, 2);
    assert_eq!(
        result.value,
        ScalarValue::String("initial note mismatch".into())
    );
    assert!(backend.writes.borrow().is_empty());
    assert_eq!(backend.current_text(), "existing note content\n");
    assert_eq!(result.observations.len(), 1);
}

#[test]
fn stickies_reference_source_requires_observed_postconditions_after_acknowledgement() {
    for (delivery, effects, message) in [
        (1, 4, "intermediate verification failed"),
        (2, 6, "final verification failed"),
    ] {
        let mut backend = NoteBackend::new(INITIAL);
        backend.corrupt_delivery = Some(delivery);
        let result = controls::run(&backend, PROGRAM).unwrap();
        assert_eq!(result.effects, effects);
        assert_eq!(result.value, ScalarValue::String(message.into()));
        assert_eq!(backend.writes.borrow().len(), delivery);
        assert_eq!(
            backend.current_text(),
            "a submitted edit produced another value\n"
        );
    }
}

#[test]
fn final_backend_scope_recheck_refuses_second_edit_and_preserves_first_effect() {
    let backend = NoteBackend::new(INITIAL);
    backend.race_before_second.set(true);
    let error = controls::run(&backend, PROGRAM).unwrap_err();
    assert_eq!(error, Error::Stale.to_string());
    assert_eq!(*backend.writes.borrow(), [MIDDLE]);
    assert_eq!(backend.current_text(), MIDDLE);
    assert_eq!(backend.state.borrow().revision, 3);
}
