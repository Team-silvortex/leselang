use std::cell::{Cell, RefCell};

use leselang_runtime_core::OptionalStringValue;
use leselang_vm::embedding::{
    Arguments, EffectSessionEnd, Fuel, Host, HostBuilder, Poll, Program, Request, ScalarType,
    ScalarValue, Session, parameter,
};
use serde::Deserialize;
use serde_json::{Value, json};

const WRITE_GRANTS: &[&str] = &["ledger.write", "ledger.read"];
const LOOKUP_GRANTS: &[&str] = &["ledger.lookup"];

#[derive(Deserialize)]
struct Fixture {
    write_source: String,
    idempotent_source: String,
    lookup_source: String,
    initial_balance: String,
    unknown_key: String,
    cases: Vec<Case>,
    replay_control: ReplayControl,
    key_isolation: Value,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    key: String,
    expected: Value,
    idempotent_retry: Value,
}

#[derive(Deserialize)]
struct ReplayControl {
    case: String,
    committed_writes: usize,
    balance: String,
}

#[derive(Clone, Copy)]
enum Write {
    Add,
    AddOnce,
}

impl Write {
    fn name(self) -> &'static str {
        match self {
            Self::Add => "ledger.add",
            Self::AddOnce => "ledger.add_once",
        }
    }
}

// Each journal entry is one application commit and contains its keyed receipt.
#[derive(Default)]
struct Ledger {
    initial_balance: u64,
    journal: RefCell<Vec<(String, u64)>>,
    add_calls: Cell<usize>,
    tail_reads: Cell<usize>,
}

impl Ledger {
    fn with_balance(initial_balance: &str) -> Self {
        Self {
            initial_balance: initial_balance.parse().unwrap(),
            ..Self::default()
        }
    }

    fn balance(&self) -> u64 {
        self.journal
            .borrow()
            .last()
            .map_or(self.initial_balance, |(_, balance)| *balance)
    }

    fn receipt(&self, key: &str) -> Option<u64> {
        self.journal
            .borrow()
            .iter()
            .rev()
            .find_map(|(record, balance)| (record == key).then_some(*balance))
    }

    fn host(&self, write: Write) -> Host<'_> {
        let mut builder = HostBuilder::new();
        builder
            .register(
                write.name(),
                vec![parameter("key", ScalarType::String)],
                ScalarType::Integer,
                "ledger.write",
                move |args, _| {
                    self.add_calls.set(self.add_calls.get() + 1);
                    let key = text_argument(args, "key");
                    if matches!(write, Write::AddOnce)
                        && let Some(receipt) = self.receipt(key)
                    {
                        return Ok(ScalarValue::Integer(receipt));
                    }
                    let balance = self.balance().checked_add(1).unwrap();
                    self.journal.borrow_mut().push((key.into(), balance));
                    Ok(ScalarValue::Integer(balance))
                },
            )
            .unwrap();
        builder
            .register(
                "ledger.read",
                vec![],
                ScalarType::Integer,
                "ledger.read",
                |_, _| {
                    self.tail_reads.set(self.tail_reads.get() + 1);
                    Ok(ScalarValue::Integer(self.balance()))
                },
            )
            .unwrap();
        builder
            .register(
                "ledger.lookup",
                vec![parameter("key", ScalarType::String)],
                ScalarType::OptionalString,
                "ledger.lookup",
                |args, _| {
                    let key = text_argument(args, "key");
                    let receipt = self.receipt(key).map(|balance| balance.to_string());
                    Ok(ScalarValue::OptionalString(OptionalStringValue(receipt)))
                },
            )
            .unwrap();
        builder.build()
    }
}

fn text_argument<'a>(args: &'a Arguments, name: &str) -> &'a str {
    match args.get(name) {
        Some(ScalarValue::String(value)) => value,
        value => panic!("expected string argument {name}, got {value:?}"),
    }
}

fn request(poll: Poll, operation: &str) -> Request {
    let Poll::Request(request) = poll else {
        panic!("expected {operation}, got {poll:?}")
    };
    assert_eq!(request.operation(), operation);
    request
}

fn input(key: &str) -> Vec<(String, ScalarValue)> {
    vec![("key".into(), ScalarValue::String(key.into()))]
}

fn run_to_value(mut session: Session<'_, '_, '_>, operations: &[&str]) -> (ScalarValue, usize) {
    let mut accepted = 0;
    loop {
        match session.poll().unwrap() {
            Poll::Request(work) => {
                assert_eq!(Some(work.operation()), operations.get(accepted).copied());
                let authorized = session.authorize(work).unwrap();
                let receipt = session.dispatch(authorized).unwrap();
                session.accept(receipt).unwrap();
                accepted += 1;
            }
            Poll::Value(value) => {
                assert_eq!(accepted, operations.len());
                return (value, accepted);
            }
            other => panic!("expected an effect or result, got {other:?}"),
        }
    }
}

fn interrupt_write(mut session: Session<'_, '_, '_>, case: &Case, operation: &str) {
    let work = request(session.poll().unwrap(), operation);
    assert_eq!(text_argument(work.arguments(), "key"), case.key);
    let authorized = session.authorize(work).unwrap();
    match case.id.as_str() {
        "request-lost" => drop(authorized),
        "reply-lost" => drop(session.dispatch(authorized).unwrap()),
        other => panic!("unknown cut {other}"),
    }
    assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
    assert!(session.cancel());
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Cancelled)
    ));
}

fn lookup_receipt(host: &Host<'_>, lookup: &Program, key: &str) -> Option<String> {
    let session = host
        .start_with_arguments(lookup, input(key), LOOKUP_GRANTS, Fuel::new(10_000))
        .unwrap();
    let (ScalarValue::OptionalString(OptionalStringValue(receipt)), _) =
        run_to_value(session, &["ledger.lookup"])
    else {
        panic!("expected an optional receipt")
    };
    receipt
}

fn fixture() -> Fixture {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../fixtures/application-operations/lost-reply.json"
    ))
    .unwrap();
    let mut cases: Vec<_> = fixture.cases.iter().map(|case| case.id.as_str()).collect();
    cases.sort_unstable();
    assert_eq!(cases, ["reply-lost", "request-lost"]);
    assert_eq!(fixture.replay_control.case, "reply-lost");
    fixture
}

fn snapshot_after_loss(
    ledger: &Ledger,
    host: &Host<'_>,
    lookup: &Program,
    case: &Case,
    unknown_key: &str,
) -> Value {
    json!({
        "run_report": false,
        "add_calls": ledger.add_calls.get(),
        "committed_writes": ledger.journal.borrow().len(),
        "tail_reads": ledger.tail_reads.get(),
        "lookup_receipt": lookup_receipt(host, lookup, &case.key),
        "unknown_receipt": lookup_receipt(host, lookup, unknown_key),
        "balance": ledger.balance().to_string(),
    })
}

#[test]
fn reconcile_a_lost_write_reply_by_key_before_considering_replay() {
    let fixture = fixture();
    for case in &fixture.cases {
        assert_ne!(case.key, fixture.unknown_key);
        let ledger = Ledger::with_balance(&fixture.initial_balance);
        let host = ledger.host(Write::Add);
        let write = host
            .compile_task(&fixture.write_source, "main", WRITE_GRANTS)
            .unwrap();
        let lookup = host
            .compile_task(&fixture.lookup_source, "main", LOOKUP_GRANTS)
            .unwrap();

        let session = host
            .start_with_arguments(&write, input(&case.key), WRITE_GRANTS, Fuel::new(10_000))
            .unwrap();
        interrupt_write(session, case, "ledger.add");
        let snapshot = snapshot_after_loss(&ledger, &host, &lookup, case, &fixture.unknown_key);
        assert_eq!(snapshot, case.expected, "{}", case.id);
        println!("Shared loss {}: {snapshot}", case.id);

        if case.id == fixture.replay_control.case {
            let session = host
                .start_with_arguments(&write, input(&case.key), WRITE_GRANTS, Fuel::new(10_000))
                .unwrap();
            assert_eq!(
                run_to_value(session, &["ledger.add", "ledger.read"]),
                (ScalarValue::Integer(ledger.balance()), 2)
            );
            assert_eq!(
                ledger.journal.borrow().len(),
                fixture.replay_control.committed_writes
            );
            assert_eq!(ledger.balance().to_string(), fixture.replay_control.balance);
            assert_eq!(ledger.tail_reads.get(), 1);
            println!(
                "Blind replay: {} commits, balance {}",
                ledger.journal.borrow().len(),
                ledger.balance()
            );
        }
    }
}

#[test]
fn same_key_retry_after_either_cut_returns_the_original_receipt_and_commits_once() {
    let fixture = fixture();
    for case in &fixture.cases {
        let ledger = Ledger::with_balance(&fixture.initial_balance);
        let host = ledger.host(Write::AddOnce);
        let write = host
            .compile_task(&fixture.idempotent_source, "main", WRITE_GRANTS)
            .unwrap();
        let lookup = host
            .compile_task(&fixture.lookup_source, "main", LOOKUP_GRANTS)
            .unwrap();

        let start = |key: &str| {
            host.start_with_arguments(&write, input(key), WRITE_GRANTS, Fuel::new(10_000))
                .unwrap()
        };
        interrupt_write(start(&case.key), case, Write::AddOnce.name());
        let interrupted = snapshot_after_loss(&ledger, &host, &lookup, case, &fixture.unknown_key);
        assert_eq!(interrupted, case.expected, "{}", case.id);

        let submit = |key| {
            let (result, effects) =
                run_to_value(start(key), &[Write::AddOnce.name(), "ledger.read"]);
            let ScalarValue::Integer(receipt) = result else {
                panic!("expected an integer receipt, got {result:?}");
            };
            (receipt.to_string(), effects)
        };
        let (receipt, effects) = submit(&case.key);
        assert_eq!(
            lookup_receipt(&host, &lookup, &case.key).as_ref(),
            Some(&receipt)
        );
        let retried = json!({
            "add_calls": ledger.add_calls.get(),
            "committed_writes": ledger.journal.borrow().len(),
            "tail_reads": ledger.tail_reads.get(),
            "effects": effects,
            "receipt": receipt,
            "balance": ledger.balance().to_string()
        });
        assert_eq!(retried, case.idempotent_retry, "{}", case.id);
        println!("Idempotent retry {}: {retried}", case.id);

        // A second key advances the balance so a handler returning the current
        // balance on duplicate delivery would produce the wrong original receipt.
        let other_key = fixture.key_isolation["other_key"].as_str().unwrap();
        assert_ne!(other_key, case.key);
        let (other_receipt, _) = submit(other_key);
        let (replayed_receipt, _) = submit(&case.key);
        let isolation = json!({
            "other_key": other_key,
            "other_receipt": other_receipt,
            "replayed_receipt": replayed_receipt,
            "balance": ledger.balance().to_string(),
            "committed_writes": ledger.journal.borrow().len()
        });
        assert_eq!(isolation, fixture.key_isolation, "{}", case.id);
        println!("Key isolation {}: {isolation}", case.id);
    }
}
