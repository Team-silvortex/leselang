//! General controls inside one explicitly authorized application/window scope.
use crate::{
    Error, driver,
    host::{Field, Profile, ProfileSnapshot},
};
use leselang_vm::native::core::{
    MAX_SCALAR_STRING_BYTES, OptionalStringValue, ScalarType, ScalarValue, StringListValue,
};
use std::rc::Rc;

pub const MAX_CONTROLS: usize = 64;
pub const MAX_SCOPE_BYTES: usize = 64 * 1024;
pub const CALCULATOR_PROGRAM: &str = include_str!("../examples/controls_calculator.lese");
pub const TEXTEDIT_PROGRAM: &str = include_str!("../examples/controls_textedit.lese");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlInfo {
    pub identity: u64,
    /// Connection-local discovery metadata. Never a serialized capability.
    pub key: String,
    pub role: String,
    pub identifier: String,
    pub label: String,
    pub value: Option<String>,
    pub enabled: bool,
    pub editable: bool,
    pub pressable: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeSnapshot {
    pub identity: u64,
    /// Checked transport epoch for its last complete observed scope state.
    pub revision: u64,
    pub controls: Vec<ControlInfo>,
}
impl ScopeSnapshot {
    pub fn validate(&self) -> Result<(), Error> {
        if self.identity == 0 || self.revision == 0 || self.controls.len() > MAX_CONTROLS {
            return Err(Error::Bounds);
        }
        let mut total = 0_usize;
        let mut keys = 0_usize;
        for (index, control) in self.controls.iter().enumerate() {
            if control.identity == 0
                || control.key.is_empty()
                || control.role.is_empty()
                || self.controls[..index]
                    .iter()
                    .any(|other| other.identity == control.identity || other.key == control.key)
            {
                return Err(Error::Invalid(
                    "invalid or duplicate discovered control identity",
                ));
            }
            for text in [
                &control.key,
                &control.role,
                &control.identifier,
                &control.label,
            ]
            .into_iter()
            .chain(control.value.iter())
            {
                if text.len() > MAX_SCALAR_STRING_BYTES {
                    return Err(Error::Bounds);
                }
                total = total.checked_add(text.len()).ok_or(Error::Bounds)?;
            }
            keys = keys.checked_add(control.key.len()).ok_or(Error::Bounds)?;
            if !matches!(
                control.role.as_str(),
                "AXButton" | "AXTextArea" | "AXTextField" | "AXStaticText"
            ) {
                return Err(Error::Invalid("unsupported discovered control role"));
            }
            if control.editable && !matches!(control.role.as_str(), "AXTextArea" | "AXTextField") {
                return Err(Error::Invalid(
                    "editable capability belongs to an unsupported role",
                ));
            }
            if control.pressable && control.role != "AXButton" {
                return Err(Error::Invalid(
                    "press capability belongs to an unsupported role",
                ));
            }
        }
        if total > MAX_SCOPE_BYTES || keys > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        Ok(())
    }
    /// Canonical state comparison must not interpret view-tree reordering as
    /// replacement or use an array position as object identity.
    pub fn normalize(mut self) -> Result<Self, Error> {
        self.validate()?;
        self.controls.sort_by_key(|control| control.identity);
        Ok(self)
    }
    pub fn control(&self, identity: u64) -> Result<&ControlInfo, Error> {
        self.controls
            .iter()
            .find(|control| control.identity == identity)
            .ok_or(Error::Unavailable(
                "the discovered control is no longer present",
            ))
    }
}
pub trait ControlsBackend {
    fn snapshot(&self) -> Result<ScopeSnapshot, Error>;
    fn check_permission(&self) -> Result<(), Error>;
    /// Re-read the complete scope and compare it before entering AXPress.
    /// An error after entering native mutation must be `Error::Uncertain`.
    fn press(&self, expected: &ScopeSnapshot, target: u64) -> Result<(), Error>;
    fn set_text(&self, expected: &ScopeSnapshot, target: u64, text: &str) -> Result<(), Error>;
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlSnapshot {
    pub scope: Rc<ScopeSnapshot>,
    pub target: u64,
}
impl ControlSnapshot {
    pub fn control(&self) -> Result<&ControlInfo, Error> {
        self.scope.control(self.target)
    }
}
struct ControlsProfile<'b>(&'b dyn ControlsBackend);
/// Additional scalar payload available only to the opt-in controls catalog.
/// Legacy `Reply::Submitted` remains a boolean with the same three variants.
pub enum ControlsScalarReply {
    Submitted(bool),
    Keys(StringListValue),
}
impl crate::host::ScalarReplyPayload for ControlsScalarReply {
    fn submitted() -> Self {
        Self::Submitted(true)
    }
    fn keys_value(keys: StringListValue) -> Result<Self, Error> {
        Ok(Self::Keys(keys))
    }
    fn as_keys(&self) -> Option<&StringListValue> {
        if let Self::Keys(keys) = self {
            Some(keys)
        } else {
            None
        }
    }
    fn is_submitted(&self) -> bool {
        matches!(self, Self::Submitted(true))
    }
    fn into_scalar(self) -> ScalarValue {
        match self {
            Self::Submitted(value) => ScalarValue::Boolean(value),
            Self::Keys(keys) => ScalarValue::StringList(keys),
        }
    }
}
impl crate::host::ReplyPolicy for ControlSnapshot {
    type ScalarReply = ControlsScalarReply;
}
impl Profile for ControlsProfile<'_> {
    type Snapshot = ControlSnapshot;
    fn check_permission(&self) -> Result<(), Error> {
        self.0.check_permission()
    }
    fn snapshot(&self) -> Result<Self::Snapshot, Error> {
        self.snapshots()?
            .into_iter()
            .next()
            .ok_or(Error::Unavailable("the scope has no admitted controls"))
    }
    fn snapshots(&self) -> Result<Vec<Self::Snapshot>, Error> {
        let scope = Rc::new(self.0.snapshot()?.normalize()?);
        Ok(scope
            .controls
            .iter()
            .map(|control| ControlSnapshot {
                scope: scope.clone(),
                target: control.identity,
            })
            .collect())
    }
    fn apply(&self, _expected: &Self::Snapshot, _argument: &str) -> Result<(), Error> {
        Err(Error::Invalid("controls require an explicit operation"))
    }
    fn apply_operation(
        &self,
        expected: &Self::Snapshot,
        operation: &str,
        argument: &str,
    ) -> Result<(), Error> {
        match operation {
            "gui.press" => self.0.press(&expected.scope, expected.target),
            "gui.set_text" => self.0.set_text(&expected.scope, expected.target, argument),
            _ => Err(Error::Invalid("unknown control operation")),
        }
    }
}
impl ProfileSnapshot for ControlSnapshot {
    const SELECTOR: &'static str = "";
    const OPERATION: &'static str = "gui.set_text";
    const ARGUMENT: &'static str = "text";
    const CONTROLS: bool = true;
    fn validate(&self) -> Result<(), Error> {
        self.scope.validate()?;
        self.control().map(|_| ())
    }
    fn key(&self) -> &str {
        self.scope
            .controls
            .iter()
            .find(|c| c.identity == self.target)
            .map_or("", |c| c.key.as_str())
    }
    fn selector_matches(&self, role: &str, identifier: &str, label: &str) -> bool {
        self.control().is_ok_and(|control| {
            (role.is_empty() || control.role == role)
                && (identifier.is_empty() || control.identifier == identifier)
                && (label.is_empty() || control.label == label)
        })
    }
    fn same_instance(&self, other: &Self) -> bool {
        self.scope.identity == other.scope.identity
            && self.target == other.target
            && self.key() == other.key()
            && self.control().ok().map(|c| &c.role) == other.control().ok().map(|c| &c.role)
    }
    fn copy_cost(&self) -> u64 {
        // The complete dependency state is shared, not copied into every view.
        // Meter the selected bounded projection that this observation exposes.
        self.control().map_or(1, |c| {
            (c.key.len()
                + c.role.len()
                + c.identifier.len()
                + c.label.len()
                + c.value.as_ref().map_or(1, |s| s.len() + 1)
                + 4) as u64
        })
    }
    fn describe(&self) -> String {
        match self.control() {
            Ok(control) => format!(
                "key={:?}, role={:?}, identifier={:?}, label={:?}, value={:?}, enabled={}, editable={}, pressable={}",
                control.key,
                control.role,
                control.identifier,
                control.label,
                control.value,
                control.enabled,
                control.editable,
                control.pressable
            ),
            Err(_) => "unavailable control".into(),
        }
    }
    fn field_type(field: Field) -> Option<ScalarType> {
        match field {
            Field::Key | Field::Role | Field::Identifier | Field::Label => Some(ScalarType::String),
            Field::Value => Some(ScalarType::OptionalString),
            Field::Enabled | Field::Editable | Field::Pressable => Some(ScalarType::Boolean),
            Field::Actions => Some(ScalarType::StringList),
            Field::Revision => Some(ScalarType::Integer),
            _ => None,
        }
    }
    fn field(&self, field: Field) -> Result<ScalarValue, Error> {
        let control = self.control()?;
        Ok(match field {
            Field::Key => ScalarValue::String(control.key.clone()),
            Field::Role => ScalarValue::String(control.role.clone()),
            Field::Identifier => ScalarValue::String(control.identifier.clone()),
            Field::Label => ScalarValue::String(control.label.clone()),
            Field::Value => ScalarValue::OptionalString(OptionalStringValue(control.value.clone())),
            Field::Enabled => ScalarValue::Boolean(control.enabled),
            Field::Editable => ScalarValue::Boolean(control.editable),
            Field::Pressable => ScalarValue::Boolean(control.pressable),
            Field::Actions => ScalarValue::StringList(StringListValue(
                [(control.pressable, "press"), (control.editable, "set_text")]
                    .into_iter()
                    .filter(|(yes, _)| *yes)
                    .map(|(_, name)| name.into())
                    .collect(),
            )),
            _ => {
                return Err(Error::Invalid(
                    "field is unavailable in the controls profile",
                ));
            }
        })
    }
    fn admit_action(&self, _argument: &str) -> Result<(), Error> {
        Err(Error::Invalid("control action must name its operation"))
    }
    fn admit_operation(&self, operation: &str, argument: &str) -> Result<(), Error> {
        let control = self.control()?;
        if !control.enabled {
            return Err(Error::Invalid("control is disabled"));
        }
        match operation {
            "gui.press" if control.pressable && control.role == "AXButton" => Ok(()),
            "gui.set_text"
                if control.editable
                    && matches!(control.role.as_str(), "AXTextArea" | "AXTextField") =>
            {
                if argument.len() > MAX_SCALAR_STRING_BYTES {
                    Err(Error::Bounds)
                } else {
                    Ok(())
                }
            }
            _ => Err(Error::Invalid("operation is not supported by this control")),
        }
    }
}
pub fn discover(backend: &dyn ControlsBackend) -> Result<ScopeSnapshot, Error> {
    backend.check_permission()?;
    backend.snapshot()?.normalize()
}
/// Enter a controls session without running it. Consumers can drive the same
/// explicit poll, authorize, dispatch, pump and accept boundaries as the demos.
pub fn with_host<T>(
    backend: &dyn ControlsBackend,
    use_host: impl FnOnce(
        &crate::host::Host<'_, '_, ControlSnapshot>,
        leselang_vm::native::core::LocalExecutionScope,
    ) -> T,
) -> Result<T, Error> {
    crate::host::with_profile(&ControlsProfile(backend), use_host)
}
pub fn run(
    backend: &dyn ControlsBackend,
    source: &str,
) -> Result<driver::ProfileRunSummary<ControlSnapshot>, String> {
    driver::run_profile(&ControlsProfile(backend), source, 10000)
}

/// Supply scalar task inputs without rewriting source or importing native handles.
/// Objects are discovered inside the selected scope through the original driver.
pub fn run_task(
    backend: &dyn ControlsBackend,
    source: &str,
    entry: &str,
    arguments: Vec<(String, ScalarValue)>,
) -> Result<driver::ProfileRunSummary<ControlSnapshot>, String> {
    driver::run_profile_task(&ControlsProfile(backend), source, entry, arguments, 10000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        driver::Vm,
        host::{Host, ProfileReceipt as Receipt, ProfileReply as Reply, STACK, Work},
    };
    use leselang_vm::native::{
        HostWorkOutcome, LocalVm,
        core::*,
        hir::effect_session::{EffectSessionEnd, EffectSessionPoll, EffectSessionStatus},
    };
    use std::cell::{Cell, RefCell};

    fn control(
        identity: u64,
        role: &str,
        identifier: &str,
        label: &str,
        value: Option<&str>,
    ) -> ControlInfo {
        ControlInfo {
            identity,
            key: format!("connection-a/{identity}"),
            role: role.into(),
            identifier: identifier.into(),
            label: label.into(),
            value: value.map(str::to_owned),
            enabled: true,
            editable: role == "AXTextArea",
            pressable: role == "AXButton",
        }
    }
    struct Fake {
        state: RefCell<ScopeSnapshot>,
        permitted: Cell<bool>,
        available: Cell<bool>,
        actions: RefCell<Vec<u64>>,
        behavior: Cell<u8>,
        attempts: Cell<usize>,
    }
    impl Fake {
        fn calculator() -> Self {
            Self {
                state: RefCell::new(ScopeSnapshot {
                    identity: 10,
                    revision: 1,
                    controls: vec![
                        control(1, "AXStaticText", "", "Edit field", Some("0")),
                        control(2, "AXStaticText", "", "Last Expression", None),
                        control(3, "AXButton", "Eight", "8", None),
                        control(4, "AXButton", "Add", "Add", None),
                        control(5, "AXButton", "Seven", "7", None),
                        control(6, "AXButton", "Equals", "Equals", None),
                    ],
                }),
                permitted: Cell::new(true),
                available: Cell::new(true),
                actions: RefCell::new(Vec::new()),
                behavior: Cell::new(0),
                attempts: Cell::new(0),
            }
        }
        fn editor() -> Self {
            let fake = Self::calculator();
            fake.state.borrow_mut().controls.push(control(
                7,
                "AXTextArea",
                "editor",
                "Document",
                Some(crate::host::INITIAL_TEXT),
            ));
            fake
        }
        fn value(&self, identity: u64) -> Option<String> {
            self.state.borrow().control(identity).unwrap().value.clone()
        }
        fn change(&self, edit: impl FnOnce(&mut ScopeSnapshot)) {
            let mut state = self.state.borrow_mut();
            edit(&mut state);
            state.revision = state.revision.checked_add(1).unwrap_or(0);
        }
        fn preflight(
            &self,
            expected: &ScopeSnapshot,
            target: u64,
            operation: &str,
        ) -> Result<(), Error> {
            self.attempts.set(self.attempts.get() + 1);
            if self.behavior.get() == 1 {
                self.change(|state| state.controls[0].value = Some("external race".into()));
            }
            let actual = self.snapshot()?.normalize()?;
            if expected != &actual {
                return Err(Error::Stale);
            }
            ControlSnapshot {
                scope: Rc::new(actual),
                target,
            }
            .admit_operation(operation, "")
        }
    }
    impl ControlsBackend for Fake {
        fn snapshot(&self) -> Result<ScopeSnapshot, Error> {
            self.check_permission()?;
            if !self.available.get() {
                return Err(Error::Unavailable("scope closed"));
            }
            Ok(self.state.borrow().clone())
        }
        fn check_permission(&self) -> Result<(), Error> {
            if self.permitted.get() {
                Ok(())
            } else {
                Err(Error::PermissionDenied)
            }
        }
        fn press(&self, expected: &ScopeSnapshot, target: u64) -> Result<(), Error> {
            self.preflight(expected, target, "gui.press")?;
            if self.behavior.get() == 3 {
                return Ok(());
            }
            self.actions.borrow_mut().push(target);
            self.change(|state| {
                let display = state
                    .controls
                    .iter()
                    .position(|control| control.identity == 1)
                    .unwrap();
                let previous = state.controls[display]
                    .value
                    .as_deref()
                    .unwrap_or("0")
                    .to_owned();
                state.controls[display].value = Some(match target {
                    3 => {
                        if previous == "0" {
                            "8".into()
                        } else {
                            format!("{previous}8")
                        }
                    }
                    4 => format!("{previous}+"),
                    5 => format!("{previous}7"),
                    6 => {
                        let value = if self.behavior.get() == 4 {
                            "0 + 15".into()
                        } else {
                            previous.replace('+', " + ")
                        };
                        if let Some(history) = state
                            .controls
                            .iter_mut()
                            .find(|control| control.identity == 2)
                        {
                            history.value = Some(value);
                        } else {
                            state.controls.push(control(
                                2,
                                "AXStaticText",
                                "",
                                "Last Expression",
                                Some(&value),
                            ));
                        }
                        "15".into()
                    }
                    _ => unreachable!(),
                });
            });
            if self.behavior.get() == 2 {
                Err(Error::Uncertain(-25204))
            } else {
                Ok(())
            }
        }
        fn set_text(&self, expected: &ScopeSnapshot, target: u64, text: &str) -> Result<(), Error> {
            self.preflight(expected, target, "gui.set_text")?;
            if self.behavior.get() == 3 {
                return Ok(());
            }
            self.actions.borrow_mut().push(target);
            self.change(|state| {
                state
                    .controls
                    .iter_mut()
                    .find(|c| c.identity == target)
                    .unwrap()
                    .value = Some(text.into())
            });
            if self.behavior.get() == 2 {
                Err(Error::Uncertain(-25204))
            } else {
                Ok(())
            }
        }
    }
    const PRESS: &str = r#"fn main() = bind(target: gui.select(role: "AXButton", identifier: "Eight", label: ""), body: bind(before: gui.observe(target: target), body: gui.press(target: target, expected_revision: field(value: before, name: "revision"))))"#;
    fn session(
        fake: &Fake,
        source: &str,
        body: impl for<'ir, 's> FnOnce(
            &Host<'_, 's, ControlSnapshot>,
            &mut Vm<'ir, 's, ControlSnapshot>,
        ),
    ) {
        with_host(fake, |host, scope| {
            let node = host.compile(source).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10000), STACK);
            body(host, &mut vm);
        })
        .unwrap();
    }
    fn advance<'ir, 's>(
        host: &Host<'_, 's, ControlSnapshot>,
        vm: &mut Vm<'ir, 's, ControlSnapshot>,
        operation: &str,
    ) {
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), operation);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        driver::accept(vm, host, receipt).unwrap();
    }
    fn ready<'ir, 's>(
        host: &Host<'_, 's, ControlSnapshot>,
        vm: &mut Vm<'ir, 's, ControlSnapshot>,
    ) -> Work<'s, ControlSnapshot> {
        advance(host, vm, "gui.select");
        advance(host, vm, "gui.observe");
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), "gui.press");
        work
    }
    fn failed(vm: &Vm<'_, '_, ControlSnapshot>) {
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
    }

    #[test]
    fn complete_examples_verify_observed_text_and_calculation() {
        let calculator = Fake::calculator();
        let result = run(&calculator, CALCULATOR_PROGRAM).unwrap();
        assert_eq!(result.effects, 17);
        assert_eq!(result.value, ScalarValue::String("15".into()));
        assert_eq!(*calculator.actions.borrow(), [3, 4, 5, 6]);
        assert_eq!(result.observations.len(), 7);
        assert!(result.fuel_remaining < 10000);
        let editor = Fake::editor();
        let result = run(&editor, TEXTEDIT_PROGRAM).unwrap();
        assert_eq!(result.effects, 4);
        assert_eq!(
            result.value,
            ScalarValue::String("LeseLang discovered controls ✓\n".into())
        );
        assert_eq!(*editor.actions.borrow(), [7]);
    }

    #[test]
    fn offline_description_matches_live_controls_catalog_and_fields() {
        let fake = Fake::editor();
        with_host(&fake, |host, _| {
            host.set_granted(false);
            fake.permitted.set(false);
            crate::host::catalog::assert_live_description(
                host,
                crate::discovery::Profile::Controls,
            );
        })
        .unwrap();
        assert!(fake.actions.borrow().is_empty());
    }

    const TEXT_TASK: &str = include_str!("../examples/controls_text_task.lese");
    fn text_inputs(expected: &str, text: &str) -> Vec<(String, ScalarValue)> {
        // Intentionally submitted in a different order from the source signature.
        [
            ("text", text),
            ("expected", expected),
            ("label", ""),
            ("identifier", ""),
            ("role", "AXTextArea"),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), ScalarValue::String(value.into())))
        .collect()
    }

    #[test]
    fn reusable_task_passes_native_objects_and_snapshots_through_helpers() {
        for (initial, replacement) in [
            ("first document", "first replacement ✓"),
            ("second note", "second replacement"),
        ] {
            let fake = Fake::editor();
            fake.change(|scope| {
                scope
                    .controls
                    .iter_mut()
                    .find(|control| control.identity == 7)
                    .unwrap()
                    .value = Some(initial.into())
            });
            let result =
                run_task(&fake, TEXT_TASK, "main", text_inputs(initial, replacement)).unwrap();
            assert_eq!(result.value, ScalarValue::Boolean(true));
            assert_eq!(result.effects, 4);
            assert_eq!(result.observations.len(), 2);
            assert_eq!(fake.value(7).as_deref(), Some(replacement));
            assert_eq!(*fake.actions.borrow(), [7]);
        }
    }

    #[test]
    fn task_input_failures_happen_before_any_effect_or_mutation() {
        let mut missing = text_inputs("initial", "changed");
        missing.pop();
        let mut duplicate = text_inputs("initial", "changed");
        duplicate.push(duplicate[0].clone());
        let mut unknown = text_inputs("initial", "changed");
        unknown[0].0 = "unknown".into();
        let mut wrong_type = text_inputs("initial", "changed");
        wrong_type[0].1 = ScalarValue::Integer(42);
        for arguments in [
            missing,
            duplicate,
            unknown,
            wrong_type,
            text_inputs("initial", &"x".repeat(4097)),
        ] {
            let fake = Fake::editor();
            assert!(run_task(&fake, TEXT_TASK, "main", arguments).is_err());
            assert!(fake.actions.borrow().is_empty());
            assert_eq!(fake.attempts.get(), 0);
        }
        let fake = Fake::editor();
        assert!(
            run(&fake, TEXT_TASK).is_err(),
            "legacy run must still require a parameterless main"
        );
        assert!(fake.actions.borrow().is_empty());
    }

    #[test]
    fn native_helper_types_are_exact_and_cannot_be_supplied_by_task_inputs() {
        let fake = Fake::editor();
        for source in [
            "fn use(target: object) = gui.observe(target: target)\nfn main() = use(target: \"forged\")",
            "fn use(target: object) = gui.observe(target: target)\nfn main() = bind(target: gui.select(role: \"AXTextArea\", identifier: \"\", label: \"\"), body: bind(before: gui.observe(target: target), body: use(target: before)))",
            "fn unused(target: unknown_type) = none\nfn main() = true",
        ] {
            assert!(run(&fake, source).is_err(), "{source}");
        }
        assert!(
            run_task(
                &fake,
                "fn main(target: object) = gui.observe(target: target)",
                "main",
                vec![("target".into(), ScalarValue::String("forged".into()))]
            )
            .is_err()
        );
        assert!(fake.actions.borrow().is_empty());
    }

    #[test]
    fn task_helpers_preserve_guards_postconditions_and_uncertain_partial_writes() {
        let fake = Fake::editor();
        let guarded = run_task(
            &fake,
            TEXT_TASK,
            "main",
            text_inputs("wrong initial", "changed"),
        )
        .unwrap();
        assert_eq!(guarded.value, ScalarValue::Boolean(false));
        assert_eq!(guarded.effects, 2);
        assert!(fake.actions.borrow().is_empty());
        fake.behavior.set(3);
        let ignored = run_task(
            &fake,
            TEXT_TASK,
            "main",
            text_inputs(crate::host::INITIAL_TEXT, "changed"),
        )
        .unwrap();
        assert_eq!(ignored.value, ScalarValue::Boolean(false));
        assert_eq!(ignored.effects, 4);
        assert_eq!(fake.value(7).as_deref(), Some(crate::host::INITIAL_TEXT));
        fake.behavior.set(2);
        assert!(
            run_task(
                &fake,
                TEXT_TASK,
                "main",
                text_inputs(crate::host::INITIAL_TEXT, "changed")
            )
            .is_err()
        );
        assert_eq!(*fake.actions.borrow(), [7]);
        assert_eq!(fake.value(7).as_deref(), Some("changed"));
    }

    #[test]
    fn absent_observation_is_not_an_empty_expected_string() {
        let fake = Fake::editor();
        fake.change(|scope| {
            scope
                .controls
                .iter_mut()
                .find(|control| control.identity == 7)
                .unwrap()
                .value = None
        });
        let result = run_task(&fake, TEXT_TASK, "main", text_inputs("", "changed")).unwrap();
        assert_eq!(result.value, ScalarValue::Boolean(false));
        assert_eq!(result.effects, 2);
        assert!(fake.actions.borrow().is_empty());
        assert_eq!(fake.value(7), None);
    }
    #[test]
    fn a_full_inventory_shares_dependencies_within_the_original_controls_fuel() {
        let fake = Fake::calculator();
        {
            let mut state = fake.state.borrow_mut();
            for identity in 100..158 {
                state.controls.push(control(
                    identity,
                    "AXStaticText",
                    "",
                    "other observed text",
                    Some("unchanged window dependency"),
                ));
            }
            for control in &mut state.controls {
                control.key = format!("01234567-89ab-cdef-0123-456789abcdef/{}", control.identity);
            }
        }
        let result = run(&fake, CALCULATOR_PROGRAM).unwrap();
        assert_eq!(result.value, ScalarValue::String("15".into()));
        assert_eq!(result.observations[0].scope.controls.len(), MAX_CONTROLS);
        assert!(result.fuel_remaining > 0 && result.fuel_remaining < 10000);
        assert_eq!(*fake.actions.borrow(), [3, 4, 5, 6]);
    }
    #[test]
    fn a_new_history_control_can_be_discovered_after_equals() {
        let fake = Fake::calculator();
        fake.state
            .borrow_mut()
            .controls
            .retain(|control| control.identity != 2);
        let result = run(&fake, CALCULATOR_PROGRAM).unwrap();
        assert_eq!(result.value, ScalarValue::String("15".into()));
        assert_eq!(result.observations[0].scope.controls.len(), 5);
        assert_eq!(result.observations.last().unwrap().scope.controls.len(), 6);
        assert_eq!(fake.value(2).as_deref(), Some("8 + 7"));
    }
    #[test]
    fn discovery_keys_feed_find_without_becoming_native_authority() {
        let fake = Fake::editor();
        let source = r#"fn main() = bind(keys: gui.list(role: "AXTextArea"), body: bind(target: gui.find(key: fold(selected: "", items: keys, item: "key", next: key, limit: 64)), body: bind(state: gui.observe(target: target), body: field(value: state, name: "role"))))"#;
        let result = run(&fake, source).unwrap();
        assert_eq!(result.value, ScalarValue::String("AXTextArea".into()));
        assert_eq!(result.effects, 3);
        assert!(fake.actions.borrow().is_empty());
        for source in [
            r#"fn main() = gui.observe(target: "connection-a/7")"#,
            r#"fn main() = gui.press(target: "connection-a/3", expected_revision: 0)"#,
            r#"fn main() = gui.find(name: "document")"#,
            r#"fn main() = gui.press(target: "calculator", expected_revision: 0, button: "one")"#,
        ] {
            let call = source.strip_prefix("fn main() = ").unwrap();
            let guarded = format!("fn main() = bind(result: {call}, body: \"unexpected success\")");
            assert!(run(&fake, &guarded).is_err());
        }
        fake.change(|state| {
            state.identity += 1;
            for control in &mut state.controls {
                control.key = control.key.replace("connection-a", "connection-b");
            }
        });
        assert!(run(&fake, r#"fn main() = bind(target: gui.find(key: "connection-a/7"), body: "unexpected success")"#).is_err());
    }
    #[test]
    fn selectors_are_exact_unique_and_constrained() {
        let fake = Fake::calculator();
        for (role, identifier, label) in [
            ("", "", ""),
            ("AXButton", "", ""),
            ("AXButton", "Ei", ""),
            ("AXButton", "Eight", "7"),
        ] {
            let source = format!(
                "fn main() = bind(target: gui.select(role: {role:?}, identifier: {identifier:?}, label: {label:?}), body: \"unexpected success\")"
            );
            assert!(run(&fake, &source).is_err(), "{source}");
        }
        let source = r#"fn main() = bind(target: gui.select(role: "", identifier: "Eight", label: ""), body: bind(state: gui.observe(target: target), body: field(value: state, name: "actions")))"#;
        assert_eq!(
            run(&fake, source).unwrap().value,
            ScalarValue::StringList(StringListValue(vec!["press".into()]))
        );
        assert!(fake.actions.borrow().is_empty());
    }
    #[test]
    fn independent_controls_have_distinct_native_owners() {
        let fake = Fake::calculator();
        let source = r#"fn main() = bind(first: gui.select(role: "AXButton", identifier: "Eight", label: ""), body: gui.select(role: "AXButton", identifier: "Seven", label: ""))"#;
        session(&fake, source, |host, vm| {
            let work = driver::next(vm, host).unwrap();
            let first = driver::dispatch(vm, host, work).unwrap();
            let Reply::Object(first_object) = &first.reply else {
                panic!("expected object");
            };
            let first_object = first_object.clone();
            driver::accept(vm, host, first).unwrap();
            let work = driver::next(vm, host).unwrap();
            let second = driver::dispatch(vm, host, work).unwrap();
            assert!(
                vm.try_accept(&second.identity, Reply::Object(first_object), host)
                    .is_err()
            );
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            driver::accept(vm, host, second).unwrap();
        });
    }
    #[test]
    fn observations_cannot_move_between_control_owners_or_payload_targets() {
        let fake = Fake::calculator();
        let source = r#"fn main() = bind(first: gui.select(role: "AXButton", identifier: "Eight", label: ""), body: bind(before: gui.observe(target: first), body: bind(second: gui.select(role: "AXButton", identifier: "Seven", label: ""), body: bind(after: gui.observe(target: second), body: "ok"))))"#;
        session(&fake, source, |host, vm| {
            advance(host, vm, "gui.select");
            let work = driver::next(vm, host).unwrap();
            let first = driver::dispatch(vm, host, work).unwrap();
            let Reply::Snapshot(snapshot) = &first.reply else {
                panic!("expected observation");
            };
            let snapshot = snapshot.clone();
            driver::accept(vm, host, first).unwrap();
            advance(host, vm, "gui.select");
            let work = driver::next(vm, host).unwrap();
            let second = driver::dispatch(vm, host, work).unwrap();
            let fuel = vm.fuel_remaining();
            assert!(
                vm.try_accept(&second.identity, Reply::Snapshot(snapshot), host)
                    .is_err()
            );
            assert_eq!(vm.fuel_remaining(), fuel);
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            driver::accept(vm, host, second).unwrap();
        });
        let source = r#"fn main() = bind(target: gui.select(role: "AXStaticText", identifier: "", label: "Edit field"), body: bind(state: gui.observe(target: target), body: "ok"))"#;
        session(&fake, source, |host, vm| {
            advance(host, vm, "gui.select");
            let work = driver::next(vm, host).unwrap();
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let Reply::Snapshot(snapshot) = &receipt.reply else {
                panic!("expected observation");
            };
            let spoof = ControlSnapshot {
                scope: snapshot.data().scope.clone(),
                target: 3,
            };
            let spoof = host
                .owner()
                .observe(snapshot.object(), &host.lease(), spoof)
                .unwrap();
            assert!(
                vm.try_accept(&receipt.identity, Reply::Snapshot(Rc::new(spoof)), host)
                    .is_err()
            );
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            driver::accept(vm, host, receipt).unwrap();
        });
    }
    #[test]
    fn other_control_changes_invalidate_dispatch_and_queued_mutations() {
        for queued in [false, true] {
            for cause in 0..13 {
                let fake = Fake::calculator();
                session(&fake, PRESS, |host, vm| {
                    let mut work = Some(ready(host, vm));
                    let receipt =
                        queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                    match cause {
                        0 => fake.change(|s| s.controls[0].value = Some("external display".into())),
                        1 => fake.change(|s| s.controls[1].value = Some("external history".into())),
                        2 => fake.change(|s| s.controls[2].enabled = false),
                        3 => fake.change(|s| s.controls[2].pressable = false),
                        4 => fake.change(|s| s.identity += 1),
                        5 => fake.change(|s| s.controls[2].identity += 20),
                        6 => fake.change(|s| {
                            s.controls.remove(1);
                        }),
                        7 => fake.permitted.set(false),
                        8 => host.set_granted(false),
                        9 => fake.available.set(false),
                        10 => fake.change(|s| {
                            s.controls.remove(2);
                        }),
                        11 => fake.change(|s| s.controls[5].label = "renamed".into()),
                        12 => fake.change(|_| ()), // unprojected native scalar changed
                        _ => unreachable!(),
                    }
                    let result = if let Some(receipt) = receipt {
                        driver::pump(vm, host, receipt)
                    } else {
                        vm.with_metered_host_outcome(work.take().unwrap(), |work, fuel| {
                            host.dispatch(work, fuel)
                        })
                        .unwrap()
                    };
                    assert!(result.is_err(), "queued={queued}, cause={cause}");
                    assert!(fake.actions.borrow().is_empty());
                    failed(vm);
                });
            }
        }
    }
    #[test]
    fn reordering_preserves_control_identity_and_retired_keys_never_revive() {
        let fake = Fake::calculator();
        session(&fake, PRESS, |host, vm| {
            let work = ready(host, vm);
            fake.state.borrow_mut().controls.reverse();
            let receipt = driver::dispatch(vm, host, work).unwrap();
            // Restore fixture order only; neither change changes scope epoch.
            fake.state.borrow_mut().controls.reverse();
            let receipt = driver::pump(vm, host, receipt).unwrap();
            driver::accept(vm, host, receipt).unwrap();
            assert_eq!(*fake.actions.borrow(), [3]);
        });
        let fake = Fake::calculator();
        session(&fake, PRESS, |host, vm| {
            let work = ready(host, vm);
            let removed = fake.state.borrow_mut().controls.remove(2);
            host.refresh().unwrap();
            fake.state.borrow_mut().controls.push(removed);
            assert!(host.refresh().is_err());
            assert!(
                vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
                    .unwrap()
                    .is_err()
            );
            assert!(fake.actions.borrow().is_empty());
        });
    }
    #[test]
    fn observed_scope_epoch_detects_restored_values_before_dispatch() {
        let fake = Fake::calculator();
        session(&fake, PRESS, |host, vm| {
            let work = ready(host, vm);
            fake.change(|s| s.controls[0].value = Some("8".into()));
            host.refresh().unwrap();
            fake.change(|s| s.controls[0].value = Some("0".into()));
            assert!(
                vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
                    .unwrap()
                    .is_err()
            );
            assert!(fake.actions.borrow().is_empty());
        });
    }
    #[test]
    fn retired_owner_slots_remain_bounded_without_reusing_keys() {
        let fake = Fake::calculator();
        with_host(&fake, |host, _scope| {
            for identity in 100..222 {
                fake.change(|state| {
                    state
                        .controls
                        .push(control(identity, "AXButton", "new", "", None))
                });
                host.refresh().unwrap();
                fake.change(|state| {
                    state.controls.pop();
                });
                host.refresh().unwrap();
            }
            fake.change(|state| {
                state
                    .controls
                    .push(control(222, "AXButton", "overflow", "", None))
            });
            assert!(matches!(host.refresh(), Err(Error::Bounds)));
            assert!(fake.actions.borrow().is_empty());
        })
        .unwrap();
    }
    #[test]
    fn bridge_rechecks_scope_at_the_last_boundary() {
        let fake = Fake::calculator();
        session(&fake, PRESS, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            fake.behavior.set(1);
            assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Stale)));
            assert_eq!(fake.attempts.get(), 1);
            assert!(fake.actions.borrow().is_empty());
            failed(vm);
        });
    }
    #[test]
    fn cancellation_and_late_replies_preserve_completed_actions_and_fuel() {
        for delivered in [false, true] {
            let fake = Fake::calculator();
            session(&fake, PRESS, |host, vm| {
                let work = ready(host, vm);
                let receipt = driver::dispatch(vm, host, work).unwrap();
                let receipt = if delivered {
                    driver::pump(vm, host, receipt).unwrap()
                } else {
                    receipt
                };
                assert!(vm.cancel());
                let fuel = vm.fuel_remaining();
                let rejected = vm
                    .try_accept(&receipt.identity, receipt.reply, host)
                    .unwrap_err();
                assert_eq!(fuel, vm.fuel_remaining());
                if !delivered {
                    let receipt = Receipt {
                        identity: receipt.identity,
                        reply: rejected.reply,
                    };
                    assert!(matches!(
                        host.pump(receipt, &mut Fuel::new(1000)),
                        HostWorkOutcome::Failed(Err(_))
                    ));
                    assert!(host.discard_pending());
                }
                assert_eq!(fake.actions.borrow().len(), usize::from(delivered));
            });
        }
    }
    #[test]
    fn original_fuel_is_required_before_poll_and_native_delivery() {
        let fake = Fake::calculator();
        with_host(&fake, |host, scope| {
            let node = host.compile(PRESS).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(0), STACK);
            assert!(vm.poll(host).is_err());
            assert_eq!(vm.fuel_remaining(), 0);
        })
        .unwrap();
        session(&fake, PRESS, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            vm.with_metered_host_outcome((), |_, fuel| {
                fuel.charge(fuel.remaining()).unwrap();
                HostWorkOutcome::Pending(())
            })
            .unwrap();
            assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Fuel)));
            assert_eq!(vm.fuel_remaining(), 0);
            failed(vm);
        });
        assert!(fake.actions.borrow().is_empty());
    }
    #[test]
    fn uncertain_mutation_is_retained_without_replay() {
        let fake = Fake::calculator();
        session(&fake, PRESS, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            fake.behavior.set(2);
            assert!(matches!(
                driver::pump(vm, host, receipt),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::HostUncertain)
            );
            assert_eq!(*fake.actions.borrow(), [3]);
            assert_eq!(fake.value(1).as_deref(), Some("8"));
            assert!(!host.has_pending());
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(EffectSessionEnd::HostUncertain)
            ));
            assert_eq!(fake.attempts.get(), 1);
        });
    }
    #[test]
    fn completed_actions_cannot_replay_and_revoked_replies_preserve_side_effects() {
        for replay in [false, true] {
            let fake = Fake::calculator();
            session(&fake, PRESS, |host, vm| {
                let work = ready(host, vm);
                let receipt = driver::dispatch(vm, host, work).unwrap();
                let receipt = driver::pump(vm, host, receipt).unwrap();
                if replay {
                    let replay = Receipt {
                        identity: receipt.identity.clone(),
                        reply: Reply::Submitted(ControlsScalarReply::Submitted(true)),
                    };
                    assert!(driver::pump(vm, host, replay).is_err());
                    failed(vm);
                } else {
                    fake.permitted.set(false);
                }
                let fuel = vm.fuel_remaining();
                assert!(
                    vm.try_accept(&receipt.identity, receipt.reply, host)
                        .is_err()
                );
                assert_eq!(fuel, vm.fuel_remaining());
                assert_eq!(fake.attempts.get(), 1);
                assert_eq!(*fake.actions.borrow(), [3]);
                assert_eq!(fake.value(1).as_deref(), Some("8"));
            });
        }
    }
    #[test]
    fn empty_inventory_and_large_discovery_are_bounded() {
        let fake = Fake::calculator();
        fake.state.borrow_mut().controls.clear();
        assert!(discover(&fake).unwrap().controls.is_empty());
        assert_eq!(
            run(&fake, r#"fn main() = gui.list(role: "")"#)
                .unwrap()
                .value,
            ScalarValue::StringList(StringListValue(Vec::new()))
        );
        for identity in 1..=64 {
            fake.state.borrow_mut().controls.push(control(
                identity,
                "AXButton",
                &format!("Button{identity}"),
                "",
                None,
            ));
        }
        assert_eq!(discover(&fake).unwrap().controls.len(), 64);
        let result = run(&fake, r#"fn main() = gui.list(role: "AXButton")"#).unwrap();
        let ScalarValue::StringList(keys) = result.value else {
            panic!("expected discovery list");
        };
        assert_eq!(keys.0.len(), 64);
        fake.state
            .borrow_mut()
            .controls
            .push(control(65, "AXButton", "extra", "", None));
        assert!(discover(&fake).is_err());
    }
    #[test]
    fn acknowledged_action_requires_observation_and_history_verification() {
        let fake = Fake::editor();
        fake.behavior.set(3);
        assert_eq!(
            run(&fake, TEXTEDIT_PROGRAM).unwrap().value,
            ScalarValue::String("verification failed".into())
        );
        assert!(fake.actions.borrow().is_empty());
        let fake = Fake::calculator();
        fake.behavior.set(4);
        assert_eq!(
            run(&fake, CALCULATOR_PROGRAM).unwrap().value,
            ScalarValue::String("verification failed".into())
        );
        assert_eq!(*fake.actions.borrow(), [3, 4, 5, 6]);
    }
    #[test]
    fn initial_mismatch_and_unsupported_actions_do_not_mutate() {
        let fake = Fake::calculator();
        fake.change(|s| s.controls[0].value = Some("9".into()));
        let result = run(&fake, CALCULATOR_PROGRAM).unwrap();
        assert_eq!(result.effects, 2);
        assert_eq!(
            result.value,
            ScalarValue::String("initial display mismatch".into())
        );
        for source in [
            r#"fn main() = bind(target: gui.select(role: "AXStaticText", identifier: "", label: "Edit field"), body: bind(before: gui.observe(target: target), body: gui.press(target: target, expected_revision: field(value: before,name: "revision"))))"#,
            r#"fn main() = bind(target: gui.select(role: "AXButton", identifier: "Eight", label: ""), body: bind(before: gui.observe(target: target), body: gui.set_text(target: target, expected_revision: field(value: before,name: "revision"), text: "text")))"#,
        ] {
            assert!(run(&fake, source).is_err());
        }
        assert!(fake.actions.borrow().is_empty());
    }
    #[test]
    fn a_later_failure_keeps_the_first_completed_control_action() {
        let fake = Fake::calculator();
        let source = PRESS.replace("fn main() =", "fn first() =");
        let source = format!(
            "{source}\nfn main() = bind(done: first(), body: bind(missing: gui.select(role: \"AXButton\", identifier: \"absent\", label: \"\"), body: \"unexpected success\"))"
        );
        assert!(run(&fake, &source).is_err());
        assert_eq!(*fake.actions.borrow(), [3]);
        assert_eq!(fake.value(1).as_deref(), Some("8"));
    }
    #[test]
    fn untrusted_inventory_metadata_is_bounded_before_discovery_or_execution() {
        for cause in 0..12 {
            let fake = Fake::calculator();
            fake.change(|s| match cause {
                0 => s.identity = 0,
                1 => {
                    s.revision = u64::MAX;
                }
                2 => s.controls[0].identity = 0,
                3 => s.controls[1].key = s.controls[0].key.clone(),
                4 => s.controls[1].identity = s.controls[0].identity,
                5 => s.controls[0].role = "AXWebArea".into(),
                6 => s.controls[0].editable = true,
                7 => s.controls[0].value = Some("x".repeat(MAX_SCALAR_STRING_BYTES + 1)),
                8 => {
                    for control in &mut s.controls {
                        control.key = "k".repeat(1000) + &control.identity.to_string();
                    }
                }
                9 => {
                    s.controls = (1..=64)
                        .map(|identity| {
                            control(identity, "AXStaticText", "", "", Some(&"v".repeat(1024)))
                        })
                        .collect();
                }
                10 => s.controls[0].pressable = true,
                11 => s.controls[0].key.clear(),
                _ => unreachable!(),
            });
            assert!(discover(&fake).is_err(), "cause={cause}");
            assert!(run(&fake, PRESS).is_err(), "cause={cause}");
            assert!(fake.actions.borrow().is_empty());
        }
    }
}
