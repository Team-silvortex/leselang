#![forbid(unsafe_code)]
#![cfg_attr(
    not(feature = "leserpent-profile"),
    doc = r#"
The native profile does not expose the product-specialized evaluator or journals:

```compile_fail
use leselang_vm::{Vm, EffectOperation, PresentationOperation};
```
"#
)]

//! Host-neutral embedded entry points for explicitly adapted native hosts.
//!
//! Default and all-features builds remain native and in-memory. Product evaluators
//! and journals belong to their hosts, not this language workspace. Native
//! execution uses the existing HIR sessions, not a duplicate evaluator.

pub mod native;

/// Optional convenience adapter for local scalar hosts, using the native VM.
pub mod embedding;
