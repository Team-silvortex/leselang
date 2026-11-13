//! Opt-in common widgets in one pinned native window. Existing controls APIs
//! keep their original types and catalog; this profile adds typed operations.
use crate::{
    Error,
    controls::{ControlInfo, ControlsScalarReply, MAX_CONTROLS, MAX_SCOPE_BYTES},
    driver,
    host::{ActionArgument, Field, Profile, ProfileSnapshot, SourceWaitKind, SourceWaitState},
};
use leselang_vm::native::core::{
    MAX_SCALAR_STRING_BYTES, OptionalStringValue, ScalarType, ScalarValue, StringListValue,
};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckState {
    Unchecked,
    Checked,
    Mixed,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WidgetCapabilities {
    pub open_menu: bool,
    pub choose_menu_item: bool,
    pub cancel_menu: bool,
    pub set_checked: bool,
    pub select_item: bool,
    pub scroll: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetInfo {
    pub control: ControlInfo,
    pub checked: Option<CheckState>,
    pub selected: Option<bool>,
    /// Normalized scrollbar position, from 0 through 1000 inclusive.
    pub position: Option<u16>,
    pub orientation: Option<String>,
    pub capabilities: WidgetCapabilities,
    /// Nearest admitted ancestor, never a positional selector or capability.
    pub parent: Option<u64>,
}
impl WidgetInfo {
    pub fn actions(&self) -> Vec<String> {
        [
            (self.control.pressable, "press"),
            (self.control.editable, "set_text"),
            (self.capabilities.open_menu, "open_menu"),
            (self.capabilities.choose_menu_item, "choose_menu_item"),
            (self.capabilities.cancel_menu, "cancel_menu"),
            (self.capabilities.set_checked, "set_checked"),
            (self.capabilities.select_item, "select_item"),
            (self.capabilities.scroll, "scroll"),
        ]
        .into_iter()
        .filter(|(available, _)| *available)
        .map(|(_, name)| name.to_owned())
        .collect()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetScope {
    pub identity: u64,
    pub revision: u64,
    pub widgets: Vec<WidgetInfo>,
}
impl WidgetScope {
    pub fn widget(&self, identity: u64) -> Result<&WidgetInfo, Error> {
        self.widgets
            .iter()
            .find(|widget| widget.control.identity == identity)
            .ok_or(Error::Unavailable("the widget is no longer in this scope"))
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.identity == 0 || self.revision == 0 || self.widgets.len() > MAX_CONTROLS {
            return Err(Error::Bounds);
        }
        let mut bytes = 0_usize;
        let mut keys = 0_usize;
        for (index, widget) in self.widgets.iter().enumerate() {
            let control = &widget.control;
            if control.identity == 0
                || control.key.is_empty()
                || self.widgets[..index].iter().any(|other| {
                    other.control.identity == control.identity || other.control.key == control.key
                })
            {
                return Err(Error::Invalid("invalid or duplicate widget identity"));
            }
            if !matches!(
                control.role.as_str(),
                "AXButton"
                    | "AXTextArea"
                    | "AXTextField"
                    | "AXStaticText"
                    | "AXPopUpButton"
                    | "AXMenuButton"
                    | "AXMenu"
                    | "AXMenuItem"
                    | "AXCheckBox"
                    | "AXList"
                    | "AXTable"
                    | "AXOutline"
                    | "AXRow"
                    | "AXCell"
                    | "AXGroup"
                    | "AXScrollBar"
                    | "AXScrollArea"
            ) {
                return Err(Error::Invalid("unsupported widget role"));
            }
            for text in [
                &control.key,
                &control.role,
                &control.identifier,
                &control.label,
            ]
            .into_iter()
            .chain(control.value.iter())
            .chain(widget.orientation.iter())
            {
                if text.len() > MAX_SCALAR_STRING_BYTES {
                    return Err(Error::Bounds);
                }
                bytes = bytes.checked_add(text.len()).ok_or(Error::Bounds)?;
            }
            keys = keys.checked_add(control.key.len()).ok_or(Error::Bounds)?;
            if control.editable && !text_role(&control.role)
                || control.pressable && control.role != "AXButton"
            {
                return Err(Error::Invalid(
                    "widget has an incompatible legacy action capability",
                ));
            }
            let caps = &widget.capabilities;
            if (caps.open_menu || caps.choose_menu_item || caps.cancel_menu)
                && !menu_owner(&control.role)
                || caps.set_checked && control.role != "AXCheckBox"
                || caps.select_item && !collection(&control.role)
                || caps.scroll && control.role != "AXScrollBar"
            {
                return Err(Error::Invalid(
                    "widget has an incompatible operation capability",
                ));
            }
            if widget.checked.is_some() && control.role != "AXCheckBox"
                || widget.position.is_some_and(|position| position > 1000)
                || (widget.position.is_some() || widget.orientation.is_some())
                    && control.role != "AXScrollBar"
                || widget
                    .orientation
                    .as_ref()
                    .is_some_and(|value| !matches!(value.as_str(), "vertical" | "horizontal"))
            {
                return Err(Error::Invalid("invalid typed widget state"));
            }
            let mut parent = widget.parent;
            for depth in 0..=self.widgets.len() {
                let Some(identity) = parent else {
                    break;
                };
                if depth == self.widgets.len() || identity == control.identity {
                    return Err(Error::Invalid("cyclic widget ancestry"));
                }
                parent = self.widget(identity)?.parent;
            }
            if control.role == "AXGroup" && widget.selected.is_none() {
                return Err(Error::Invalid(
                    "generic groups must be observed collection items",
                ));
            }
            if widget.selected.is_some() {
                let Some(parent) = widget.parent else {
                    return Err(Error::Invalid("selected state has no collection owner"));
                };
                if !collection(&self.widget(parent)?.control.role) {
                    return Err(Error::Invalid(
                        "selected state belongs to an unsupported parent",
                    ));
                }
            }
        }
        if bytes > MAX_SCOPE_BYTES || keys > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        Ok(())
    }
    pub fn normalize(mut self) -> Result<Self, Error> {
        self.validate()?;
        self.widgets.sort_by_key(|widget| widget.control.identity);
        Ok(self)
    }
    fn descendant(&self, item: u64, owner: u64) -> Result<bool, Error> {
        let mut parent = self.widget(item)?.parent;
        for _ in 0..self.widgets.len() {
            let Some(identity) = parent else {
                return Ok(false);
            };
            if identity == owner {
                return Ok(true);
            }
            parent = self.widget(identity)?.parent;
        }
        Err(Error::Invalid("cyclic widget ancestry"))
    }
    pub fn admit(&self, target: u64, action: &WidgetAction) -> Result<(), Error> {
        self.validate()?;
        let widget = self.widget(target)?;
        let control = &widget.control;
        let caps = &widget.capabilities;
        if !control.enabled {
            return Err(Error::Invalid("widget is disabled"));
        }
        let admitted = match action {
            WidgetAction::Press => control.role == "AXButton" && control.pressable,
            WidgetAction::SetText(text) => {
                if text.len() > MAX_SCALAR_STRING_BYTES {
                    return Err(Error::Bounds);
                }
                text_role(&control.role) && control.editable
            }
            WidgetAction::OpenMenu => menu_owner(&control.role) && caps.open_menu,
            WidgetAction::CancelMenu => menu_owner(&control.role) && caps.cancel_menu,
            WidgetAction::ChooseMenuItem { item } => {
                let item_control = &self.widget(*item)?.control;
                menu_owner(&control.role)
                    && caps.choose_menu_item
                    && item_control.enabled
                    && item_control.role == "AXMenuItem"
                    && self.descendant(*item, target)?
            }
            WidgetAction::SetChecked(_) => {
                control.role == "AXCheckBox"
                    && caps.set_checked
                    && matches!(
                        widget.checked,
                        Some(CheckState::Unchecked | CheckState::Checked)
                    )
            }
            WidgetAction::SelectItem { item } => {
                let item = self.widget(*item)?;
                collection(&control.role)
                    && caps.select_item
                    && item.parent == Some(target)
                    && item.control.enabled
                    && item.selected.is_some()
            }
            WidgetAction::Scroll(position) => {
                *position <= 1000
                    && control.role == "AXScrollBar"
                    && caps.scroll
                    && widget.position.is_some()
            }
        };
        if admitted {
            Ok(())
        } else {
            Err(Error::Invalid(
                "operation is not supported by this widget or membership",
            ))
        }
    }
}
fn text_role(role: &str) -> bool {
    matches!(role, "AXTextArea" | "AXTextField")
}
fn menu_owner(role: &str) -> bool {
    matches!(role, "AXPopUpButton" | "AXMenuButton")
}
fn collection(role: &str) -> bool {
    matches!(role, "AXList" | "AXTable" | "AXOutline")
}

/// Backend identities have already been resolved from typed local references by
/// the host. The backend still rechecks the complete expected native scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WidgetAction {
    Press,
    SetText(String),
    OpenMenu,
    ChooseMenuItem { item: u64 },
    CancelMenu,
    SetChecked(bool),
    SelectItem { item: u64 },
    Scroll(u16),
}
pub trait WidgetsBackend {
    fn snapshot(&self) -> Result<WidgetScope, Error>;
    fn check_permission(&self) -> Result<(), Error>;
    /// A failure after entering native mutation must be Error::Uncertain.
    /// Successful acknowledgement still requires a separate observation.
    fn perform(
        &self,
        expected: &WidgetScope,
        target: u64,
        action: &WidgetAction,
    ) -> Result<(), Error>;
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetSnapshot {
    pub scope: Rc<WidgetScope>,
    pub target: u64,
}
impl WidgetSnapshot {
    pub fn widget(&self) -> Result<&WidgetInfo, Error> {
        self.scope.widget(self.target)
    }
}
struct WidgetsProfile<'b>(&'b dyn WidgetsBackend);
impl Profile for WidgetsProfile<'_> {
    type Snapshot = WidgetSnapshot;
    fn check_permission(&self) -> Result<(), Error> {
        self.0.check_permission()
    }
    fn snapshot(&self) -> Result<WidgetSnapshot, Error> {
        self.snapshots()?
            .into_iter()
            .next()
            .ok_or(Error::Unavailable("the scope has no widgets"))
    }
    fn source_wait_state<'a>(
        &self,
        snapshot: &'a WidgetSnapshot,
        kind: SourceWaitKind,
    ) -> Result<SourceWaitState<'a>, Error> {
        let widget = snapshot.widget()?;
        let mut selection_parent = None;
        let value = match kind {
            SourceWaitKind::Checked if widget.control.role == "AXCheckBox" => {
                match widget.checked {
                    Some(CheckState::Checked) => Some(ScalarValue::Boolean(true)),
                    Some(CheckState::Unchecked) => Some(ScalarValue::Boolean(false)),
                    Some(CheckState::Mixed) => None,
                    None => return Err(Error::Invalid("checkbox state is unavailable")),
                }
            }
            SourceWaitKind::Selected => {
                let selected = widget
                    .selected
                    .ok_or(Error::Invalid("selected state is unavailable"))?;
                let parent = snapshot.scope.widget(
                    widget
                        .parent
                        .ok_or(Error::Invalid("selected state has no collection owner"))?,
                )?;
                if !collection(&parent.control.role) {
                    return Err(Error::Invalid(
                        "selected state has an unsupported collection owner",
                    ));
                }
                selection_parent = Some((parent.control.identity, parent.control.key.as_str()));
                Some(ScalarValue::Boolean(selected))
            }
            SourceWaitKind::Position if widget.control.role == "AXScrollBar" => {
                let position = widget
                    .position
                    .ok_or(Error::Invalid("scroll position is unavailable"))?;
                if position > 1000 {
                    return Err(Error::Bounds);
                }
                Some(ScalarValue::Integer(u64::from(position)))
            }
            _ => return Err(Error::Invalid("typed wait is unsupported for this widget")),
        };
        Ok(SourceWaitState {
            value,
            selection_parent,
        })
    }
    fn snapshots(&self) -> Result<Vec<WidgetSnapshot>, Error> {
        let scope = Rc::new(self.0.snapshot()?.normalize()?);
        Ok(scope
            .widgets
            .iter()
            .map(|widget| WidgetSnapshot {
                scope: scope.clone(),
                target: widget.control.identity,
            })
            .collect())
    }
    fn apply(&self, _: &WidgetSnapshot, _: &str) -> Result<(), Error> {
        Err(Error::Invalid("widgets require a typed operation"))
    }
    fn apply_typed_operation(
        &self,
        expected: &WidgetSnapshot,
        operation: &str,
        argument: &ActionArgument,
    ) -> Result<(), Error> {
        self.0.perform(
            &expected.scope,
            expected.target,
            &action(operation, argument)?,
        )
    }
}
fn action(operation: &str, argument: &ActionArgument) -> Result<WidgetAction, Error> {
    match (operation, argument) {
        ("gui.press", ActionArgument::None) => Ok(WidgetAction::Press),
        ("gui.set_text", ActionArgument::Text(text)) => Ok(WidgetAction::SetText(text.clone())),
        ("gui.open_menu", ActionArgument::None) => Ok(WidgetAction::OpenMenu),
        ("gui.choose_menu_item", ActionArgument::Item { identity, .. }) => {
            Ok(WidgetAction::ChooseMenuItem { item: *identity })
        }
        ("gui.cancel_menu", ActionArgument::None) => Ok(WidgetAction::CancelMenu),
        ("gui.set_checked", ActionArgument::Boolean(checked)) => {
            Ok(WidgetAction::SetChecked(*checked))
        }
        ("gui.select_item", ActionArgument::Item { identity, .. }) => {
            Ok(WidgetAction::SelectItem { item: *identity })
        }
        ("gui.scroll", ActionArgument::Integer(position)) if (0..=1000).contains(position) => {
            Ok(WidgetAction::Scroll(*position as u16))
        }
        _ => Err(Error::Invalid("invalid typed widget operation argument")),
    }
}
impl crate::host::ReplyPolicy for WidgetSnapshot {
    type ScalarReply = ControlsScalarReply;
}
impl ProfileSnapshot for WidgetSnapshot {
    const SELECTOR: &'static str = "";
    const OPERATION: &'static str = "gui.set_text";
    const ARGUMENT: &'static str = "text";
    const CONTROLS: bool = true;
    const WIDGETS: bool = true;
    fn native_identity(&self) -> Option<u64> {
        Some(self.target)
    }
    fn validate(&self) -> Result<(), Error> {
        self.scope.validate()?;
        self.widget().map(|_| ())
    }
    fn key(&self) -> &str {
        self.scope
            .widgets
            .iter()
            .find(|widget| widget.control.identity == self.target)
            .map_or("", |widget| widget.control.key.as_str())
    }
    fn selector_matches(&self, role: &str, identifier: &str, label: &str) -> bool {
        self.widget().is_ok_and(|widget| {
            let control = &widget.control;
            (role.is_empty() || control.role == role)
                && (identifier.is_empty() || control.identifier == identifier)
                && (label.is_empty() || control.label == label)
        })
    }
    fn value_selector_matches(&self, role: &str, value: &str) -> bool {
        self.widget().is_ok_and(|widget| {
            widget.control.role == role && widget.control.value.as_deref() == Some(value)
        })
    }
    fn same_instance(&self, other: &Self) -> bool {
        self.scope.identity == other.scope.identity
            && self.target == other.target
            && self.key() == other.key()
            && self.widget().ok().map(|widget| &widget.control.role)
                == other.widget().ok().map(|widget| &widget.control.role)
    }
    fn copy_cost(&self) -> u64 {
        self.widget().map_or(1, |widget| {
            let c = &widget.control;
            (c.key.len()
                + c.role.len()
                + c.identifier.len()
                + c.label.len()
                + c.value.as_ref().map_or(1, |value| value.len() + 1)
                + widget
                    .orientation
                    .as_ref()
                    .map_or(1, |value| value.len() + 1)
                + 16) as u64
        })
    }
    fn describe(&self) -> String {
        self.widget().map_or_else(|_| "unavailable widget".into(), |widget| format!("key={:?}, role={:?}, identifier={:?}, label={:?}, value={:?}, checked={:?}, selected={:?}, position={:?}, actions={:?}", widget.control.key, widget.control.role, widget.control.identifier, widget.control.label, widget.control.value, widget.checked, widget.selected, widget.position, widget.actions()))
    }
    fn field_type(field: Field) -> Option<ScalarType> {
        match field {
            Field::Key
            | Field::Role
            | Field::Identifier
            | Field::Label
            | Field::CheckState
            | Field::Orientation => Some(ScalarType::String),
            Field::Value | Field::ParentKey => Some(ScalarType::OptionalString),
            Field::Enabled
            | Field::Editable
            | Field::Pressable
            | Field::Selected
            | Field::SelectionAvailable
            | Field::PositionAvailable => Some(ScalarType::Boolean),
            Field::Actions | Field::SelectedKeys => Some(ScalarType::StringList),
            Field::Revision | Field::Position => Some(ScalarType::Integer),
            _ => None,
        }
    }
    fn field(&self, field: Field) -> Result<ScalarValue, Error> {
        let widget = self.widget()?;
        let c = &widget.control;
        Ok(match field {
            Field::Key => ScalarValue::String(c.key.clone()),
            Field::Role => ScalarValue::String(c.role.clone()),
            Field::Identifier => ScalarValue::String(c.identifier.clone()),
            Field::Label => ScalarValue::String(c.label.clone()),
            Field::Value => ScalarValue::OptionalString(OptionalStringValue(c.value.clone())),
            Field::Enabled => ScalarValue::Boolean(c.enabled),
            Field::Editable => ScalarValue::Boolean(c.editable),
            Field::Pressable => ScalarValue::Boolean(c.pressable),
            Field::Actions => ScalarValue::StringList(StringListValue(widget.actions())),
            Field::CheckState => ScalarValue::String(
                match widget.checked {
                    None => "unavailable",
                    Some(CheckState::Unchecked) => "unchecked",
                    Some(CheckState::Checked) => "checked",
                    Some(CheckState::Mixed) => "mixed",
                }
                .into(),
            ),
            Field::Selected => ScalarValue::Boolean(widget.selected.unwrap_or(false)),
            Field::SelectionAvailable => ScalarValue::Boolean(widget.selected.is_some()),
            Field::SelectedKeys => ScalarValue::StringList(StringListValue(
                self.scope
                    .widgets
                    .iter()
                    .filter(|item| item.parent == Some(self.target) && item.selected == Some(true))
                    .map(|item| item.control.key.clone())
                    .collect(),
            )),
            Field::Position => ScalarValue::Integer(widget.position.map_or(0, u64::from)),
            Field::PositionAvailable => ScalarValue::Boolean(widget.position.is_some()),
            Field::Orientation => {
                ScalarValue::String(widget.orientation.clone().unwrap_or_default())
            }
            Field::ParentKey => ScalarValue::OptionalString(OptionalStringValue(
                widget
                    .parent
                    .map(|parent| {
                        self.scope
                            .widget(parent)
                            .map(|parent| parent.control.key.clone())
                    })
                    .transpose()?,
            )),
            _ => {
                return Err(Error::Invalid(
                    "field is unavailable in the widgets profile",
                ));
            }
        })
    }
    fn admit_action(&self, _: &str) -> Result<(), Error> {
        Err(Error::Invalid("widgets require a typed operation"))
    }
    fn admit_operation(&self, operation: &str, argument: &str) -> Result<(), Error> {
        let widget = self.widget()?;
        if argument.len() > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        if operation == "gui.set_text"
            && widget.control.enabled
            && widget.control.editable
            && text_role(&widget.control.role)
        {
            Ok(())
        } else {
            Err(Error::Invalid(
                "text operation is not supported by this widget",
            ))
        }
    }
    fn admit_typed_operation(
        &self,
        operation: &str,
        argument: &ActionArgument,
    ) -> Result<(), Error> {
        if let ActionArgument::Text(text) = argument {
            self.admit_operation(operation, text)
        } else {
            self.scope.admit(self.target, &action(operation, argument)?)
        }
    }
}
pub fn discover(backend: &dyn WidgetsBackend) -> Result<WidgetScope, Error> {
    backend.check_permission()?;
    backend.snapshot()?.normalize()
}
pub fn with_host<T>(
    backend: &dyn WidgetsBackend,
    use_host: impl FnOnce(
        &crate::host::Host<'_, '_, WidgetSnapshot>,
        leselang_vm::native::core::LocalExecutionScope,
    ) -> T,
) -> Result<T, Error> {
    crate::host::with_profile(&WidgetsProfile(backend), use_host)
}
/// Opt in to the fourteen-operation catalog, adding source-declared
/// `gui.wait_value`. Drive that request through `observation_wait::begin_source`
/// and `step`, then explicitly accept its Ready receipt.
pub fn with_wait_host<T>(
    backend: &dyn WidgetsBackend,
    use_host: impl FnOnce(
        &crate::host::Host<'_, '_, WidgetSnapshot>,
        leselang_vm::native::core::LocalExecutionScope,
    ) -> T,
) -> Result<T, Error> {
    crate::host::with_profile_waits(&WidgetsProfile(backend), use_host)
}
/// Opt in to seventeen operations, including typed waits for checkbox state,
/// collection-item selection and scrollbar position as well as text values.
pub fn with_state_wait_host<T>(
    backend: &dyn WidgetsBackend,
    use_host: impl FnOnce(
        &crate::host::Host<'_, '_, WidgetSnapshot>,
        leselang_vm::native::core::LocalExecutionScope,
    ) -> T,
) -> Result<T, Error> {
    crate::host::with_profile_state_waits(&WidgetsProfile(backend), use_host)
}
pub fn run(
    backend: &dyn WidgetsBackend,
    source: &str,
) -> Result<driver::ProfileRunSummary<WidgetSnapshot>, String> {
    driver::run_profile(&WidgetsProfile(backend), source, 10000)
}
/// Run with source waits using a caller-owned monotonic clock and explicit
/// scheduling callback. `wait_until` receives an absolute time in that clock's
/// origin; no scheduler or sleep is installed by the library.
pub fn run_with_waits(
    backend: &dyn WidgetsBackend,
    source: &str,
    clock: &mut impl FnMut() -> std::time::Duration,
    wait_until: &mut impl FnMut(std::time::Duration),
) -> Result<driver::ProfileRunSummary<WidgetSnapshot>, String> {
    driver::run_profile_with_waits(&WidgetsProfile(backend), source, 10000, clock, wait_until)
}
/// Run the seventeen-operation catalog with an explicit host clock and
/// scheduling callback; source waits never retry a mutation.
pub fn run_with_state_waits(
    backend: &dyn WidgetsBackend,
    source: &str,
    clock: &mut impl FnMut() -> std::time::Duration,
    wait_until: &mut impl FnMut(std::time::Duration),
) -> Result<driver::ProfileRunSummary<WidgetSnapshot>, String> {
    driver::run_profile_with_state_waits(&WidgetsProfile(backend), source, 10000, clock, wait_until)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        driver::Vm,
        host::{Host, ProfileReceipt, STACK, Work},
    };
    use leselang_vm::native::{
        HostWorkOutcome, LocalVm,
        core::*,
        hir::effect_session::{EffectSessionEnd, EffectSessionStatus},
    };
    use std::cell::{Cell, RefCell};

    fn widget(identity: u64, role: &str, identifier: &str) -> WidgetInfo {
        WidgetInfo {
            control: ControlInfo {
                identity,
                key: format!("connection/{identity}"),
                role: role.into(),
                identifier: identifier.into(),
                label: identifier.into(),
                value: None,
                enabled: true,
                editable: false,
                pressable: role == "AXButton",
            },
            checked: None,
            selected: None,
            position: None,
            orientation: None,
            capabilities: WidgetCapabilities::default(),
            parent: None,
        }
    }
    struct Fake {
        state: RefCell<WidgetScope>,
        permitted: Cell<bool>,
        calls: RefCell<Vec<WidgetAction>>,
        changes: Cell<usize>,
        behavior: Cell<u8>,
    }
    impl Fake {
        fn new() -> Self {
            let mut popup = widget(1, "AXPopUpButton", "format");
            popup.control.value = Some("One".into());
            popup.capabilities.open_menu = true;
            let mut check = widget(2, "AXCheckBox", "enabled");
            check.checked = Some(CheckState::Unchecked);
            check.capabilities.set_checked = true;
            let mut list = widget(3, "AXList", "items");
            list.capabilities.select_item = true;
            let mut first = widget(4, "AXRow", "first");
            first.parent = Some(3);
            first.selected = Some(true);
            let mut second = widget(5, "AXRow", "second");
            second.parent = Some(3);
            second.selected = Some(false);
            let mut scroll = widget(6, "AXScrollBar", "vertical");
            scroll.position = Some(0);
            scroll.orientation = Some("vertical".into());
            scroll.capabilities.scroll = true;
            let mut text = widget(7, "AXTextArea", "editor");
            text.control.editable = true;
            text.control.value = Some("initial".into());
            Self {
                state: RefCell::new(WidgetScope {
                    identity: 100,
                    revision: 1,
                    widgets: vec![
                        popup,
                        check,
                        list,
                        first,
                        second,
                        scroll,
                        text,
                        widget(8, "AXButton", "button"),
                    ],
                }),
                permitted: Cell::new(true),
                calls: RefCell::new(Vec::new()),
                changes: Cell::new(0),
                behavior: Cell::new(0),
            }
        }
        fn change(&self, change: impl FnOnce(&mut WidgetScope)) {
            let mut state = self.state.borrow_mut();
            change(&mut state);
            state.revision += 1;
        }
        fn open(&self) {
            self.change(|state| {
                let popup = state
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == 1)
                    .unwrap();
                popup.capabilities.open_menu = false;
                popup.capabilities.choose_menu_item = true;
                popup.capabilities.cancel_menu = true;
                let mut menu = widget(20, "AXMenu", "menu");
                menu.parent = Some(1);
                let mut item = widget(21, "AXMenuItem", "two");
                item.control.label = "Two".into();
                item.parent = Some(20);
                state.widgets.extend([menu, item]);
            });
        }
    }
    impl WidgetsBackend for Fake {
        fn snapshot(&self) -> Result<WidgetScope, Error> {
            self.check_permission()?;
            Ok(self.state.borrow().clone())
        }
        fn check_permission(&self) -> Result<(), Error> {
            if self.permitted.get() {
                Ok(())
            } else {
                Err(Error::PermissionDenied)
            }
        }
        fn perform(
            &self,
            expected: &WidgetScope,
            target: u64,
            action: &WidgetAction,
        ) -> Result<(), Error> {
            self.check_permission()?;
            if self.behavior.get() == 2
                || (self.behavior.get() == 4 && !self.calls.borrow().is_empty())
            {
                self.change(|state| state.widgets[6].control.value = Some("external".into()));
            }
            if self.snapshot()? != *expected {
                return Err(Error::Stale);
            }
            expected.admit(target, action)?;
            self.calls.borrow_mut().push(action.clone());
            if self.behavior.get() == 3 {
                return Ok(());
            }
            if let WidgetAction::SetChecked(desired) = action {
                let current = self.state.borrow().widget(target)?.checked;
                if current
                    == Some(if *desired {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    })
                {
                    return Ok(());
                }
            }
            self.changes.set(self.changes.get() + 1);
            match action {
                WidgetAction::OpenMenu => self.open(),
                WidgetAction::ChooseMenuItem { item } => {
                    let label = expected.widget(*item)?.control.label.clone();
                    self.change(|state| {
                        state
                            .widgets
                            .retain(|widget| !matches!(widget.control.identity, 20 | 21));
                        let owner = state
                            .widgets
                            .iter_mut()
                            .find(|widget| widget.control.identity == target)
                            .unwrap();
                        owner.control.value = Some(label);
                        owner.capabilities.open_menu = true;
                        owner.capabilities.choose_menu_item = false;
                        owner.capabilities.cancel_menu = false;
                    });
                }
                WidgetAction::CancelMenu => self.change(|state| {
                    state
                        .widgets
                        .retain(|widget| !matches!(widget.control.identity, 20 | 21));
                    let owner = state
                        .widgets
                        .iter_mut()
                        .find(|widget| widget.control.identity == target)
                        .unwrap();
                    owner.capabilities.open_menu = true;
                    owner.capabilities.choose_menu_item = false;
                    owner.capabilities.cancel_menu = false;
                }),
                WidgetAction::SelectItem { item } => self.change(|state| {
                    for widget in &mut state.widgets {
                        if widget.parent == Some(target) {
                            widget.selected = Some(widget.control.identity == *item);
                        }
                    }
                }),
                WidgetAction::SetChecked(desired) => self.change(|state| {
                    state
                        .widgets
                        .iter_mut()
                        .find(|widget| widget.control.identity == target)
                        .unwrap()
                        .checked = Some(if *desired {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    })
                }),
                WidgetAction::Scroll(position) => self.change(|state| {
                    state
                        .widgets
                        .iter_mut()
                        .find(|widget| widget.control.identity == target)
                        .unwrap()
                        .position = Some(*position)
                }),
                WidgetAction::SetText(text) => self.change(|state| {
                    state
                        .widgets
                        .iter_mut()
                        .find(|widget| widget.control.identity == target)
                        .unwrap()
                        .control
                        .value = Some(text.clone())
                }),
                WidgetAction::Press => self.change(|_| ()),
            }
            if self.behavior.get() == 1 {
                Err(Error::Uncertain(-25204))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn typed_wait_projection_distinguishes_missing_mixed_and_legitimate_zero_values() {
        let fake = Fake::new();
        let profile = WidgetsProfile(&fake);
        let sample = |target| WidgetSnapshot {
            scope: Rc::new(fake.snapshot().unwrap()),
            target,
        };
        assert_eq!(
            profile
                .source_wait_state(&sample(2), SourceWaitKind::Checked)
                .unwrap()
                .value,
            Some(ScalarValue::Boolean(false))
        );
        assert_eq!(
            profile
                .source_wait_state(&sample(5), SourceWaitKind::Selected)
                .unwrap()
                .value,
            Some(ScalarValue::Boolean(false))
        );
        assert_eq!(
            profile
                .source_wait_state(&sample(6), SourceWaitKind::Position)
                .unwrap()
                .value,
            Some(ScalarValue::Integer(0))
        );
        fake.change(|scope| scope.widgets[1].checked = Some(CheckState::Mixed));
        assert_eq!(
            profile
                .source_wait_state(&sample(2), SourceWaitKind::Checked)
                .unwrap()
                .value,
            None
        );
        fake.change(|scope| {
            scope.widgets[1].checked = None;
            scope.widgets[4].selected = None;
            scope.widgets[5].position = None;
        });
        // Historical field getters intentionally keep their existing defaults;
        // source waits must instead reject unavailable raw state.
        assert_eq!(
            sample(5).field(Field::Selected).unwrap(),
            ScalarValue::Boolean(false)
        );
        assert_eq!(
            sample(6).field(Field::Position).unwrap(),
            ScalarValue::Integer(0)
        );
        for (target, kind) in [
            (2, SourceWaitKind::Checked),
            (5, SourceWaitKind::Selected),
            (6, SourceWaitKind::Position),
        ] {
            assert!(matches!(
                profile.source_wait_state(&sample(target), kind),
                Err(Error::Invalid(_))
            ));
        }
        assert!(fake.calls.borrow().is_empty());
    }
    const CHECK: &str = r#"fn main() = bind(target: gui.select(role: "AXCheckBox", identifier: "enabled", label: ""), body: bind(before: gui.observe(target: target), body: bind(sent: gui.set_checked(target: target, expected_revision: field(value: before, name: "revision"), checked: true), body: bind(after: gui.observe(target: target), body: field(value: after, name: "check_state")))))"#;
    const SELECT: &str = r#"fn main() = bind(target: gui.select(role: "AXList", identifier: "items", label: ""), body: bind(item: gui.select(role: "AXRow", identifier: "second", label: ""), body: bind(before: gui.observe(target: target), body: bind(sent: gui.select_item(target: target, expected_revision: field(value: before, name: "revision"), item: item), body: bind(after: gui.observe(target: target), body: field(value: after, name: "selected_keys"))))))"#;
    fn session(
        fake: &Fake,
        source: &str,
        body: impl for<'ir, 's> FnOnce(&Host<'_, 's, WidgetSnapshot>, &mut Vm<'ir, 's, WidgetSnapshot>),
    ) {
        with_host(fake, |host, scope| {
            let node = host.compile(source).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10000), STACK);
            body(host, &mut vm);
        })
        .unwrap();
    }
    fn advance<'ir, 's>(
        host: &Host<'_, 's, WidgetSnapshot>,
        vm: &mut Vm<'ir, 's, WidgetSnapshot>,
        operation: &str,
    ) {
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), operation);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        driver::accept(vm, host, receipt).unwrap();
    }
    fn ready<'ir, 's>(
        host: &Host<'_, 's, WidgetSnapshot>,
        vm: &mut Vm<'ir, 's, WidgetSnapshot>,
    ) -> Work<'s, WidgetSnapshot> {
        advance(host, vm, "gui.select");
        advance(host, vm, "gui.observe");
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), "gui.set_checked");
        work
    }
    #[test]
    fn menu_choice_acknowledges_stable_owner_after_item_disappears() {
        let fake = Fake::new();
        let source = r#"fn main() = bind(owner: gui.select(role: "AXPopUpButton", identifier: "format", label: ""), body: bind(before: gui.observe(target: owner), body: bind(opened: gui.open_menu(target: owner, expected_revision: field(value: before, name: "revision")), body: bind(item: gui.select(role: "AXMenuItem", identifier: "two", label: ""), body: bind(current: gui.observe(target: owner), body: bind(chosen: gui.choose_menu_item(target: owner, expected_revision: field(value: current, name: "revision"), item: item), body: bind(after: gui.observe(target: owner), body: field(value: after, name: "value"))))))))"#;
        let result = run(&fake, source).unwrap();
        assert_eq!(result.effects, 7);
        assert_eq!(
            result.value,
            ScalarValue::OptionalString(OptionalStringValue(Some("Two".into())))
        );
        assert!(fake.state.borrow().widget(21).is_err());
        assert_eq!(
            *fake.calls.borrow(),
            [
                WidgetAction::OpenMenu,
                WidgetAction::ChooseMenuItem { item: 21 }
            ]
        );
        assert_eq!(
            result.observations[0]
                .widget()
                .unwrap()
                .control
                .value
                .as_deref(),
            Some("One")
        );
    }
    #[test]
    fn menu_cancel_uses_owner_and_can_be_observed_after_menu_disappears() {
        let fake = Fake::new();
        fake.open();
        let source = r#"fn main() = bind(owner: gui.select(role: "AXPopUpButton", identifier: "format", label: ""), body: bind(before: gui.observe(target: owner), body: bind(sent: gui.cancel_menu(target: owner, expected_revision: field(value: before, name: "revision")), body: bind(after: gui.observe(target: owner), body: field(value: after, name: "value")))))"#;
        assert_eq!(run(&fake, source).unwrap().effects, 4);
        assert!(fake.state.borrow().widget(21).is_err());
        assert_eq!(
            fake.state
                .borrow()
                .widget(1)
                .unwrap()
                .control
                .value
                .as_deref(),
            Some("One")
        );
    }
    #[test]
    fn checkbox_desired_state_is_idempotent_and_snapshots_remain_historical() {
        let fake = Fake::new();
        let first = run(&fake, CHECK).unwrap();
        assert_eq!(first.effects, 4);
        assert_eq!(first.value, ScalarValue::String("checked".into()));
        assert_eq!(
            first.observations[0].widget().unwrap().checked,
            Some(CheckState::Unchecked)
        );
        assert_eq!(
            first.observations[1].widget().unwrap().checked,
            Some(CheckState::Checked)
        );
        assert_eq!(
            run(&fake, CHECK).unwrap().value,
            ScalarValue::String("checked".into())
        );
        assert_eq!(fake.changes.get(), 1);
    }
    #[test]
    fn list_selection_uses_item_authority_and_observes_owner_selected_keys() {
        let fake = Fake::new();
        let result = run(&fake, SELECT).unwrap();
        assert_eq!(result.effects, 5);
        assert_eq!(
            result.value,
            ScalarValue::StringList(StringListValue(vec!["connection/5".into()]))
        );
        assert_eq!(*fake.calls.borrow(), [WidgetAction::SelectItem { item: 5 }]);
        assert_eq!(
            result.observations[0].scope.widget(4).unwrap().selected,
            Some(true)
        );
    }
    #[test]
    fn scroll_projects_normalized_position_and_orientation() {
        let fake = Fake::new();
        let source = r#"fn main() = bind(target: gui.select(role: "AXScrollBar", identifier: "vertical", label: ""), body: bind(before: gui.observe(target: target), body: bind(sent: gui.scroll(target: target, expected_revision: field(value: before, name: "revision"), position: 750), body: bind(after: gui.observe(target: target), body: field(value: after, name: "position")))))"#;
        let result = run(&fake, source).unwrap();
        assert_eq!(result.effects, 4);
        assert_eq!(result.value, ScalarValue::Integer(750));
        assert!(
            run(
                &Fake::new(),
                &source.replace("position: 750", "position: 1001")
            )
            .is_err()
        );
        assert_eq!(
            result.observations[0].field(Field::Orientation).unwrap(),
            ScalarValue::String("vertical".into())
        );
    }
    #[test]
    fn rejects_mixed_unknown_disabled_and_mistyped_actions_before_delivery() {
        for cause in 0..5 {
            let fake = Fake::new();
            fake.change(|state| match cause {
                0 => state.widgets[1].checked = Some(CheckState::Mixed),
                1 => state.widgets[1].checked = None,
                2 => state.widgets[1].control.enabled = false,
                3 => state.widgets[1].capabilities.set_checked = false,
                4 => state.widgets[1].control.role = "AXButton".into(),
                _ => unreachable!(),
            });
            assert!(run(&fake, CHECK).is_err(), "{cause}");
            assert!(fake.calls.borrow().is_empty());
        }
        for source in [
            CHECK.replace("checked: true", "checked: \"true\""),
            SELECT.replace("item: item", "item: \"connection/5\""),
            CHECK.replace("gui.set_checked", "gui.press"),
        ] {
            let fake = Fake::new();
            assert!(run(&fake, &source).is_err());
            assert!(fake.calls.borrow().is_empty());
        }
    }
    #[test]
    fn selected_item_must_belong_to_exact_collection_owner() {
        let fake = Fake::new();
        fake.change(|state| {
            let mut other = widget(9, "AXList", "other");
            other.capabilities.select_item = true;
            state.widgets.push(other);
            state.widgets[4].parent = Some(9);
        });
        assert!(run(&fake, SELECT).is_err());
        assert!(fake.calls.borrow().is_empty());
    }
    #[test]
    fn all_widget_actions_keep_original_scope_revision_permission_and_capability_fences() {
        for queued in [false, true] {
            for cause in 0..8 {
                let fake = Fake::new();
                session(&fake, CHECK, |host, vm| {
                    let mut work = Some(ready(host, vm));
                    let receipt =
                        queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                    match cause {
                        0 => fake.change(|s| s.widgets[6].control.value = Some("elsewhere".into())),
                        1 => fake.change(|s| s.widgets[1].checked = Some(CheckState::Checked)),
                        2 => fake.change(|s| s.widgets[1].capabilities.set_checked = false),
                        3 => fake.change(|s| s.widgets[1].control.enabled = false),
                        4 => fake.permitted.set(false),
                        5 => host.set_granted(false),
                        6 => fake.change(|s| s.revision += 1),
                        7 => fake.change(|s| s.widgets[1].control.identity = 99),
                        _ => unreachable!(),
                    };
                    let outcome = if let Some(receipt) = receipt {
                        driver::pump(vm, host, receipt)
                    } else {
                        vm.with_metered_host_outcome(work.take().unwrap(), |work, fuel| {
                            host.dispatch(work, fuel)
                        })
                        .unwrap()
                    };
                    assert!(outcome.is_err(), "queued={queued},cause={cause}");
                    assert!(fake.calls.borrow().is_empty());
                    assert_eq!(
                        vm.status(),
                        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
                    );
                });
            }
        }
    }
    #[test]
    fn queued_selection_rechecks_item_membership_and_identity() {
        for cause in 0..3 {
            let fake = Fake::new();
            session(&fake, SELECT, |host, vm| {
                advance(host, vm, "gui.select");
                advance(host, vm, "gui.select");
                advance(host, vm, "gui.observe");
                let work = driver::next(vm, host).unwrap();
                let receipt = driver::dispatch(vm, host, work).unwrap();
                fake.change(|scope| match cause {
                    0 => {
                        scope.widgets.remove(4);
                    }
                    1 => scope.widgets[4].control.identity = 55,
                    2 => scope.widgets[4].parent = None,
                    _ => unreachable!(),
                });
                assert!(driver::pump(vm, host, receipt).is_err());
                assert!(fake.calls.borrow().is_empty());
            });
        }
    }
    #[test]
    fn cancel_and_late_acknowledgement_do_not_deliver_queued_or_undo_completed_actions() {
        for delivered in [false, true] {
            let fake = Fake::new();
            session(&fake, CHECK, |host, vm| {
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
                    assert!(matches!(
                        host.pump(
                            ProfileReceipt {
                                identity: receipt.identity,
                                reply: rejected.reply
                            },
                            &mut Fuel::new(100)
                        ),
                        HostWorkOutcome::Failed(Err(_))
                    ));
                }
                assert_eq!(fake.changes.get(), usize::from(delivered));
            });
        }
    }
    #[test]
    fn fuel_exhaustion_before_delivery_has_no_native_mutation() {
        let fake = Fake::new();
        session(&fake, CHECK, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let remaining = vm.fuel_remaining();
            vm.with_metered_host_outcome((), |_, fuel| {
                fuel.charge(remaining).unwrap();
                HostWorkOutcome::Pending(())
            })
            .unwrap();
            assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Fuel)));
            assert!(fake.calls.borrow().is_empty());
        });
    }
    #[test]
    fn uncertain_native_result_preserves_side_effect_without_retry() {
        let fake = Fake::new();
        session(&fake, CHECK, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            fake.behavior.set(1);
            assert!(matches!(
                driver::pump(vm, host, receipt),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(
                fake.state.borrow().widget(2).unwrap().checked,
                Some(CheckState::Checked)
            );
            assert_eq!(fake.calls.borrow().len(), 1);
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::HostUncertain)
            );
        });
    }
    #[test]
    fn backend_last_boundary_race_refuses_before_native_entry() {
        let fake = Fake::new();
        fake.behavior.set(2);
        assert!(run(&fake, CHECK).is_err());
        assert!(fake.calls.borrow().is_empty());
    }
    #[test]
    fn acknowledgement_does_not_substitute_for_observed_postcondition() {
        let fake = Fake::new();
        fake.behavior.set(3);
        assert_eq!(
            run(&fake, CHECK).unwrap().value,
            ScalarValue::String("unchecked".into())
        );
        assert_eq!(fake.changes.get(), 0);
    }
    #[test]
    fn inventory_rejects_invalid_membership_cycles_capabilities_and_bounds() {
        for cause in 0..9 {
            let fake = Fake::new();
            fake.change(|s| match cause {
                0 => s.widgets[0].parent = Some(1),
                1 => s.widgets[0].parent = Some(999),
                2 => s.widgets[1].capabilities.scroll = true,
                3 => s.widgets[5].position = Some(1001),
                4 => s.widgets[5].orientation = Some("diagonal".into()),
                5 => s.widgets[1].control.key = s.widgets[0].control.key.clone(),
                6 => s.widgets[4].parent = None,
                7 => s.widgets[4].control.label = "x".repeat(4097),
                8 => s.identity = 0,
                _ => unreachable!(),
            });
            assert!(discover(&fake).is_err(), "{cause}");
        }
    }
    #[test]
    fn a_later_widget_failure_preserves_the_first_completed_effect() {
        let fake = Fake::new();
        fake.behavior.set(4);
        let source = r#"fn check() = bind(target: gui.select(role: "AXCheckBox", identifier: "enabled", label: ""), body: bind(before: gui.observe(target: target), body: gui.set_checked(target: target, expected_revision: field(value: before, name: "revision"), checked: true)))
fn main() = bind(first: check(), body: bind(target: gui.select(role: "AXScrollBar", identifier: "vertical", label: ""), body: bind(before: gui.observe(target: target), body: gui.scroll(target: target, expected_revision: field(value: before, name: "revision"), position: 500))))"#;
        assert!(run(&fake, source).is_err());
        assert_eq!(
            fake.state.borrow().widget(2).unwrap().checked,
            Some(CheckState::Checked)
        );
        assert_eq!(fake.state.borrow().widget(6).unwrap().position, Some(0));
        assert_eq!(*fake.calls.borrow(), [WidgetAction::SetChecked(true)]);
    }
    #[test]
    fn menu_item_must_descend_from_the_requested_stable_owner() {
        let fake = Fake::new();
        fake.open();
        fake.change(|state| {
            let mut other = widget(9, "AXPopUpButton", "other");
            other.capabilities.choose_menu_item = true;
            state.widgets.push(other);
            state
                .widgets
                .iter_mut()
                .find(|widget| widget.control.identity == 20)
                .unwrap()
                .parent = Some(9);
        });
        let source = r#"fn main() = bind(owner: gui.select(role: "AXPopUpButton", identifier: "format", label: ""), body: bind(item: gui.select(role: "AXMenuItem", identifier: "two", label: ""), body: bind(before: gui.observe(target: owner), body: gui.choose_menu_item(target: owner, expected_revision: field(value: before, name: "revision"), item: item))))"#;
        assert!(run(&fake, source).is_err());
        assert!(fake.calls.borrow().is_empty());
    }
    #[test]
    fn old_controls_catalog_does_not_acquire_widget_operations() {
        let source = CHECK
            .replace("AXCheckBox", "AXTextArea")
            .replace("enabled", "status");
        struct Old;
        impl crate::controls::ControlsBackend for Old {
            fn snapshot(&self) -> Result<crate::controls::ScopeSnapshot, Error> {
                Ok(crate::controls::ScopeSnapshot {
                    identity: 1,
                    revision: 1,
                    controls: vec![widget(2, "AXTextArea", "status").control],
                })
            }
            fn check_permission(&self) -> Result<(), Error> {
                Ok(())
            }
            fn press(&self, _: &crate::controls::ScopeSnapshot, _: u64) -> Result<(), Error> {
                panic!("not called")
            }
            fn set_text(
                &self,
                _: &crate::controls::ScopeSnapshot,
                _: u64,
                _: &str,
            ) -> Result<(), Error> {
                panic!("not called")
            }
        }
        assert!(crate::controls::run(&Old, &source).is_err());
        assert!(crate::controls::run(&Old, VALUE_SELECT).is_err());
    }

    const VALUE_SELECT: &str = r#"fn main() = bind(target: gui.select_value(role: "AXTextArea", value: "initial"), body: bind(observed: gui.observe(target: target), body: field(value: observed, name: "key")))"#;

    #[test]
    fn value_selection_is_exact_and_distinguishes_empty_from_missing() {
        let fake = Fake::new();
        assert_eq!(
            run(&fake, VALUE_SELECT).unwrap().value,
            ScalarValue::String("connection/7".into())
        );
        for source in [
            VALUE_SELECT.replace("initial", "Initial"),
            VALUE_SELECT.replace("initial", "init"),
            VALUE_SELECT.replace("AXTextArea", "AXTextField"),
            VALUE_SELECT.replace("AXTextArea", ""),
            VALUE_SELECT.replace("value: \"initial\"", "value: true"),
        ] {
            assert!(run(&fake, &source).is_err());
        }
        let empty = VALUE_SELECT.replace("initial", "");
        fake.change(|scope| scope.widgets[6].control.value = None);
        assert!(run(&fake, &empty).is_err());
        fake.change(|scope| scope.widgets[6].control.value = Some(String::new()));
        assert_eq!(
            run(&fake, &empty).unwrap().value,
            ScalarValue::String("connection/7".into())
        );
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn value_selection_never_filters_disabled_duplicates_or_grants_write_access() {
        let fake = Fake::new();
        fake.change(|scope| {
            let mut duplicate = widget(9, "AXTextArea", "duplicate");
            duplicate.control.value = Some("initial".into());
            duplicate.control.enabled = false;
            scope.widgets.push(duplicate);
        });
        assert!(run(&fake, VALUE_SELECT).unwrap_err().contains("ambiguous"));
        fake.change(|scope| {
            scope.widgets.pop();
            scope.widgets[6].control.enabled = false;
        });
        assert_eq!(
            run(&fake, VALUE_SELECT).unwrap().value,
            ScalarValue::String("connection/7".into())
        );
        let write = r#"fn main() = bind(target: gui.select_value(role: "AXTextArea", value: "initial"), body: bind(before: gui.observe(target: target), body: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "updated")))"#;
        assert!(run(&fake, write).is_err());
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn value_selection_keeps_permission_and_object_lifetime_fences() {
        for at_accept in [false, true] {
            for cause in 0..4 {
                let fake = Fake::new();
                session(&fake, VALUE_SELECT, |host, vm| {
                    let work = driver::next(vm, host).unwrap();
                    assert_eq!(work.operation(), "gui.select_value");
                    if at_accept {
                        let receipt = driver::dispatch(vm, host, work).unwrap();
                        match cause {
                            0 => fake.permitted.set(false),
                            1 => host.set_granted(false),
                            2 => fake.change(|scope| scope.widgets[6].control.identity = 99),
                            3 => {
                                assert!(vm.cancel());
                            }
                            _ => unreachable!(),
                        }
                        assert!(driver::accept(vm, host, receipt).is_err());
                    } else {
                        match cause {
                            0 => fake.permitted.set(false),
                            1 => host.set_granted(false),
                            2 => fake.change(|scope| {
                                scope.widgets[6].control.value = Some("changed".into())
                            }),
                            3 => {
                                assert!(vm.cancel());
                            }
                            _ => unreachable!(),
                        }
                        assert!(driver::dispatch(vm, host, work).is_err());
                    }
                    assert!(fake.calls.borrow().is_empty());
                });
            }
        }
    }
}
