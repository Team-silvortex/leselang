//! Public widgets embedding contracts. These simulated windows never access a GUI.
use leselang_macos_ax_host::{
    Error,
    controls::ControlInfo,
    widgets::{
        self, CheckState, WidgetAction, WidgetCapabilities, WidgetInfo, WidgetScope, WidgetsBackend,
    },
};
use leselang_vm::native::core::{ScalarValue, StringListValue};
use std::cell::{Cell, RefCell};

const PROGRAM: &str = r#"
fn choose_format() = bind(owner: gui.select(role: "AXPopUpButton", identifier: "format", label: ""), body:
    bind(before: gui.observe(target: owner), body:
        bind(opened: gui.open_menu(target: owner, expected_revision: field(value: before, name: "revision")), body:
            bind(item: gui.select(role: "AXMenuItem", identifier: "option", label: ""), body:
                bind(current: gui.observe(target: owner), body:
                    bind(chosen: gui.choose_menu_item(target: owner, expected_revision: field(value: current, name: "revision"), item: item), body:
                        bind(after: gui.observe(target: owner), body:
                            eq(left: value_or(left: field(value: after, name: "value"), right: ""), right: "Two"))))))))
fn cancel_format() = bind(owner: gui.select(role: "AXPopUpButton", identifier: "format", label: ""), body:
    bind(before: gui.observe(target: owner), body:
        bind(opened: gui.open_menu(target: owner, expected_revision: field(value: before, name: "revision")), body:
            bind(current: gui.observe(target: owner), body:
                bind(cancelled: gui.cancel_menu(target: owner, expected_revision: field(value: current, name: "revision")), body:
                    bind(after: gui.observe(target: owner), body:
                        eq(left: value_or(left: field(value: after, name: "value"), right: ""), right: "Two")))))))
fn check_option() = bind(target: gui.select(role: "AXCheckBox", identifier: "option", label: ""), body:
    bind(before: gui.observe(target: target), body:
        bind(submitted: gui.set_checked(target: target, expected_revision: field(value: before, name: "revision"), checked: true), body:
            bind(after: gui.observe(target: target), body:
                eq(left: field(value: after, name: "check_state"), right: "checked")))))
fn select_row() = bind(owner: gui.select(role: "AXList", identifier: "items", label: ""), body:
    bind(item: gui.select(role: "AXRow", identifier: "second", label: ""), body:
        bind(before: gui.observe(target: owner), body:
            bind(submitted: gui.select_item(target: owner, expected_revision: field(value: before, name: "revision"), item: item), body:
                bind(after: gui.observe(target: item), body:
                    and(left: field(value: after, name: "selection_available"), right: field(value: after, name: "selected")))))))
fn scroll_list() = bind(target: gui.select(role: "AXScrollBar", identifier: "vertical", label: ""), body:
    bind(before: gui.observe(target: target), body:
        bind(submitted: gui.scroll(target: target, expected_revision: field(value: before, name: "revision"), position: 750), body:
            bind(after: gui.observe(target: target), body:
                and(left: field(value: after, name: "position_available"), right: eq(left: field(value: after, name: "position"), right: 750))))))
fn main() = bind(menu: choose_format(), body:
    choose(when: menu, then: bind(cancelled: cancel_format(), body:
        choose(when: cancelled, then: bind(checked: check_option(), body:
            choose(when: checked, then: bind(selected: select_row(), body:
                choose(when: selected, then: scroll_list(), otherwise: false)), otherwise: false)), otherwise: false)), otherwise: false))
"#;

fn widget(identity: u64, role: &str, identifier: &str) -> WidgetInfo {
    // Existing ControlInfo struct-literal callers need no new fields.
    WidgetInfo {
        control: ControlInfo {
            identity,
            key: format!("external/{identity}"),
            role: role.into(),
            identifier: identifier.into(),
            label: identifier.into(),
            value: None,
            enabled: true,
            editable: role == "AXTextArea",
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
struct WindowBackend {
    state: RefCell<WidgetScope>,
    next_menu: Cell<u64>,
    delivered: RefCell<Vec<WidgetAction>>,
    wrong_menu_result: bool,
    uncertain_check: bool,
    menu_label: &'static str,
    defer_menu_value: bool,
    unchanged_check: bool,
}
impl WindowBackend {
    fn new() -> Self {
        let mut popup = widget(2, "AXPopUpButton", "format");
        popup.control.value = Some("One".into());
        popup.capabilities.open_menu = true;
        let mut check = widget(3, "AXCheckBox", "option");
        check.checked = Some(CheckState::Unchecked);
        check.capabilities.set_checked = true;
        let mut list = widget(4, "AXList", "items");
        list.capabilities.select_item = true;
        let mut first = widget(5, "AXRow", "first");
        first.parent = Some(4);
        first.selected = Some(true);
        let mut second = widget(6, "AXRow", "second");
        second.parent = Some(4);
        second.selected = Some(false);
        let mut scroll = widget(7, "AXScrollBar", "vertical");
        scroll.position = Some(0);
        scroll.orientation = Some("vertical".into());
        scroll.capabilities.scroll = true;
        Self {
            state: RefCell::new(WidgetScope {
                identity: 1,
                revision: 1,
                widgets: vec![
                    popup,
                    check,
                    list,
                    first,
                    second,
                    scroll,
                    widget(8, "AXTextArea", "editor"),
                    widget(9, "AXButton", "button"),
                ],
            }),
            next_menu: Cell::new(20),
            delivered: RefCell::new(Vec::new()),
            wrong_menu_result: false,
            uncertain_check: false,
            menu_label: "Two",
            defer_menu_value: false,
            unchanged_check: false,
        }
    }
}
impl WidgetsBackend for WindowBackend {
    fn snapshot(&self) -> Result<WidgetScope, Error> {
        Ok(self.state.borrow().clone())
    }
    fn check_permission(&self) -> Result<(), Error> {
        Ok(())
    }
    fn perform(
        &self,
        expected: &WidgetScope,
        target: u64,
        action: &WidgetAction,
    ) -> Result<(), Error> {
        let mut current = self.state.borrow_mut();
        // A backend must repeat the complete-scope check at its final boundary.
        if &*current != expected {
            return Err(Error::Stale);
        }
        expected.admit(target, action)?;
        self.delivered.borrow_mut().push(action.clone());
        match action {
            WidgetAction::OpenMenu => {
                let identity = self.next_menu.get();
                self.next_menu.set(identity + 2);
                let mut menu = widget(identity, "AXMenu", "menu");
                menu.parent = Some(target);
                let mut item = widget(identity + 1, "AXMenuItem", "option");
                item.parent = Some(identity);
                item.control.label = self.menu_label.into();
                current.widgets.extend([menu, item]);
                let owner = current
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == target)
                    .unwrap();
                owner.capabilities.open_menu = false;
                owner.capabilities.choose_menu_item = true;
                owner.capabilities.cancel_menu = true;
            }
            WidgetAction::ChooseMenuItem { item } => {
                let value = if self.wrong_menu_result {
                    "unexpected".into()
                } else {
                    expected.widget(*item)?.control.label.clone()
                };
                current.widgets.retain(|widget| {
                    !matches!(widget.control.role.as_str(), "AXMenu" | "AXMenuItem")
                });
                let owner = current
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == target)
                    .unwrap();
                if !self.defer_menu_value {
                    owner.control.value = Some(value);
                }
                owner.capabilities.open_menu = true;
                owner.capabilities.choose_menu_item = false;
                owner.capabilities.cancel_menu = false;
            }
            WidgetAction::CancelMenu => {
                current.widgets.retain(|widget| {
                    !matches!(widget.control.role.as_str(), "AXMenu" | "AXMenuItem")
                });
                let owner = current
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == target)
                    .unwrap();
                owner.capabilities.open_menu = true;
                owner.capabilities.choose_menu_item = false;
                owner.capabilities.cancel_menu = false;
            }
            WidgetAction::SetChecked(value) => {
                if !self.unchanged_check {
                    current
                        .widgets
                        .iter_mut()
                        .find(|widget| widget.control.identity == target)
                        .unwrap()
                        .checked = Some(if *value {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    })
                }
            }
            WidgetAction::SelectItem { item } => {
                for widget in &mut current.widgets {
                    if widget.parent == Some(target) {
                        widget.selected = Some(widget.control.identity == *item);
                    }
                }
            }
            WidgetAction::Scroll(position) => {
                current
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == target)
                    .unwrap()
                    .position = Some(*position)
            }
            WidgetAction::SetText(text) => {
                current
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == target)
                    .unwrap()
                    .control
                    .value = Some(text.clone())
            }
            WidgetAction::Press => {
                current
                    .widgets
                    .iter_mut()
                    .find(|widget| widget.control.identity == target)
                    .unwrap()
                    .control
                    .value = Some("pressed".into())
            }
        }
        current.revision += 1;
        if self.uncertain_check && matches!(action, WidgetAction::SetChecked(_)) {
            Err(Error::Uncertain(-25204))
        } else {
            Ok(())
        }
    }
}

#[test]
fn external_backend_drives_all_six_new_actions_with_postconditions() {
    let backend = WindowBackend::new();
    let result = widgets::run(&backend, PROGRAM).unwrap();
    assert_eq!(result.value, ScalarValue::Boolean(true));
    assert_eq!(result.effects, 26);
    assert_eq!(result.observations.len(), 12);
    assert_eq!(
        *backend.delivered.borrow(),
        [
            WidgetAction::OpenMenu,
            WidgetAction::ChooseMenuItem { item: 21 },
            WidgetAction::OpenMenu,
            WidgetAction::CancelMenu,
            WidgetAction::SetChecked(true),
            WidgetAction::SelectItem { item: 6 },
            WidgetAction::Scroll(750),
        ]
    );
    assert_eq!(
        backend.state.borrow().widget(7).unwrap().position,
        Some(750)
    );
    assert!(
        backend
            .state
            .borrow()
            .widgets
            .iter()
            .all(|widget| widget.control.identity < 20)
    );
    assert!(result.fuel_remaining > 0 && result.fuel_remaining < 10_000);
    // The first menu observation remains historical after both menu deliveries.
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
fn public_catalog_contains_new_operations_and_all_six_original_controls_operations() {
    let backend = WindowBackend::new();
    let original = r#"fn main() = bind(keys: gui.list(role: "AXTextArea"), body: bind(target: gui.find(key: fold(selected: "", items: keys, item: "key", next: key, limit: 64)), body: bind(before: gui.observe(target: target), body: bind(text: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "updated"), body: bind(button: gui.select(role: "AXButton", identifier: "button", label: ""), body: bind(state: gui.observe(target: button), body: bind(pressed: gui.press(target: button, expected_revision: field(value: state, name: "revision")), body: bind(after: gui.observe(target: target), body: field(value: after, name: "actions")))))))))"#;
    let result = widgets::run(&backend, original).unwrap();
    assert_eq!(result.effects, 8);
    assert_eq!(
        result.value,
        ScalarValue::StringList(StringListValue(vec!["set_text".into()]))
    );
    assert_eq!(
        backend
            .state
            .borrow()
            .widget(8)
            .unwrap()
            .control
            .value
            .as_deref(),
        Some("updated")
    );
    assert_eq!(
        *backend.delivered.borrow(),
        [WidgetAction::SetText("updated".into()), WidgetAction::Press]
    );
}

#[test]
fn observed_menu_postcondition_controls_whether_later_actions_run() {
    let mut backend = WindowBackend::new();
    backend.wrong_menu_result = true;
    let result = widgets::run(&backend, PROGRAM).unwrap();
    assert_eq!(result.value, ScalarValue::Boolean(false));
    assert_eq!(result.effects, 7);
    assert_eq!(backend.delivered.borrow().len(), 2);
    assert_eq!(
        backend.state.borrow().widget(3).unwrap().checked,
        Some(CheckState::Unchecked)
    );
}

#[test]
fn uncertain_delivery_preserves_prior_and_current_possible_effects() {
    let mut backend = WindowBackend::new();
    backend.uncertain_check = true;
    assert!(widgets::run(&backend, PROGRAM).is_err());
    assert_eq!(backend.delivered.borrow().len(), 5);
    assert_eq!(
        backend
            .state
            .borrow()
            .widget(2)
            .unwrap()
            .control
            .value
            .as_deref(),
        Some("Two")
    );
    assert_eq!(
        backend.state.borrow().widget(3).unwrap().checked,
        Some(CheckState::Checked)
    );
    assert_eq!(backend.state.borrow().widget(7).unwrap().position, Some(0));
}

#[cfg(target_os = "macos")]
#[test]
fn public_native_widget_factory_is_available_without_accessing_a_desktop() {
    use leselang_macos_ax_host::{
        scope::WindowSelector, scope_access::ApplicationAccess, widgets_access::WidgetsAccess,
    };
    let _: fn(&ApplicationAccess, &WindowSelector) -> Result<WidgetsAccess, Error> =
        ApplicationAccess::widgets;
}

const FINDER_REFERENCE: &str = include_str!("../examples/widgets_finder_selection.lese");

fn finder_backend() -> WindowBackend {
    let backend = WindowBackend::new();
    let mut outline = widget(41, "AXOutline", "ListView");
    outline.capabilities.select_item = true;
    let mut row = widget(42, "AXRow", "");
    row.parent = Some(41);
    row.selected = Some(false);
    let mut cell = widget(43, "AXCell", "");
    cell.parent = Some(42);
    let mut text = widget(44, "AXTextField", "");
    text.parent = Some(43);
    text.control.value = Some("Widget folder 01".into());
    let mut scrollbar = widget(45, "AXScrollBar", "");
    scrollbar.position = Some(0);
    scrollbar.orientation = Some("vertical".into());
    scrollbar.capabilities.scroll = true;
    backend.state.borrow_mut().widgets = vec![outline, row, cell, text, scrollbar];
    backend
}

#[test]
fn finder_reference_compiles_and_verifies_selection_and_scroll_with_public_backend() {
    let backend = finder_backend();
    let result = widgets::run(&backend, FINDER_REFERENCE).unwrap();
    assert_eq!(result.value, ScalarValue::Boolean(true));
    assert_eq!(result.effects, 15);
    assert_eq!(
        *backend.delivered.borrow(),
        [
            WidgetAction::SelectItem { item: 42 },
            WidgetAction::Scroll(1000)
        ]
    );
    assert_eq!(
        backend.state.borrow().widget(42).unwrap().selected,
        Some(true)
    );
    assert_eq!(
        backend.state.borrow().widget(45).unwrap().position,
        Some(1000)
    );
}

#[test]
fn finder_reference_refuses_missing_ambiguous_or_mismatched_initial_selection() {
    for cause in 0..5 {
        let backend = finder_backend();
        {
            let mut state = backend.state.borrow_mut();
            match cause {
                0 => state.widgets[3].control.value = Some("Another folder".into()),
                1 => {
                    let mut duplicate = state.widgets[3].clone();
                    duplicate.control.identity = 46;
                    duplicate.control.key = "external/46".into();
                    state.widgets.push(duplicate);
                }
                2 => state.widgets[2].control.role = "AXStaticText".into(),
                3 => state.widgets[1].selected = Some(true),
                4 => state.widgets[0].control.identifier = "OtherListView".into(),
                _ => unreachable!(),
            }
        }
        let result = widgets::run(&backend, FINDER_REFERENCE);
        assert!(
            result.is_err() || result.unwrap().value == ScalarValue::Boolean(false),
            "cause={cause}"
        );
        assert!(backend.delivered.borrow().is_empty(), "cause={cause}");
    }
}

#[test]
fn finder_reference_stops_before_scroll_when_its_initial_position_is_wrong() {
    let backend = finder_backend();
    backend.state.borrow_mut().widgets[4].position = Some(100);
    let result = widgets::run(&backend, FINDER_REFERENCE).unwrap();
    assert_eq!(result.value, ScalarValue::Boolean(false));
    assert_eq!(
        *backend.delivered.borrow(),
        [WidgetAction::SelectItem { item: 42 }]
    );
    assert_eq!(
        backend.state.borrow().widget(45).unwrap().position,
        Some(100)
    );
}

const FINDER_OPTIONS: &str = include_str!("../examples/widgets_finder_options.lese");
const FINDER_COLUMNS: &str = include_str!("../examples/widgets_finder_columns.lese");

fn finder_options_backend() -> WindowBackend {
    let mut backend = WindowBackend::new();
    backend.menu_label = "16";
    let mut popup = widget(2, "AXPopUpButton", "Text size:");
    popup.control.value = Some("13".into());
    popup.capabilities.open_menu = true;
    let mut size = widget(3, "AXCheckBox", "");
    size.control.label = "Size".into();
    size.checked = Some(CheckState::Checked);
    size.capabilities.set_checked = true;
    let mut kind = widget(4, "AXCheckBox", "");
    kind.control.label = "Kind".into();
    kind.checked = Some(CheckState::Checked);
    kind.capabilities.set_checked = true;
    backend.state.borrow_mut().widgets = vec![popup, size, kind];
    backend
}

#[test]
fn finder_options_reference_cancels_reopens_and_observes_the_chosen_text_size() {
    let backend = finder_options_backend();
    let result = widgets::run(&backend, FINDER_OPTIONS).unwrap();
    assert_eq!(result.value, ScalarValue::Boolean(true));
    assert_eq!(result.effects, 11);
    assert_eq!(
        *backend.delivered.borrow(),
        [
            WidgetAction::OpenMenu,
            WidgetAction::CancelMenu,
            WidgetAction::OpenMenu,
            WidgetAction::ChooseMenuItem { item: 23 },
        ]
    );
    assert_eq!(
        backend
            .state
            .borrow()
            .widget(2)
            .unwrap()
            .control
            .value
            .as_deref(),
        Some("16")
    );
    assert!(backend.state.borrow().widget(23).is_err());
    assert_eq!(
        backend.state.borrow().widget(3).unwrap().checked,
        Some(CheckState::Checked)
    );
    assert_eq!(
        backend.state.borrow().widget(4).unwrap().checked,
        Some(CheckState::Checked)
    );
}

#[test]
fn finder_examples_require_a_later_observation_before_the_next_independent_phase() {
    let mut backend = finder_options_backend();
    backend.defer_menu_value = true;
    let options = widgets::run(&backend, FINDER_OPTIONS).unwrap();
    assert_eq!(options.value, ScalarValue::Boolean(false));
    assert_eq!(options.effects, 11);
    assert_eq!(backend.delivered.borrow().len(), 4);
    assert_eq!(
        backend
            .state
            .borrow()
            .widget(2)
            .unwrap()
            .control
            .value
            .as_deref(),
        Some("13")
    );

    // Acknowledged selection is not a postcondition and does not trigger retry.
    let early_columns = widgets::run(&backend, FINDER_COLUMNS).unwrap();
    assert_eq!(early_columns.value, ScalarValue::Boolean(false));
    assert_eq!(early_columns.effects, 2);
    assert_eq!(backend.delivered.borrow().len(), 4);

    // Model the application's later publication explicitly, outside both runs.
    {
        let mut state = backend.state.borrow_mut();
        state.widgets[0].control.value = Some("16".into());
        state.revision += 1;
    }
    let columns = widgets::run(&backend, FINDER_COLUMNS).unwrap();
    assert_eq!(columns.value, ScalarValue::Boolean(true));
    assert_eq!(columns.effects, 10);
    assert_eq!(
        &backend.delivered.borrow()[4..],
        [
            WidgetAction::SetChecked(false),
            WidgetAction::SetChecked(false)
        ]
    );
    assert_eq!(
        backend.state.borrow().widget(3).unwrap().checked,
        Some(CheckState::Unchecked)
    );
    assert_eq!(
        backend.state.borrow().widget(4).unwrap().checked,
        Some(CheckState::Unchecked)
    );
}

#[test]
fn finder_columns_reference_stops_when_the_first_checkbox_postcondition_fails() {
    let mut backend = finder_options_backend();
    backend.state.borrow_mut().widgets[0].control.value = Some("16".into());
    backend.unchanged_check = true;
    let result = widgets::run(&backend, FINDER_COLUMNS).unwrap();
    assert_eq!(result.value, ScalarValue::Boolean(false));
    assert_eq!(result.effects, 6);
    assert_eq!(
        *backend.delivered.borrow(),
        [WidgetAction::SetChecked(false)]
    );
    assert_eq!(
        backend.state.borrow().widget(3).unwrap().checked,
        Some(CheckState::Checked)
    );
    assert_eq!(
        backend.state.borrow().widget(4).unwrap().checked,
        Some(CheckState::Checked)
    );
}

#[test]
fn preview_panel_reference_verifies_both_edits_and_refuses_an_unexpected_initial_name() {
    let source = include_str!("../examples/widgets_preview_panel.lese");
    for initial in ["Panel Fixture", "An existing name"] {
        let backend = WindowBackend::new();
        {
            let mut state = backend.state.borrow_mut();
            let text = state
                .widgets
                .iter_mut()
                .find(|widget| widget.control.identity == 8)
                .unwrap();
            text.control.role = "AXTextField".into();
            text.control.identifier = "saveAsNameTextField".into();
            text.control.value = Some(initial.into());
        }
        let result = widgets::run(&backend, source).unwrap();
        if initial == "Panel Fixture" {
            assert_eq!(
                result.value,
                ScalarValue::String("LeseLang XPC verified".into())
            );
            assert_eq!(result.effects, 6);
            assert_eq!(
                *backend.delivered.borrow(),
                [
                    WidgetAction::SetText("LeseLang XPC draft".into()),
                    WidgetAction::SetText("LeseLang XPC verified".into()),
                ]
            );
            assert_eq!(
                backend
                    .state
                    .borrow()
                    .widget(8)
                    .unwrap()
                    .control
                    .value
                    .as_deref(),
                Some("LeseLang XPC verified")
            );
            let values: Vec<_> = result
                .observations
                .iter()
                .map(|snapshot| snapshot.widget().unwrap().control.value.as_deref().unwrap())
                .collect();
            assert_eq!(
                values,
                [
                    "Panel Fixture",
                    "LeseLang XPC draft",
                    "LeseLang XPC verified"
                ]
            );
        } else {
            assert_eq!(
                result.value,
                ScalarValue::String("initial export name mismatch".into())
            );
            assert_eq!(result.effects, 2);
            assert!(backend.delivered.borrow().is_empty());
            assert_eq!(
                backend
                    .state
                    .borrow()
                    .widget(8)
                    .unwrap()
                    .control
                    .value
                    .as_deref(),
                Some(initial)
            );
        }
    }
}
