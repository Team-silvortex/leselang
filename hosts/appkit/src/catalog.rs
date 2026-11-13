//! One metadata builder for offline discovery and live AppKit operation domains.
use crate::Error;
use leselang_vm::native::core::discovery::{
    CatalogDescription, FieldDescription, NativeTypeDescription, OperationDescription,
    ParameterDescription, TypeDescription,
};
use leselang_vm::native::{core::*, hir::pure_typing::PureType};

const NATIVE_TYPES: [(&str, NativeType); 2] = [
    ("object", NativeType::Object),
    ("snapshot", NativeType::Snapshot),
];
const SOURCE_FIELDS: &[(&str, Field, ScalarType)] = &[
    ("identifier", Field::Identifier, ScalarType::String),
    ("role", Field::Role, ScalarType::String),
    ("value", Field::Value, ScalarType::String),
    ("editable", Field::Editable, ScalarType::Boolean),
    ("revision", Field::Revision, ScalarType::Integer),
    ("title", Field::Title, ScalarType::String),
    ("clicks", Field::Clicks, ScalarType::Integer),
    ("enabled", Field::Enabled, ScalarType::Boolean),
];

#[cfg(any(target_os = "macos", test))]
pub(crate) fn native_type(name: &str) -> Option<NativeType> {
    NATIVE_TYPES
        .iter()
        .find_map(|(key, ty)| (*key == name).then_some(*ty))
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn field_type(field: Field) -> Option<ScalarType> {
    SOURCE_FIELDS
        .iter()
        .find_map(|(_, candidate, ty)| (*candidate == field).then_some(*ty))
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn source_field(name: &str) -> Option<(Field, ScalarType)> {
    SOURCE_FIELDS
        .iter()
        .find_map(|(key, field, ty)| (*key == name).then_some((*field, *ty)))
}

pub(crate) fn result_type(kind: ResultKind) -> PureType<NativeType> {
    match kind {
        ResultKind::Identifiers => PureType::Scalar(ScalarType::StringList),
        ResultKind::Object => PureType::Result(NativeType::Object),
        ResultKind::Snapshot => PureType::Result(NativeType::Snapshot),
        ResultKind::Submitted => PureType::Scalar(ScalarType::Boolean),
    }
}

fn describe_type(ty: PureType<NativeType>) -> TypeDescription {
    match ty {
        PureType::Scalar(ty) => TypeDescription::Scalar(ty),
        PureType::Result(ty) => TypeDescription::Native(
            NATIVE_TYPES
                .iter()
                .find(|(_, candidate)| *candidate == ty)
                .expect("every native type has a source alias")
                .0
                .into(),
        ),
    }
}

pub(crate) fn describe() -> Result<CatalogDescription, Error> {
    with_catalog(
        |kind| kind,
        |catalog| CatalogDescription {
            profile: "appkit".into(),
            catalog_version: catalog.version(),
            operations: catalog
                .schemas()
                .iter()
                .map(|schema| OperationDescription {
                    name: schema.key.into(),
                    parameters: schema
                        .parameters
                        .iter()
                        .map(|parameter| ParameterDescription {
                            name: parameter.name.into(),
                            required: parameter.required,
                            types: match parameter.domain {
                                ArgumentDomain::Scalar(types) => {
                                    types.iter().map(TypeDescription::Scalar).collect()
                                }
                                ArgumentDomain::Object => {
                                    vec![describe_type(PureType::Result(NativeType::Object))]
                                }
                            },
                        })
                        .collect(),
                    result: describe_type(result_type(schema.result)),
                    required_capability: match schema.required_capability {
                        1 => "gui.read",
                        2 => "gui.write",
                        _ => unreachable!("private catalog capability"),
                    }
                    .into(),
                })
                .collect(),
            native_types: NATIVE_TYPES
                .iter()
                .map(|(name, ty)| NativeTypeDescription {
                    name: (*name).into(),
                    fields: if *ty == NativeType::Snapshot {
                        SOURCE_FIELDS
                            .iter()
                            .map(|(name, _, scalar_type)| FieldDescription {
                                name: (*name).into(),
                                scalar_type: *scalar_type,
                            })
                            .collect()
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Identifier,
    Role,
    Value,
    Editable,
    Revision,
    Title,
    Clicks,
    Enabled,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeType {
    Object,
    Snapshot,
}
#[derive(Clone, Copy)]
pub(crate) enum ResultKind {
    Identifiers,
    Object,
    Snapshot,
    Submitted,
}
#[derive(Clone, Copy)]
pub enum ArgumentDomain {
    Scalar(ScalarTypeSet),
    Object,
}
impl ScalarArgumentDomain for ArgumentDomain {
    fn scalar_types(&self) -> ScalarTypeSet {
        match self {
            Self::Scalar(value) => *value,
            Self::Object => ScalarTypeSet::empty(),
        }
    }
    fn accepts_literal(&self, value: &ScalarValue) -> bool {
        matches!(self, Self::Scalar(domain) if domain.accepts_literal(value))
    }
}

type MetadataCatalog<'s, Result> =
    OperationCatalog<'s, &'static str, &'s str, ArgumentDomain, Result, u8>;

pub(crate) fn with_catalog<Result, Output>(
    mut result_factory: impl FnMut(ResultKind) -> Result,
    use_catalog: impl FnOnce(&MetadataCatalog<'_, Result>) -> Output,
) -> std::result::Result<Output, Error> {
    let find_parameters = [NamedParameter::required(
        "name",
        ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String)),
    )];
    let observe_parameters = [NamedParameter::required("target", ArgumentDomain::Object)];
    let click_parameters = [
        NamedParameter::required("target", ArgumentDomain::Object),
        NamedParameter::required(
            "expected_revision",
            ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::Integer)),
        ),
    ];
    let set_text_parameters = [
        NamedParameter::required("target", ArgumentDomain::Object),
        NamedParameter::required(
            "expected_revision",
            ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::Integer)),
        ),
        NamedParameter::required(
            "text",
            ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String)),
        ),
    ];
    let schemas = [
        OperationSchema {
            key: "gui.list",
            parameters: &[],
            required_capability: 1,
            result: result_factory(ResultKind::Identifiers),
        },
        OperationSchema {
            key: "gui.set_text",
            parameters: &set_text_parameters,
            required_capability: 2,
            result: result_factory(ResultKind::Submitted),
        },
        OperationSchema {
            key: "gui.find",
            parameters: &find_parameters,
            required_capability: 1,
            result: result_factory(ResultKind::Object),
        },
        OperationSchema {
            key: "gui.observe",
            parameters: &observe_parameters,
            required_capability: 1,
            result: result_factory(ResultKind::Snapshot),
        },
        OperationSchema {
            key: "gui.click",
            parameters: &click_parameters,
            required_capability: 2,
            result: result_factory(ResultKind::Submitted),
        },
    ];
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 5,
            max_parameters_per_operation: 3,
        },
    )
    .map_err(|_| Error::Native("invalid AppKit operation catalog"))?;
    Ok(use_catalog(&catalog))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn description_matches_catalog_domains_and_compiler_type_mapping() {
        let description = describe().unwrap();
        with_catalog(
            |kind| (kind, Cell::new(false)),
            |catalog| {
                assert_eq!(description.catalog_version, catalog.version());
                assert_eq!(description.operations.len(), catalog.schemas().len());
                for (operation, live) in description.operations.iter().zip(catalog.schemas()) {
                    assert_eq!(operation.name, live.key);
                    assert_eq!(
                        operation.required_capability,
                        match live.required_capability {
                            1 => "gui.read",
                            2 => "gui.write",
                            _ => panic!("unknown capability"),
                        }
                    );
                    let result = match &operation.result {
                        TypeDescription::Scalar(ty) => PureType::Scalar(*ty),
                        TypeDescription::Native(name) => {
                            PureType::Result(native_type(name).unwrap())
                        }
                    };
                    assert_eq!(result, result_type(live.result.0));
                    assert_eq!(operation.parameters.len(), live.parameters.len());
                    for (described, parameter) in operation.parameters.iter().zip(live.parameters) {
                        assert_eq!(described.name, parameter.name);
                        assert_eq!(described.required, parameter.required);
                        match parameter.domain {
                            ArgumentDomain::Scalar(types) => assert_eq!(
                                described.types,
                                types
                                    .iter()
                                    .map(TypeDescription::Scalar)
                                    .collect::<Vec<_>>()
                            ),
                            ArgumentDomain::Object => assert_eq!(
                                described.types,
                                [TypeDescription::Native("object".into())]
                            ),
                        }
                    }
                }
                // Each live result factory call gets independent pending-domain state.
                catalog.schemas()[0].result.1.set(true);
                assert!(
                    catalog.schemas()[1..]
                        .iter()
                        .all(|schema| !schema.result.1.get())
                );
            },
        )
        .unwrap();
        for native in description.native_types {
            let ty = native_type(&native.name).unwrap();
            if ty == NativeType::Object {
                assert!(native.fields.is_empty());
            } else {
                assert_eq!(native.fields.len(), SOURCE_FIELDS.len());
                for field in native.fields {
                    let (native_field, scalar) = source_field(&field.name).unwrap();
                    assert_eq!(field.scalar_type, scalar);
                    assert_eq!(field_type(native_field), Some(scalar));
                }
            }
        }
        assert!(source_field("unknown").is_none());
        assert!(native_type("unknown").is_none());
    }
}
