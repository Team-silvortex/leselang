#![forbid(unsafe_code)]
#![cfg_attr(
    not(feature = "leserpent-profile"),
    doc = r#"
The native profile does not export product operations or legacy entry points:

```compile_fail
use leselang_hir::{Effect, lower};
```

```compile_fail
use leselang_hir::host_call::HostOperation;
```

```compile_fail
use leselang_hir::computation::Computation;
```

```compile_fail
use leselang_hir::result_field::ResultField;
```
"#
)]

//! Host-defined compilation, typed control IR and bounded in-memory execution.
//!
//! This independent crate exposes native compilation and session types only.
//! Hosts own operation catalogs, authority, dispatch and native work limits.

pub use leselang_hir_core::*;
