use leselang_runtime_core::{
    OperationCatalog, OperationCatalogError, OperationCatalogLimits, OperationSchema,
};
use std::cell::Cell;

#[test]
fn discovering_exact_native_schemas_does_not_run_keys_domains_or_authorize() {
    struct Key<'a>(&'a Cell<usize>);
    impl PartialEq for Key<'_> {
        fn eq(&self, _: &Self) -> bool {
            self.0.set(self.0.get() + 1);
            true
        }
    }
    struct NativeDomain;
    let comparisons = Cell::new(0);
    let schemas = [OperationSchema {
        key: Key(&comparisons),
        parameters: &[],
        result: NativeDomain,
        required_capability: "write",
    }];
    let catalog = OperationCatalog::<_, &str, (), _, _>::new(
        7,
        &schemas,
        OperationCatalogLimits {
            max_operations: 1,
            max_parameters_per_operation: 0,
        },
    )
    .unwrap();
    assert!(std::ptr::eq(catalog.schemas(), schemas.as_slice()));
    assert_eq!(catalog.version(), 7);
    assert_eq!(catalog.schemas()[0].required_capability, "write");
    assert_eq!(comparisons.get(), 0);
    assert!(matches!(
        catalog.authorize(&Key(&comparisons), 7, &[]),
        Err(OperationCatalogError::CapabilityDenied)
    ));
    assert_eq!(comparisons.get(), 1);
}
