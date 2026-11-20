//! Decode catalog wire records into validated, authority-free descriptions.
use leselang_runtime_core::{
    ScalarType,
    discovery::{
        CatalogDescription, FieldDescription, NativeTypeDescription, OperationDescription,
        ParameterDescription, TypeDescription,
    },
};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Catalog {
    schema_version: u32,
    protocol_version: u32,
    catalog_version: u32,
    profile: String,
    operations: Vec<Operation>,
    native_types: Vec<NativeType>,
}
#[derive(Deserialize)]
struct Operation {
    name: String,
    parameters: Vec<Parameter>,
    result: Type,
    required_capability: String,
}
#[derive(Deserialize)]
struct Parameter {
    name: String,
    required: bool,
    types: Vec<Type>,
}
#[derive(Deserialize)]
#[serde(tag = "kind")]
enum Type {
    #[serde(rename = "scalar")]
    Scalar { name: String },
    #[serde(rename = "native")]
    Native { name: String },
}
#[derive(Deserialize)]
struct NativeType {
    name: String,
    fields: Vec<Field>,
}
#[derive(Deserialize)]
struct Field {
    name: String,
    scalar_type: String,
}

fn scalar(name: &str) -> Result<ScalarType, String> {
    match name {
        "integer" => Ok(ScalarType::Integer),
        "boolean" => Ok(ScalarType::Boolean),
        "string" => Ok(ScalarType::String),
        "none" => Ok(ScalarType::None),
        "optional_string" => Ok(ScalarType::OptionalString),
        "string_list" => Ok(ScalarType::StringList),
        _ => Err("catalog uses an unsupported scalar type".into()),
    }
}
impl Type {
    fn into_description(self) -> Result<TypeDescription, String> {
        match self {
            Self::Scalar { name } => scalar(&name).map(TypeDescription::Scalar),
            Self::Native { name } => Ok(TypeDescription::Native(name)),
        }
    }
}
impl Catalog {
    pub(super) fn into_description(self) -> Result<CatalogDescription, String> {
        if self.catalog_version != 1 {
            return Err(
                "this strategy requires catalog schema, protocol and catalog version 1".into(),
            );
        }
        self.into_semantic_description()
    }

    pub(super) fn into_semantic_description(self) -> Result<CatalogDescription, String> {
        if self.schema_version != 1 || self.protocol_version != 1 {
            return Err("this strategy requires catalog schema and protocol version 1".into());
        }
        let description = CatalogDescription {
            profile: self.profile,
            catalog_version: self.catalog_version,
            operations: self
                .operations
                .into_iter()
                .map(|operation| {
                    Ok(OperationDescription {
                        name: operation.name,
                        parameters: operation
                            .parameters
                            .into_iter()
                            .map(|parameter| {
                                Ok(ParameterDescription {
                                    name: parameter.name,
                                    required: parameter.required,
                                    types: parameter
                                        .types
                                        .into_iter()
                                        .map(Type::into_description)
                                        .collect::<Result<_, String>>()?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                        result: operation.result.into_description()?,
                        required_capability: operation.required_capability,
                    })
                })
                .collect::<Result<_, String>>()?,
            native_types: self
                .native_types
                .into_iter()
                .map(|native| {
                    Ok(NativeTypeDescription {
                        name: native.name,
                        fields: native
                            .fields
                            .into_iter()
                            .map(|field| {
                                Ok(FieldDescription {
                                    name: field.name,
                                    scalar_type: scalar(&field.scalar_type)?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        // The metadata codec is only a validator here. This does not deserialize
        // an OperationCatalog, ObjectRef or anything granting execution authority.
        description
            .to_json()
            .map_err(|error| format!("invalid catalog metadata: {error}"))?;
        Ok(description)
    }
}
