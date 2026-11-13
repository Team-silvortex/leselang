//! Offline catalog metadata. No backend, permission query or session is created.
use crate::{
    Error, Snapshot,
    calculator::CalculatorSnapshot,
    controls::ControlSnapshot,
    host::{SourceWaitCatalog, catalog},
    widgets::WidgetSnapshot,
};
use leselang_vm::native::core::discovery::CatalogDescription;
#[path = "../../reference_semantics.rs"]
mod reference_semantics;

/// Explicit catalog choices; selecting metadata grants no access to an application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    TextEdit,
    Calculator,
    Controls,
    Widgets,
    WidgetsWithValueWaits,
    WidgetsWithStateWaits,
}

pub fn describe(profile: Profile) -> Result<CatalogDescription, Error> {
    match profile {
        Profile::TextEdit => {
            catalog::describe::<Snapshot>("macos_ax.textedit", SourceWaitCatalog::None)
        }
        Profile::Calculator => {
            catalog::describe::<CalculatorSnapshot>("macos_ax.calculator", SourceWaitCatalog::None)
        }
        Profile::Controls => {
            catalog::describe::<ControlSnapshot>("macos_ax.controls", SourceWaitCatalog::None)
        }
        Profile::Widgets => {
            catalog::describe::<WidgetSnapshot>("macos_ax.widgets", SourceWaitCatalog::None)
        }
        Profile::WidgetsWithValueWaits => catalog::describe::<WidgetSnapshot>(
            "macos_ax.widgets.wait_values",
            SourceWaitCatalog::Values,
        ),
        Profile::WidgetsWithStateWaits => catalog::describe::<WidgetSnapshot>(
            "macos_ax.widgets.wait_states",
            SourceWaitCatalog::States,
        ),
    }
}

/// Describe structural capabilities plus supported host-owned task semantics.
/// Unsupported reference tasks are absent; metadata never queries live AX state.
pub fn describe_json(profile: Profile) -> Result<String, String> {
    let description = describe(profile).map_err(|error| error.to_string())?;
    match reference_semantics::describe(&description.profile) {
        Some(semantics) => description.to_json_with_semantics(&semantics),
        None => description.to_json(),
    }
    .map_err(|error| error.to_string())
}
