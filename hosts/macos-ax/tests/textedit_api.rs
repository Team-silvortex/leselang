//! Existing TextEdit caller expressions must keep their original inference.
use leselang_macos_ax_host::{Snapshot, driver, host};
use leselang_vm::native::core::{
    LocalExecutionScope, ScalarValue, host_protocol::LocalObjectOwner,
};

#[test]
fn document_constructors_do_not_require_new_generic_annotations() {
    let scope = LocalExecutionScope::new();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&scope.lease()).unwrap();
    let _submitted = host::Reply::Submitted(true);
    let _object = host::Reply::Object(object.clone());
    let _native = host::NativeValue::Object(object);
    let _summary = driver::RunSummary {
        value: ScalarValue::None,
        effects: 0,
        fuel_remaining: 1000,
        observations: Vec::new(),
    };
    let _default_snapshot: Option<Snapshot> = None;
    let _host: Option<host::Host<'_, '_>> = None;
    let _domain: Option<host::Domain<'_>> = None;
    let _work: Option<host::Work<'_>> = None;
    let _receipt: Option<host::Receipt> = None;
    let _vm: Option<driver::Vm<'_, '_>> = None;
}

#[test]
fn document_reply_still_has_the_original_exhaustive_variants() {
    fn submitted(reply: host::Reply) -> bool {
        match reply {
            host::Reply::Object(_) => false,
            host::Reply::Snapshot(_) => false,
            host::Reply::Submitted(value) => value,
        }
    }
    assert!(submitted(host::Reply::Submitted(true)));
}
