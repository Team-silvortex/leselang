//! Catalog metadata and cold source admission. No native mutation or session
//! scheduling belongs here; reply guards remain bound to one execution.
use super::*;
use call_evaluation::PreparedValueCall;
use catalog_program::{
    CatalogProgramLimits, CompiledCatalogProgram, compile_catalog_typed_program,
};
use catalog_source::{CatalogSourceEnvironment, CatalogSourceLimits};
use control_source::ControlSourceLimits;
use flow_typing::CallFlowEnvironment;
use leselang_vm::native::core::discovery::{
    CatalogDescription, FieldDescription, NativeTypeDescription, OperationDescription,
    ParameterDescription, TypeDescription,
};
use leselang_vm::native::syntax::parse;
use pure_typing::{PureType, PureTypeEnvironment};
use source_call::{SourceCallHost, SourceCallLimits};

const CATALOG_VERSION: u32 = 1;
const READ: u8 = 1;
const WRITE: u8 = 2;

const NATIVE_TYPES: [(&str, NativeType); 2] = [
    ("object", NativeType::Object),
    ("snapshot", NativeType::Snapshot),
];
const SOURCE_FIELDS: &[(&str, Field)] = &[
    ("document", Field::Document),
    ("role", Field::Role),
    ("value", Field::Value),
    ("editable", Field::Editable),
    ("revision", Field::Revision),
    ("display", Field::Display),
    ("buttons", Field::Buttons),
    ("last_expression", Field::LastExpression),
    ("key", Field::Key),
    ("identifier", Field::Identifier),
    ("label", Field::Label),
    ("enabled", Field::Enabled),
    ("pressable", Field::Pressable),
    ("actions", Field::Actions),
    ("check_state", Field::CheckState),
    ("selected", Field::Selected),
    ("selection_available", Field::SelectionAvailable),
    ("selected_keys", Field::SelectedKeys),
    ("position", Field::Position),
    ("position_available", Field::PositionAvailable),
    ("orientation", Field::Orientation),
    ("parent_key", Field::ParentKey),
];

fn native_type(name: &str) -> Option<NativeType> {
    NATIVE_TYPES
        .iter()
        .find_map(|(key, ty)| (*key == name).then_some(*ty))
}

fn result_type(kind: ResultKind) -> PureType<NativeType> {
    match kind {
        ResultKind::Keys => PureType::Scalar(ScalarType::StringList),
        ResultKind::Object => PureType::Result(NativeType::Object),
        ResultKind::Snapshot | ResultKind::WaitSnapshot => PureType::Result(NativeType::Snapshot),
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

pub(crate) fn describe<S: ProfileSnapshot>(
    profile: &str,
    source_waits: SourceWaitCatalog,
) -> Result<CatalogDescription, Error> {
    with_metadata_catalog::<S, _, _>(
        source_waits,
        |kind| kind,
        |catalog| CatalogDescription {
            profile: profile.into(),
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
                        READ => "gui.read",
                        WRITE => "gui.write",
                        _ => unreachable!("private catalog capability"),
                    }
                    .into(),
                })
                .collect(),
            native_types: NATIVE_TYPES
                .iter()
                .map(|(name, ty)| NativeTypeDescription {
                    name: (*name).into(),
                    fields: SOURCE_FIELDS
                        .iter()
                        .filter_map(|(name, field)| {
                            if *ty != NativeType::Snapshot {
                                return None;
                            }
                            S::field_type(*field).map(|scalar_type| FieldDescription {
                                name: (*name).into(),
                                scalar_type,
                            })
                        })
                        .collect(),
                })
                .collect(),
        },
    )
}

#[cfg(test)]
pub(crate) fn assert_live_description<S: ProfileSnapshot>(
    host: &Host<'_, '_, S>,
    profile: crate::discovery::Profile,
) {
    let description = crate::discovery::describe(profile).unwrap();
    assert_eq!(description.catalog_version, host.catalog.version());
    assert_eq!(description.operations.len(), host.catalog.schemas().len());
    for (described, live) in description.operations.iter().zip(host.catalog.schemas()) {
        assert_eq!(described.name, live.key);
        assert_eq!(
            described.required_capability,
            match live.required_capability {
                1 => "gui.read",
                2 => "gui.write",
                _ => panic!("unmapped capability"),
            }
        );
        assert_eq!(described.parameters.len(), live.parameters.len());
        for (parameter, native) in described.parameters.iter().zip(live.parameters) {
            assert_eq!(parameter.name, native.name);
            assert_eq!(parameter.required, native.required);
            match native.domain {
                ArgumentDomain::Scalar(types) => assert_eq!(
                    parameter.types,
                    types
                        .iter()
                        .map(TypeDescription::Scalar)
                        .collect::<Vec<_>>()
                ),
                ArgumentDomain::Object => {
                    assert_eq!(parameter.types, [TypeDescription::Native("object".into())]);
                    assert!(host.call_native_argument_type(
                        &live.key,
                        native.name,
                        &NativeType::Object
                    ));
                }
            }
        }
        let actual = host.call_result_type(&live.key, &live.result).unwrap();
        match &described.result {
            TypeDescription::Scalar(ty) => assert_eq!(actual, PureType::Scalar(*ty)),
            TypeDescription::Native(name) => assert_eq!(
                actual,
                PureType::Result(host.source_type_name(name).unwrap())
            ),
        }
    }
    for native in &description.native_types {
        let ty = host.source_type_name(&native.name).unwrap();
        for (name, field) in SOURCE_FIELDS {
            let actual = host.source_field(&ty, name).unwrap();
            let described = native.fields.iter().find(|entry| entry.name == *name);
            match (actual, described) {
                (Some((actual_field, actual_type)), Some(described)) => {
                    assert_eq!(actual_field, *field);
                    assert_eq!(actual_type, described.scalar_type);
                    assert_eq!(host.field_type(&ty, field), Some(described.scalar_type));
                }
                (None, None) => {}
                _ => panic!("source and discovery field contracts differ: {name}"),
            }
        }
        assert!(host.source_field(&ty, "not_a_field").unwrap().is_none());
    }
    assert!(host.source_type_name("not_a_type").is_none());
}

const COMPILATION: CatalogProgramLimits = CatalogProgramLimits {
    expression: CatalogSourceLimits {
        control: ControlSourceLimits {
            source: SourceCallLimits {
                max_source_nodes: 512,
                max_source_depth: 64,
                max_lowered_nodes: 512,
                max_lowered_depth: 64,
                max_arguments: 8,
            },
            max_bindings: 16,
        },
        max_calls: 16,
    },
    max_functions: 8,
    max_parameters: 8,
    max_compilation_nodes: 1024,
    max_expansion_depth: 16,
};

pub(super) type Schema<'s, S> =
    OperationSchema<'s, &'static str, &'s str, ArgumentDomain, Domain<'s, S>, u8>;
pub(super) type Catalog<'s, S> =
    OperationCatalog<'s, &'static str, &'s str, ArgumentDomain, Domain<'s, S>, u8>;
pub(super) type Prepared<'s, S> =
    PreparedValueCall<'s, &'static str, ArgumentDomain, Domain<'s, S>, u8, ProfileNativeValue<S>>;

pub(super) fn with_catalog<S: ProfileSnapshot, T>(
    tracked: &TrackedDocument<'_, S>,
    lease: LocalExecutionLease,
    source_waits: SourceWaitCatalog,
    use_catalog: impl FnOnce(&Catalog<'_, S>) -> T,
) -> Result<T, Error> {
    with_metadata_catalog::<S, _, _>(
        source_waits,
        |kind| Domain {
            tracked,
            lease: lease.clone(),
            kind,
            expected: RefCell::new(None),
            source_wait: RefCell::new(None),
        },
        |catalog| use_catalog(catalog),
    )
}

type MetadataCatalog<'s, Result> =
    OperationCatalog<'s, &'static str, &'s str, ArgumentDomain, Result, u8>;

fn with_metadata_catalog<S: ProfileSnapshot, Result, Output>(
    source_waits: SourceWaitCatalog,
    mut result_factory: impl FnMut(ResultKind) -> Result,
    use_catalog: impl FnOnce(&MetadataCatalog<'_, Result>) -> Output,
) -> std::result::Result<Output, Error> {
    let scalar = |name, kind| {
        NamedParameter::required(name, ArgumentDomain::Scalar(ScalarTypeSet::only(kind)))
    };
    let target = || NamedParameter::required("target", ArgumentDomain::Object);
    let mutation = |payload| {
        [
            target(),
            scalar("expected_revision", ScalarType::Integer),
            payload,
        ]
    };
    let find_parameters = [scalar(
        if S::CONTROLS { "key" } else { "name" },
        ScalarType::String,
    )];
    let observe_parameters = [target()];
    let wait_parameters: Vec<_> = source_waits
        .kinds()
        .iter()
        .map(|kind| {
            [
                target(),
                scalar("expected", kind.expected_type()),
                scalar("max_attempts", ScalarType::Integer),
                scalar("interval_ms", ScalarType::Integer),
                scalar("timeout_ms", ScalarType::Integer),
            ]
        })
        .collect();
    let set_text_parameters = mutation(scalar(S::ARGUMENT, ScalarType::String));
    let list_parameters = [scalar("role", ScalarType::String)];
    let select_parameters =
        ["role", "identifier", "label"].map(|name| scalar(name, ScalarType::String));
    let select_value_parameters = ["role", "value"].map(|name| scalar(name, ScalarType::String));
    let press_parameters = [target(), scalar("expected_revision", ScalarType::Integer)];
    let checked_parameters = mutation(scalar("checked", ScalarType::Boolean));
    let item_parameters = mutation(NamedParameter::required("item", ArgumentDomain::Object));
    let scroll_parameters = mutation(scalar("position", ScalarType::Integer));
    // Each declaration gets its own expected-object cell. Catalog construction
    // shares metadata, never a pending reply guard between operations.
    let mut schema = |key, parameters, kind| OperationSchema {
        key,
        parameters,
        required_capability: if matches!(kind, ResultKind::Submitted) {
            WRITE
        } else {
            READ
        },
        result: result_factory(kind),
    };
    let mut schemas = vec![
        schema("gui.find", find_parameters.as_slice(), ResultKind::Object),
        schema(
            "gui.observe",
            observe_parameters.as_slice(),
            ResultKind::Snapshot,
        ),
        schema(
            S::OPERATION,
            set_text_parameters.as_slice(),
            ResultKind::Submitted,
        ),
    ];
    if S::CONTROLS {
        schemas.extend([
            schema("gui.list", list_parameters.as_slice(), ResultKind::Keys),
            schema(
                "gui.select",
                select_parameters.as_slice(),
                ResultKind::Object,
            ),
            schema(
                "gui.press",
                press_parameters.as_slice(),
                ResultKind::Submitted,
            ),
        ]);
    }
    if S::WIDGETS {
        schemas.push(schema(
            "gui.select_value",
            select_value_parameters.as_slice(),
            ResultKind::Object,
        ));
        for (key, parameters) in [
            ("gui.open_menu", press_parameters.as_slice()),
            ("gui.choose_menu_item", item_parameters.as_slice()),
            ("gui.cancel_menu", press_parameters.as_slice()),
            ("gui.set_checked", checked_parameters.as_slice()),
            ("gui.select_item", item_parameters.as_slice()),
            ("gui.scroll", scroll_parameters.as_slice()),
        ] {
            schemas.push(schema(key, parameters, ResultKind::Submitted));
        }
    }
    for (kind, parameters) in source_waits.kinds().iter().zip(&wait_parameters) {
        schemas.push(schema(
            kind.operation(),
            parameters.as_slice(),
            ResultKind::WaitSnapshot,
        ));
    }
    let catalog = OperationCatalog::new(
        CATALOG_VERSION,
        &schemas,
        OperationCatalogLimits {
            max_operations: if S::WIDGETS {
                13 + source_waits.kinds().len()
            } else if S::CONTROLS {
                6
            } else {
                3
            },
            max_parameters_per_operation: if source_waits != SourceWaitCatalog::None {
                5
            } else {
                3
            },
        },
    )
    .map_err(|_| Error::Invalid("invalid AX catalog"))?;
    Ok(use_catalog(&catalog))
}

impl<'s, S: ProfileSnapshot> Host<'_, 's, S> {
    fn source(&self) -> SourceCallHost<'_, 's, &'static str, ArgumentDomain, Domain<'s, S>, u8> {
        SourceCallHost {
            catalog: self.catalog,
            version: CATALOG_VERSION,
            granted: if self.granted.get() {
                &[READ, WRITE]
            } else {
                &[]
            },
        }
    }
    pub(super) fn schema(&self, operation: &'static str) -> Result<&'s Schema<'s, S>, Error> {
        self.catalog
            .authorize(&operation, CATALOG_VERSION, self.source().granted)
            .map_err(|_| Error::PermissionDenied)
    }
    pub fn compile(&self, source: &str) -> Result<Node, String> {
        let program = self.compile_task(source, "main")?;
        if !program.entry_parameters.is_empty() {
            return Err("default entry cannot have parameters; use compile_task".into());
        }
        Ok(program.node)
    }

    /// Compile a task with scalar ingress and explicitly named native helper types.
    /// This only describes source; it neither obtains objects nor invokes work.
    pub fn compile_task(
        &self,
        source: &str,
        entry: &str,
    ) -> Result<CompiledCatalogProgram<Node, NativeType>, String> {
        if source.len() > 16 * 1024 {
            return Err("source limit".into());
        }
        let parsed = parse(source);
        if !parsed.diagnostics.is_empty() {
            return Err("syntax rejected".into());
        }
        let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
        let mut limits = COMPILATION;
        if S::CONTROLS {
            limits.expression.max_calls = 32;
        }
        compile_catalog_typed_program::<Field, _, (), (), _, _, _, _>(
            &declarations,
            entry,
            limits,
            &self.source(),
            self,
        )
        .map_err(|error| error.to_string())
    }
}
impl<S: ProfileSnapshot> PureTypeEnvironment<Field, &'static str> for Host<'_, '_, S> {
    type Result = NativeType;
    fn field_type(&self, result: &NativeType, field: &Field) -> Option<ScalarType> {
        if *result != NativeType::Snapshot {
            return None;
        }
        S::field_type(*field)
    }

    fn member_result(&self, _: &NativeType, _: &str, _: &&'static str) -> Option<NativeType> {
        None
    }
}
impl<'s, S: ProfileSnapshot> CallFlowEnvironment<'s, Field, &'static str, Domain<'s, S>>
    for Host<'_, 's, S>
{
    fn call_native_argument_type(
        &self,
        operation: &&'static str,
        parameter_name: &str,
        result: &NativeType,
    ) -> bool {
        *result == NativeType::Object
            && (((*operation == "gui.observe"
                || self.source_waits.contains(operation)
                || S::is_mutation(operation))
                && parameter_name == "target")
                || (S::WIDGETS
                    && matches!(*operation, "gui.choose_menu_item" | "gui.select_item")
                    && parameter_name == "item"))
    }
    fn call_result_type(
        &self,
        _: &&'static str,
        domain: &'s Domain<'s, S>,
    ) -> Option<PureType<NativeType>> {
        Some(result_type(domain.kind))
    }
}
impl<'source, 's, S: ProfileSnapshot>
    CatalogSourceEnvironment<'source, 's, Field, &'static str, Domain<'s, S>> for Host<'_, 's, S>
{
    type Error = Error;
    fn source_type_name(&self, name: &str) -> Option<NativeType> {
        native_type(name)
    }
    fn source_operation(&self, operation: &&'static str) -> Result<&'static str, Error> {
        Ok(*operation)
    }
    fn source_field(
        &self,
        result: &NativeType,
        name: &'source str,
    ) -> Result<Option<(Field, ScalarType)>, Error> {
        if *result != NativeType::Snapshot {
            return Ok(None);
        }
        let Some((_, field)) = SOURCE_FIELDS.iter().find(|(key, _)| *key == name) else {
            return Ok(None);
        };
        Ok(S::field_type(*field).map(|kind| (*field, kind)))
    }

    fn source_member(
        &self,
        _: &NativeType,
        _: &'source str,
        _: &'source str,
    ) -> Result<Option<(&'static str, NativeType)>, Error> {
        Ok(None)
    }
}
