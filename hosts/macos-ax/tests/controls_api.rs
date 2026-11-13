//! An external Rust caller can supply a backend without native AX or private APIs.
use leselang_macos_ax_host::{
    Error,
    controls::{self, ControlInfo, ControlsBackend, ScopeSnapshot},
};
use leselang_vm::native::core::ScalarValue;

struct ReadOnlyBackend;
impl ControlsBackend for ReadOnlyBackend {
    fn snapshot(&self) -> Result<ScopeSnapshot, Error> {
        Ok(ScopeSnapshot {
            identity: 1,
            revision: 1,
            controls: vec![ControlInfo {
                identity: 2,
                key: "connection/test-status".into(),
                role: "AXStaticText".into(),
                identifier: "status".into(),
                label: "Status".into(),
                value: Some("ready".into()),
                enabled: true,
                editable: false,
                pressable: false,
            }],
        })
    }
    fn check_permission(&self) -> Result<(), Error> {
        Ok(())
    }
    fn press(&self, _: &ScopeSnapshot, _: u64) -> Result<(), Error> {
        panic!("a read-only program must not press a control")
    }
    fn set_text(&self, _: &ScopeSnapshot, _: u64, _: &str) -> Result<(), Error> {
        panic!("a read-only program must not edit a control")
    }
}

#[test]
fn public_controls_api_runs_two_effects_without_native_access() {
    let backend = ReadOnlyBackend;
    let inventory = controls::discover(&backend).unwrap();
    assert_eq!(inventory.controls.len(), 1);
    let result = controls::run(
        &backend,
        r#"fn main() = bind(
            target: gui.select(role: "AXStaticText", identifier: "status", label: ""),
            body: bind(observed: gui.observe(target: target),
                body: value_or(left: field(value: observed, name: "value"), right: "missing")))"#,
    )
    .unwrap();
    assert_eq!(result.effects, 2);
    assert_eq!(result.value, ScalarValue::String("ready".into()));
    assert!(result.fuel_remaining < 10_000);
    assert_eq!(result.observations.len(), 1);
}
