//! Bounded, versioned descriptions of host catalogs for editors and agents.
//!
//! These owned, unchecked descriptions contain metadata only. Hosts project them
//! from their actual registration and type declarations; successful validation
//! does not attest that a host or object exists, that a schema is compatible, or
//! that any capability has been granted. No object reference, execution request,
//! reply domain, session, transport or scheduler is serialized here.
//!
//! [`CatalogDescription::to_json`] validates the complete description and returns
//! one JSON object followed by LF, with at most 256 KiB of encoded UTF-8 output.
//! A failure returns no partial output. Hosts still bound the allocation and work
//! used to construct their descriptions; these limits do not bound prior ingress.
//! There is no catalog deserializer or mechanism to turn metadata into runtime
//! authority. Optional [`semantics`] records have an unchecked metadata codec;
//! they must be validated against a catalog before a consumer uses their claims.

use std::fmt;

use crate::{ScalarType, host_protocol::ProtocolVersion};

pub mod semantics;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_DESCRIPTION_BYTES: usize = 256 * 1024;
pub const MAX_OPERATIONS: usize = 64;
pub const MAX_PARAMETERS_PER_OPERATION: usize = 64;
pub const MAX_NATIVE_TYPES: usize = 32;
pub const MAX_FIELDS_PER_NATIVE_TYPE: usize = 64;
pub const MAX_PARAMETER_TYPES: usize = 6 + MAX_NATIVE_TYPES;

/// An owned catalog projection, not registration or permission to execute.
/// Native type names are the exact helper parameter aliases supported by this
/// profile. Catalog versions are independent of this description schema and the
/// local host protocol version.
///
/// ```compile_fail
/// use leselang_runtime_core::discovery::CatalogDescription;
/// let description: CatalogDescription = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogDescription {
    pub profile: String,
    pub catalog_version: u32,
    pub operations: Vec<OperationDescription>,
    pub native_types: Vec<NativeTypeDescription>,
}

/// A declaration-ordered operation and its original required capability label.
/// The capability is descriptive policy metadata, never a grant or live verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDescription {
    pub name: String,
    pub parameters: Vec<ParameterDescription>,
    pub result: TypeDescription,
    pub required_capability: String,
}

/// Accepted alternatives for one original named parameter, in supplied order.
/// An empty `types` list is an explicit deny-all domain: no supplied value can
/// satisfy it. It does not imply a default, wildcard or implicit optionality.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterDescription {
    pub name: String,
    pub required: bool,
    pub types: Vec<TypeDescription>,
}

/// A scalar language type or the exact name of a declared native helper alias.
/// Native references remain native; their names are not scalar object handles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeDescription {
    Scalar(ScalarType),
    Native(String),
}

/// A host-declared helper type alias and its exported scalar field names.
/// Empty fields are valid, for example for an opaque `object` reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeTypeDescription {
    pub name: String,
    pub fields: Vec<FieldDescription>,
}

/// A field accepted by the host's literal-name projection for this native type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldDescription {
    pub name: String,
    pub scalar_type: ScalarType,
}

/// Payload-free rejection of an invalid or oversized metadata projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DescriptionError {
    InvalidCatalogVersion,
    InvalidProfile,
    OperationLimit,
    ParameterLimit,
    NativeTypeLimit,
    FieldLimit,
    TypeLimit,
    InvalidOperationName,
    InvalidParameterName,
    InvalidNativeTypeName,
    InvalidFieldName,
    InvalidCapability,
    DuplicateOperation,
    DuplicateParameter,
    DuplicateNativeType,
    DuplicateField,
    DuplicateParameterType,
    UndeclaredNativeType,
    OutputLimit,
    InvalidSemantics,
    SemanticsLimit,
}

impl fmt::Display for DescriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCatalogVersion => "catalog description version must be nonzero",
            Self::InvalidProfile => "catalog description profile identifier is invalid",
            Self::OperationLimit => "catalog description has too many operations",
            Self::ParameterLimit => "operation description has too many parameters",
            Self::NativeTypeLimit => "catalog description has too many native types",
            Self::FieldLimit => "native type description has too many fields",
            Self::TypeLimit => "parameter description has too many type alternatives",
            Self::InvalidOperationName => "operation description name is invalid",
            Self::InvalidParameterName => "parameter description name is invalid",
            Self::InvalidNativeTypeName => "native helper type alias is invalid or reserved",
            Self::InvalidFieldName => "field description name is empty or exceeds its byte limit",
            Self::InvalidCapability => "capability description is empty or exceeds its byte limit",
            Self::DuplicateOperation => "catalog description repeats an operation name",
            Self::DuplicateParameter => "operation description repeats a parameter name",
            Self::DuplicateNativeType => "catalog description repeats a native type alias",
            Self::DuplicateField => "native type description repeats a field name",
            Self::DuplicateParameterType => "parameter description repeats a type alternative",
            Self::UndeclaredNativeType => {
                "operation description refers to an undeclared native type"
            }
            Self::OutputLimit => "encoded catalog description exceeds 256 KiB",
            Self::InvalidSemantics => {
                "catalog semantics are invalid or incompatible with the catalog"
            }
            Self::SemanticsLimit => "catalog semantics exceed a declared bound",
        })
    }
}

impl std::error::Error for DescriptionError {}

fn identifier(name: &str, limit: usize) -> bool {
    let mut bytes = name.bytes();
    name.len() <= limit
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !matches!(name, "fn" | "true" | "false" | "none")
}

fn scalar_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::Integer => "integer",
        ScalarType::Boolean => "boolean",
        ScalarType::String => "string",
        ScalarType::None => "none",
        ScalarType::OptionalString => "optional_string",
        ScalarType::StringList => "string_list",
    }
}

impl CatalogDescription {
    /// Add optional, unchecked host claims after validating their references to
    /// this catalog. This does not grant access or attest application behavior.
    /// The legacy [`Self::to_json`] format and Rust description structs are unchanged.
    pub fn to_json_with_semantics(
        &self,
        semantics: &semantics::SemanticDescription,
    ) -> Result<String, DescriptionError> {
        semantics.validate(self)?;
        let mut base = self.to_json()?;
        base.truncate(base.len() - 2); // Replace only the closing object and LF.
        let mut output = Encoder(base);
        output.push(",\"semantics\":")?;
        semantics.encode(&mut output)?;
        output.push("}\n")?;
        Ok(output.0)
    }

    /// Emit schema-version 1 metadata in declaration order, with one trailing LF.
    ///
    /// Operation names use dot-separated ASCII identifiers, at most 128 bytes in
    /// total. Parameter labels use one such identifier of at most 64 bytes; `body`
    /// remains valid in both, as in scalar host registration. Native helper aliases
    /// use at most 64 bytes and additionally reject `body` and all six scalar names.
    /// Profiles are 1–128 ASCII letters/digits or `_`, `-`, `.`. Field names and
    /// capability labels are 1–128 UTF-8 bytes; their spelling is preserved and
    /// escaped for JSON. Names are compared exactly, without normalization.
    ///
    /// Count ceilings are inclusive and checked before traversing each collection.
    /// Every native reference must have a declared helper alias. Empty catalogs,
    /// field lists and deny-all parameter type lists are valid. The returned data
    /// does not contain live grants, resource identities or execution authority.
    pub fn to_json(&self) -> Result<String, DescriptionError> {
        self.validate()?;
        let mut output = Encoder(String::new());
        output.push("{\"schema_version\":")?;
        output.push(&SCHEMA_VERSION.to_string())?;
        output.push(",\"protocol_version\":")?;
        output.push(&ProtocolVersion::V1.number().to_string())?;
        output.push(",\"catalog_version\":")?;
        output.push(&self.catalog_version.to_string())?;
        output.push(",\"profile\":")?;
        output.string(&self.profile)?;
        output.push(",\"operations\":[")?;
        for (index, operation) in self.operations.iter().enumerate() {
            if index != 0 {
                output.push(",")?;
            }
            output.push("{\"name\":")?;
            output.string(&operation.name)?;
            output.push(",\"parameters\":[")?;
            for (index, parameter) in operation.parameters.iter().enumerate() {
                if index != 0 {
                    output.push(",")?;
                }
                output.push("{\"name\":")?;
                output.string(&parameter.name)?;
                output.push(",\"required\":")?;
                output.push(if parameter.required { "true" } else { "false" })?;
                output.push(",\"types\":[")?;
                for (index, ty) in parameter.types.iter().enumerate() {
                    if index != 0 {
                        output.push(",")?;
                    }
                    output.ty(ty)?;
                }
                output.push("]}")?;
            }
            output.push("],\"result\":")?;
            output.ty(&operation.result)?;
            output.push(",\"required_capability\":")?;
            output.string(&operation.required_capability)?;
            output.push("}")?;
        }
        output.push("],\"native_types\":[")?;
        for (index, native) in self.native_types.iter().enumerate() {
            if index != 0 {
                output.push(",")?;
            }
            output.push("{\"name\":")?;
            output.string(&native.name)?;
            output.push(",\"fields\":[")?;
            for (index, field) in native.fields.iter().enumerate() {
                if index != 0 {
                    output.push(",")?;
                }
                output.push("{\"name\":")?;
                output.string(&field.name)?;
                output.push(",\"scalar_type\":")?;
                output.string(scalar_name(field.scalar_type))?;
                output.push("}")?;
            }
            output.push("]}")?;
        }
        output.push("]}\n")?;
        Ok(output.0)
    }

    fn validate(&self) -> Result<(), DescriptionError> {
        if self.catalog_version == 0 {
            return Err(DescriptionError::InvalidCatalogVersion);
        }
        if self.profile.is_empty()
            || self.profile.len() > 128
            || !self
                .profile
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(DescriptionError::InvalidProfile);
        }
        if self.operations.len() > MAX_OPERATIONS {
            return Err(DescriptionError::OperationLimit);
        }
        if self.native_types.len() > MAX_NATIVE_TYPES {
            return Err(DescriptionError::NativeTypeLimit);
        }
        for (index, native) in self.native_types.iter().enumerate() {
            if !identifier(&native.name, 64)
                || matches!(
                    native.name.as_str(),
                    "body" | "integer" | "boolean" | "string" | "optional_string" | "string_list"
                )
            {
                return Err(DescriptionError::InvalidNativeTypeName);
            }
            if self.native_types[..index]
                .iter()
                .any(|prior| prior.name == native.name)
            {
                return Err(DescriptionError::DuplicateNativeType);
            }
            if native.fields.len() > MAX_FIELDS_PER_NATIVE_TYPE {
                return Err(DescriptionError::FieldLimit);
            }
            for (index, field) in native.fields.iter().enumerate() {
                if field.name.is_empty() || field.name.len() > 128 {
                    return Err(DescriptionError::InvalidFieldName);
                }
                if native.fields[..index]
                    .iter()
                    .any(|prior| prior.name == field.name)
                {
                    return Err(DescriptionError::DuplicateField);
                }
            }
        }
        for (index, operation) in self.operations.iter().enumerate() {
            if operation.name.len() > 128
                || !operation
                    .name
                    .split('.')
                    .all(|segment| identifier(segment, 128))
            {
                return Err(DescriptionError::InvalidOperationName);
            }
            if self.operations[..index]
                .iter()
                .any(|prior| prior.name == operation.name)
            {
                return Err(DescriptionError::DuplicateOperation);
            }
            if operation.required_capability.is_empty() || operation.required_capability.len() > 128
            {
                return Err(DescriptionError::InvalidCapability);
            }
            if operation.parameters.len() > MAX_PARAMETERS_PER_OPERATION {
                return Err(DescriptionError::ParameterLimit);
            }
            self.validate_type(&operation.result)?;
            for (index, parameter) in operation.parameters.iter().enumerate() {
                if !identifier(&parameter.name, 64) {
                    return Err(DescriptionError::InvalidParameterName);
                }
                if operation.parameters[..index]
                    .iter()
                    .any(|prior| prior.name == parameter.name)
                {
                    return Err(DescriptionError::DuplicateParameter);
                }
                if parameter.types.len() > MAX_PARAMETER_TYPES {
                    return Err(DescriptionError::TypeLimit);
                }
                for (index, ty) in parameter.types.iter().enumerate() {
                    self.validate_type(ty)?;
                    if parameter.types[..index].contains(ty) {
                        return Err(DescriptionError::DuplicateParameterType);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_type(&self, ty: &TypeDescription) -> Result<(), DescriptionError> {
        if let TypeDescription::Native(name) = ty
            && !self.native_types.iter().any(|native| native.name == *name)
        {
            return Err(DescriptionError::UndeclaredNativeType);
        }
        Ok(())
    }
}

struct Encoder(String);
impl Encoder {
    fn push(&mut self, text: &str) -> Result<(), DescriptionError> {
        if self
            .0
            .len()
            .checked_add(text.len())
            .is_none_or(|length| length > MAX_DESCRIPTION_BYTES)
        {
            return Err(DescriptionError::OutputLimit);
        }
        self.0.push_str(text);
        Ok(())
    }

    fn string(&mut self, text: &str) -> Result<(), DescriptionError> {
        self.push("\"")?;
        for character in text.chars() {
            match character {
                '"' => self.push("\\\"")?,
                '\\' => self.push("\\\\")?,
                character
                    if character.is_control()
                        || matches!(character, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
                {
                    self.push(&format!("\\u{:04x}", character as u32))?;
                }
                character => self.push(character.encode_utf8(&mut [0; 4]))?,
            }
        }
        self.push("\"")
    }

    fn ty(&mut self, ty: &TypeDescription) -> Result<(), DescriptionError> {
        let (kind, name) = match ty {
            TypeDescription::Scalar(ty) => ("scalar", scalar_name(*ty)),
            TypeDescription::Native(name) => ("native", name.as_str()),
        };
        self.push("{\"kind\":")?;
        self.string(kind)?;
        self.push(",\"name\":")?;
        self.string(name)?;
        self.push("}")
    }
}
