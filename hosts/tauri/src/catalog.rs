//! Tauri application identities projected from the shared registered catalog.
use crate::Error;
use leselang_vm::native::core::discovery::CatalogDescription;

pub(crate) fn describe() -> Result<CatalogDescription, Error> {
    leselang_registered_host::describe_catalog("tauri.typescript")
}

pub(crate) fn describe_board() -> Result<CatalogDescription, Error> {
    leselang_registered_host::describe_catalog("tauri.taskboard")
}

pub(crate) fn describe_article() -> Result<CatalogDescription, Error> {
    leselang_registered_host::describe_catalog("tauri.article")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describe_registered() -> Result<CatalogDescription, Error> {
        leselang_registered_host::describe_catalog("javascript.registered")
    }

    #[test]
    fn registered_profile_describes_the_same_live_operations_without_form_identity() {
        let form = describe().unwrap();
        let registered = describe_registered().unwrap();
        assert_eq!(form.profile, "tauri.typescript");
        assert_eq!(registered.profile, "javascript.registered");
        assert_eq!(registered.catalog_version, form.catalog_version);
        assert_eq!(registered.operations, form.operations);
        assert_eq!(registered.native_types, form.native_types);
        let mut form_json: serde_json::Value =
            serde_json::from_str(&crate::describe().unwrap()).unwrap();
        assert_eq!(
            crate::describe().unwrap().as_bytes(),
            include_bytes!("../../../fixtures/host-semantics/tauri-semantics-v1.json")
        );
        let mut registered_json: serde_json::Value =
            serde_json::from_str(&crate::describe_registered().unwrap()).unwrap();
        registered_json["profile"] = serde_json::json!("tauri.typescript");
        assert!(registered_json.get("semantics").is_none());
        let semantics = form_json
            .as_object_mut()
            .unwrap()
            .remove("semantics")
            .unwrap();
        assert_eq!(semantics["version"], 1);
        assert_eq!(semantics["tasks"][0]["id"], "fill_and_save");
        assert_eq!(semantics["tasks"][0]["execution"], "text");
        assert_eq!(semantics["tasks"][0]["verification"]["max_fuel"], 10_000);
        assert_eq!(semantics["operations"].as_array().unwrap().len(), 5);
        assert_eq!(registered_json, form_json);
    }
}
