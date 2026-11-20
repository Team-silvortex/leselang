use leselang_registered_host::{
    Scope, Transport, prepare_named_task, prepare_task, run_prepared_registered_task,
};
use serde_json::{Value, json};

struct EmptyRegistry {
    requests: usize,
    live: bool,
}
impl Transport for EmptyRegistry {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        self.requests += 1;
        assert_eq!(request["operation"], "gui.list");
        Ok(
            json!({"transport_version":1,"window":request["window"],"epoch":request["epoch"],
            "session":request["session"],"request_id":request["request_id"],"status":"ok","value":[],"error":null}),
        )
    }
    fn is_live(&self) -> bool {
        self.live
    }
}
fn scope() -> Scope {
    Scope {
        window: "named".into(),
        epoch: "fresh".into(),
        session: "one".into(),
    }
}

#[test]
fn named_inputs_bind_by_name_and_keep_two_effects_in_the_existing_vm() {
    let source = "fn main(title: string, content: string) = bind(first: gui.list(), body: bind(second: gui.list(), body: concat(left: title, right: content)))";
    let arguments = vec![
        ("content".into(), "正文 ✓".into()),
        ("title".into(), "Title: ".into()),
    ];
    assert!(prepare_task(source, arguments.clone()).is_err());
    let prepared = prepare_named_task(source, arguments).unwrap();
    let mut transport = EmptyRegistry {
        requests: 0,
        live: true,
    };
    let report = run_prepared_registered_task(&mut transport, scope(), prepared).unwrap();
    assert_eq!(
        report["result"],
        json!({"type":"string","value":"Title: 正文 ✓"})
    );
    assert_eq!(report["effects"], 2);
    assert!(report["fuel_remaining"].as_u64().unwrap() < 10_000);
    assert_eq!(
        transport.requests, 3,
        "bootstrap plus two explicitly driven Effects"
    );
}

#[test]
fn named_admission_checks_the_whole_input_map_before_any_transport_exists() {
    let source = "fn main(title: string, content: string) = title";
    for arguments in [
        vec![],
        vec![("title".into(), "missing content".into())],
        vec![
            ("title".into(), "a".into()),
            ("title".into(), "duplicate".into()),
        ],
        vec![
            ("title".into(), "a".into()),
            ("other".into(), "unknown".into()),
        ],
        vec![
            ("title".into(), "a".into()),
            ("content".into(), "x".repeat(4097)),
        ],
        vec![
            ("title".into(), "a".into()),
            ("content".into(), "nul\0data".into()),
        ],
        (0..9)
            .map(|index| (format!("arg{index}"), "x".into()))
            .collect(),
    ] {
        assert!(prepare_named_task(source, arguments).is_err());
    }
    assert!(
        prepare_named_task(
            "fn main(count: integer) = count",
            vec![("count".into(), "1".into())]
        )
        .is_err()
    );
    assert!(prepare_named_task("fn main() = gui.find(name: \"title\")", vec![]).is_err());
    assert!(prepare_named_task("fn main() = true", vec![]).is_ok());
    let parameters = (0..8)
        .map(|index| format!("p{index}: string"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("fn main({parameters}) = p0");
    let arguments = (0..8)
        .map(|index| (format!("p{index}"), "x".repeat(2048)))
        .collect();
    assert!(
        prepare_named_task(&source, arguments).is_ok(),
        "exactly 16 KiB across eight inputs"
    );
    let arguments = (0..8)
        .map(|index| (format!("p{index}"), "x".repeat(2049)))
        .collect();
    assert!(prepare_named_task(&source, arguments).is_err());
}

#[test]
fn a_closed_transport_still_refuses_named_execution_without_an_effect() {
    let task = prepare_named_task(
        "fn main(a: string, b: string) = a",
        vec![("a".into(), "one".into()), ("b".into(), "two".into())],
    )
    .unwrap();
    let mut transport = EmptyRegistry {
        requests: 0,
        live: false,
    };
    assert!(run_prepared_registered_task(&mut transport, scope(), task).is_err());
    assert_eq!(transport.requests, 0);
}

#[test]
fn generated_two_field_fixture_is_admitted_by_the_actual_registered_host() {
    let result = prepare_named_task(
        include_str!("../../../fixtures/host-semantics/multi-input-save.lese"),
        vec![
            ("title".into(), "Title".into()),
            ("content".into(), "Content".into()),
        ],
    );
    if let Err(error) = result {
        panic!("{error}");
    }
}
