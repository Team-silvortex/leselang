//! Application-declared scalar operations over the existing scoped worker carrier.
use super::StartScope;
use crate::{
    Transport,
    gateway::Connection,
    task::{input_name, validate_inputs},
};
use leselang_vm::embedding::{
    Arguments, Error, Fuel, HostBuilder, HostFailure, Poll, ScalarType, ScalarValue, parameter,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    agent_worker_version: u32,
    kind: String,
    #[serde(default = "legacy_profile")]
    profile: String,
    scope: StartScope,
    source: String,
    arguments: BTreeMap<String, String>,
    operations: Vec<Declaration>,
    grants: Vec<String>,
}

fn legacy_profile() -> String {
    "javascript.operations".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    name: String,
    parameters: BTreeMap<String, String>,
    result: String,
    capability: String,
}

fn qualified_name(name: &str) -> bool {
    name.len() <= 128 && name.contains('.') && name.split('.').all(input_name)
}

// The v4 vocabulary is shared with other host languages. Rust's Serde
// representation of ScalarType can evolve independently.
const V4_SCALARS: &[(&str, ScalarType)] = &[
    ("integer", ScalarType::Integer),
    ("boolean", ScalarType::Boolean),
    ("string", ScalarType::String),
    ("none", ScalarType::None),
    ("optional_string", ScalarType::OptionalString),
    ("string_list", ScalarType::StringList),
];

fn scalar_type(name: &str) -> Result<ScalarType, String> {
    V4_SCALARS
        .iter()
        .find_map(|&(label, ty)| (label == name).then_some(ty))
        .ok_or_else(|| "unknown operation scalar type".into())
}

fn wire_type(ty: ScalarType) -> Result<&'static str, String> {
    V4_SCALARS
        .iter()
        .find_map(|&(label, scalar)| (scalar == ty).then_some(label))
        .ok_or_else(|| "unsupported v4 operation result type".into())
}

// v4 encodes integers as canonical decimal strings, including in the final
// report. No JavaScript JSON Number needs to hold an arbitrary u64.
fn encode(value: &ScalarValue) -> Value {
    match value {
        ScalarValue::Integer(n) => Value::String(n.to_string()),
        ScalarValue::Boolean(flag) => Value::Bool(*flag),
        ScalarValue::String(text) => Value::String(text.clone()),
        ScalarValue::None => Value::Null,
        ScalarValue::OptionalString(text) => json!(text),
        ScalarValue::StringList(items) => json!(items),
    }
}

fn decode(value: Value, expected: ScalarType) -> Result<ScalarValue, HostFailure> {
    let parsed = match (expected, value) {
        (ScalarType::Integer, Value::String(text)) => text
            .parse::<u64>()
            .ok()
            .filter(|n| n.to_string() == text)
            .map(ScalarValue::Integer),
        (ScalarType::Boolean, Value::Bool(flag)) => Some(ScalarValue::Boolean(flag)),
        (ScalarType::String, Value::String(text)) => Some(ScalarValue::String(text)),
        (ScalarType::None, Value::Null) => Some(ScalarValue::None),
        (ScalarType::OptionalString, value) => serde_json::from_value(value)
            .ok()
            .map(ScalarValue::OptionalString),
        (ScalarType::StringList, value) => serde_json::from_value(value)
            .ok()
            .map(ScalarValue::StringList),
        _ => None,
    };
    parsed
        .filter(ScalarValue::is_bounded)
        .ok_or_else(|| HostFailure::Uncertain("invalid operation result".into()))
}

fn invoke(
    connection: &RefCell<Connection<'_>>,
    name: &str,
    result: ScalarType,
    arguments: &Arguments,
    fuel: &mut Fuel,
) -> Result<ScalarValue, HostFailure> {
    fuel.charge(1)
        .map_err(|_| HostFailure::Failed("host fuel exhausted".into()))?;
    let arguments: Map<_, _> = arguments
        .iter()
        .map(|(key, value)| (key.to_owned(), encode(value)))
        .collect();
    let reply = connection
        .borrow_mut()
        .request(name, Value::Object(arguments))
        .map_err(|error| HostFailure::Uncertain(error.to_string()))?;
    decode(reply, result)
}

fn ensure_live(connection: &RefCell<Connection<'_>>) -> Result<(), String> {
    if connection.borrow().is_live() {
        Ok(())
    } else {
        Err("operation scope closed".into())
    }
}

pub(super) fn execute(transport: &mut dyn Transport, initial: Value) -> Result<Value, String> {
    // Serde can also deserialize named structs from positional arrays; the
    // wire contract requires records so their field names remain explicit.
    if !initial["scope"].is_object()
        || !initial["operations"]
            .as_array()
            .is_some_and(|ops| (1..=32).contains(&ops.len()) && ops.iter().all(Value::is_object))
    {
        return Err("invalid operation start record".into());
    }
    let start: Start =
        serde_json::from_value(initial).map_err(|_| "invalid operation start record")?;
    if start.agent_worker_version != 4 || start.kind != "start" {
        return Err("unsupported operation start record".into());
    }
    if !matches!(
        start.profile.as_str(),
        "javascript.operations" | "lese.operations"
    ) {
        return Err("unsupported operation profile".into());
    }
    let scope = start.scope.admit()?;
    let arguments: Vec<_> = start.arguments.into_iter().collect();
    validate_inputs(&start.source, &arguments, 8)?;
    let grants: Vec<_> = start.grants.iter().map(String::as_str).collect();
    if grants.len() > start.operations.len()
        || grants.iter().copied().collect::<BTreeSet<_>>().len() != grants.len()
        || grants
            .iter()
            .any(|grant| !start.operations.iter().any(|op| &op.capability == grant))
    {
        return Err("invalid operation grants".into());
    }
    let connection = RefCell::new(Connection::new(transport, scope));
    let mut builder = HostBuilder::new();
    for op in &start.operations {
        if !qualified_name(&op.name)
            || !qualified_name(&op.capability)
            || op.parameters.len() > 8
            || !op.parameters.keys().all(|name| input_name(name))
        {
            return Err("invalid operation declaration".into());
        }
        let result = scalar_type(&op.result)?;
        let parameters = op
            .parameters
            .iter()
            .map(|(key, ty)| Ok(parameter(key, scalar_type(ty)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let connection = &connection;
        builder
            .register(
                &op.name,
                parameters,
                result,
                &op.capability,
                move |arguments, fuel| invoke(connection, &op.name, result, arguments, fuel),
            )
            .map_err(|error| error.to_string())?;
    }
    let host = builder.build();
    let program = host
        .compile_task(&start.source, "main", &grants)
        .map_err(|error| error.to_string())?;
    let arguments = arguments
        .into_iter()
        .map(|(key, value)| (key, ScalarValue::String(value)))
        .collect();
    let mut session = host
        .start_with_arguments(&program, arguments, &grants, Fuel::new(10_000))
        .map_err(|error| error.to_string())?;
    let mut effects = 0;
    loop {
        ensure_live(&connection)?;
        match session.poll().map_err(|error| error.to_string())? {
            Poll::Request(request) => {
                let authorized = session
                    .authorize(request)
                    .map_err(|error| error.to_string())?;
                let receipt = session
                    .dispatch(authorized)
                    .map_err(|error| match error.error {
                        Error::Host(HostFailure::Failed(text) | HostFailure::Uncertain(text)) => {
                            text
                        }
                        other => other.to_string(),
                    })?;
                ensure_live(&connection)?;
                session.accept(receipt).map_err(|error| error.to_string())?;
                effects += 1;
            }
            Poll::Value(value) => {
                ensure_live(&connection)?;
                return Ok(
                    json!({"schema_version":1,"kind":"operation_task_run","profile":start.profile,
                    "result":{"type":wire_type(value.scalar_type())?,"value":encode(&value)},
                    "effects":effects,"fuel_remaining":session.fuel_remaining()}),
                );
            }
            Poll::Awaiting | Poll::Terminal(_) => {
                return Err("operation task stopped before completion".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo {
        calls: Vec<Value>,
        reply_override: Option<Value>,
        live: bool,
        retire_during_exchange: bool,
    }
    impl Echo {
        fn open() -> Self {
            Self {
                calls: Vec::new(),
                reply_override: None,
                live: true,
                retire_during_exchange: false,
            }
        }
    }
    impl Transport for Echo {
        fn exchange(&mut self, request: Value) -> Result<Value, String> {
            let value = self
                .reply_override
                .clone()
                .unwrap_or_else(|| request["arguments"]["value"].clone());
            let reply = json!({"transport_version":1,"window":"consumer","epoch":"boot","session":"task",
                "request_id":request["request_id"],"status":"ok","error":null,"value":value});
            self.calls.push(request);
            if self.retire_during_exchange {
                self.live = false;
            }
            Ok(reply)
        }
        fn is_live(&self) -> bool {
            self.live
        }
    }
    fn start() -> Value {
        json!({"agent_worker_version":4,"kind":"start","scope":{"window":"consumer","epoch":"boot","session":"task"},
            "source":"fn main() = bind(n: tally.echo(value: 18446744073709551615), body: tally.echo(value: n))",
            "arguments":{},"operations":[{"name":"tally.echo","parameters":{"value":"integer"},"result":"integer","capability":"log.read"}],"grants":["log.read"]})
    }

    #[test]
    fn two_effects_roundtrip_exact_u64_max_using_declared_names_and_types() {
        let mut transport = Echo::open();
        let report = execute(&mut transport, start()).unwrap();
        assert_eq!(report["effects"], 2);
        assert_eq!(
            report["result"],
            json!({"type":"integer","value":u64::MAX.to_string()})
        );
        assert!(report["fuel_remaining"].as_u64().unwrap() < 10_000);
        assert_eq!(transport.calls.len(), 2);
        for (i, call) in transport.calls.iter().enumerate() {
            assert_eq!(call["operation"], "tally.echo");
            assert_eq!(call["request_id"], i + 1);
            assert_eq!(call["arguments"], json!({"value":u64::MAX.to_string()}));
        }
    }

    #[test]
    fn v4_scalar_labels_roundtrip_from_declarations_through_effects_to_reports() {
        for (label, value) in [
            ("integer", json!("18446744073709551615")),
            ("boolean", json!(false)),
            ("string", json!("线")),
            ("none", Value::Null),
            ("optional_string", Value::Null),
            ("optional_string", json!("")),
            ("string_list", json!(["", "线"])),
        ] {
            let mut start = start();
            start["profile"] = json!("lese.operations");
            start["source"] =
                json!("fn main() = bind(v: tally.first(), body: tally.echo(value: v))");
            start["operations"] = json!([
                {"name":"tally.first","parameters":{},"result":label,"capability":"log.read"},
                {"name":"tally.echo","parameters":{"value":label},"result":label,"capability":"log.read"}
            ]);
            let mut echo = Echo::open();
            echo.reply_override = Some(value.clone());
            let report = execute(&mut echo, start).unwrap();
            assert_eq!(echo.calls.len(), 2);
            assert_eq!(echo.calls[1]["arguments"], json!({"value":value}));
            assert_eq!(report["effects"], 2);
            assert_eq!(report["result"], json!({"type":label,"value":value}));
        }
    }

    #[test]
    fn explicit_profiles_preserve_legacy_execution_and_metering() {
        let mut legacy = Echo::open();
        let baseline = execute(&mut legacy, start()).unwrap();
        assert_eq!(baseline["profile"], "javascript.operations");
        for profile in ["javascript.operations", "lese.operations"] {
            let mut transport = Echo::open();
            let mut initial = start();
            initial["profile"] = json!(profile);
            let mut expected = baseline.clone();
            expected["profile"] = json!(profile);
            assert_eq!(execute(&mut transport, initial).unwrap(), expected);
            assert_eq!(transport.calls, legacy.calls);
        }
    }

    #[test]
    fn admits_declarations_grants_entry_arguments_and_all_source_before_any_request() {
        for (field, value) in [
            ("profile", json!("unrecognized.operations")),
            ("profile", Value::Null),
            ("profile", json!({"lese.operations":null})),
            ("grants", json!([])),
            ("grants", json!(["log.read", "log.read"])),
            ("grants", json!(["unknown.read"])),
            ("source", json!("fn main() = tally.echo(value: true)")),
            ("arguments", json!({"unknown":"x"})),
            ("scope", json!(["consumer", "boot", "task"])),
            (
                "scope",
                json!({"window":"foreign.window","epoch":"boot","session":"task"}),
            ),
            ("operations", json!([])),
            (
                "operations",
                json!([start()["operations"][0], start()["operations"][0]]),
            ),
            (
                "operations",
                json!([["tally.echo",{"value":"integer"},"integer","log.read"]]),
            ),
            ("input_mode", json!("named_strings")),
        ] {
            let mut initial = start();
            initial[field] = value;
            let mut echo = Echo::open();
            assert!(execute(&mut echo, initial).is_err(), "invalid {field}");
            assert!(echo.calls.is_empty());
        }
        for (field, value) in [
            ("parameters", json!({"expr":"record"})),
            ("result", json!("float")),
            ("parameters", json!({"body":"none"})),
            ("name", json!("tally")),
            ("capability", json!("invalid cap")),
            ("result", json!({"integer":null})),
            ("extra", json!(true)),
        ] {
            let mut initial = start();
            initial["source"] = json!("fn main() = true");
            initial["grants"] = json!([]);
            initial["operations"][0][field] = value;
            let mut echo = Echo::open();
            assert!(execute(&mut echo, initial).is_err(), "invalid {field}");
            assert!(echo.calls.is_empty());
        }
    }

    #[test]
    fn malformed_or_late_reply_stops_before_next_effect() {
        for bad in [
            json!(12),
            json!("-1"),
            json!("01"),
            json!("+1"),
            json!("18446744073709551616"),
            Value::Null,
        ] {
            let mut echo = Echo::open();
            echo.reply_override = Some(bad);
            assert_eq!(
                execute(&mut echo, start()).unwrap_err(),
                "invalid operation result"
            );
            assert_eq!(echo.calls.len(), 1);
        }
        let mut echo = Echo::open();
        echo.retire_during_exchange = true;
        assert!(execute(&mut echo, start()).is_err());
        assert_eq!(echo.calls.len(), 1);
    }

    #[test]
    fn failure_after_an_accepted_effect_never_replays_or_runs_the_following_operation() {
        struct FailOnSecond {
            echo: Echo,
            fields: Map<String, Value>,
        }
        impl Transport for FailOnSecond {
            fn exchange(&mut self, request: Value) -> Result<Value, String> {
                let mut reply = self.echo.exchange(request)?;
                if self.echo.calls.len() == 2 {
                    reply.as_object_mut().unwrap().extend(self.fields.clone());
                }
                Ok(reply)
            }
            fn is_live(&self) -> bool {
                true
            }
        }
        for fields in [
            json!({"status":"rejected","value":null,"error":"permission_denied"}),
            json!({"status":"uncertain","value":null,"error":"handler_failed"}),
            json!({"status":"rejected","value":"1","error":"permission_denied"}),
            json!({"status":"ok","error":"foreign"}),
            json!({"request_id":1}),
            json!({"extra":true}),
        ] {
            let mut transport = FailOnSecond {
                echo: Echo::open(),
                fields: fields.as_object().unwrap().clone(),
            };
            let mut initial = start();
            initial["source"] = json!(
                "fn main() = bind(a: tally.echo(value: 1), body: bind(b: tally.echo(value: a), body: tally.echo(value: b)))"
            );
            assert!(execute(&mut transport, initial).is_err());
            assert_eq!(transport.echo.calls.len(), 2);
        }
    }

    #[test]
    fn schema_controls_nullable_and_bounded_list_result_decoding() {
        for (ty, value) in [
            (ScalarType::None, Value::Null),
            (ScalarType::OptionalString, Value::Null),
            (ScalarType::OptionalString, json!("")),
            (ScalarType::StringList, json!(["", "界"])),
            (ScalarType::Boolean, json!(false)),
            (ScalarType::String, json!("a\0b")),
        ] {
            let scalar = decode(value.clone(), ty).unwrap();
            assert_eq!(scalar.scalar_type(), ty);
            assert_eq!(encode(&scalar), value);
        }
        for (ty, value) in [
            (ScalarType::None, json!("")),
            (ScalarType::OptionalString, json!(false)),
            (ScalarType::StringList, json!(vec![""; 65])),
            (ScalarType::StringList, json!(["x".repeat(4096), "y"])),
            (ScalarType::OptionalString, json!("界".repeat(1366))),
        ] {
            assert!(decode(value, ty).is_err());
        }
    }
}
