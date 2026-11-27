use crate::article::{Article, Fault, RequestTimer, Session};
use leselang_registered_host::{
    ExecutionFailure, ExecutionFailureCode, Transport, prepare_task,
    run_prepared_registered_task_detailed,
};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Instant};

pub(crate) struct Fixture {
    program: String,
    read_draft: String,
    read_publication: String,
    pub(crate) text: String,
    pub(crate) superseding_text: String,
    pub(crate) cases: Vec<Value>,
}

fn execute(
    session: &mut Session<'_>,
    program: &str,
    arguments: Vec<(String, String)>,
) -> Result<Value, ExecutionFailure> {
    let prepared = prepare_task(program, arguments).unwrap();
    let scope = session.scope.clone();
    run_prepared_registered_task_detailed(session, scope, prepared)
}

fn state_from_snapshots(snapshots: &Value) -> Value {
    let controls = snapshots.as_array().unwrap();
    assert_eq!(controls.len(), 2);
    let control = |name: &str| controls.iter().find(|c| c["identifier"] == name).unwrap();
    let draft = control("draft");
    let publish = control("publish");
    json!({"draft":draft["value"].as_str().unwrap(),
           "published":publish["value"].as_str().unwrap(),
           "clicks":publish["clicks"].as_u64().unwrap()})
}

impl Fixture {
    pub(crate) fn load(path: &Path) -> Self {
        let fixture: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(fixture["version"], 1);
        let source = |key: &str| {
            fs::read_to_string(path.parent().unwrap().join(fixture[key].as_str().unwrap())).unwrap()
        };
        Self {
            program: source("program"),
            read_draft: source("read_draft"),
            read_publication: source("read_publication"),
            text: fixture["arguments"]["text"].as_str().unwrap().into(),
            superseding_text: fixture["superseding_text"].as_str().unwrap().into(),
            cases: fixture["cases"].as_array().unwrap().clone(),
        }
    }

    fn read_value(&self, app: &mut Article, name: &str, program: &str) -> String {
        let mut session = app.session(
            &format!("read-{name}"),
            Fault::None,
            &self.text,
            &self.superseding_text,
        );
        session.read_only = true;
        let report = execute(&mut session, program, vec![]).unwrap();
        assert_eq!(report["effects"], 2);
        assert_eq!(report["result"]["type"], "string");
        // Two discovery reads per object, then this program's own find/observe,
        // then the host's independent verification of both objects.
        assert_eq!(
            session.trace,
            [
                "list",
                "find:draft",
                "observe:draft",
                "find:publish",
                "observe:publish",
                &format!("find:{name}"),
                &format!("observe:{name}"),
                "observe:draft",
                "observe:publish",
            ]
        );
        report["result"]["value"].as_str().unwrap().into()
    }

    /// Return the independently observed row and diagnostic for the crate's
    /// message regressions. Expected data is consulted only after both reads.
    pub(crate) fn check(
        &self,
        case: &Value,
        measure: bool,
    ) -> (Value, Option<ExecutionFailure>, Option<Value>) {
        let fault = Fault::parse(case["fault"].as_str().unwrap());
        let mut app = Article::new(fault);
        let mut session = app.session("publish", fault, &self.text, &self.superseding_text);
        session.request_timer = measure.then(RequestTimer::default);
        let arguments = vec![("text".into(), self.text.clone())];
        let primary_start = measure.then(Instant::now);
        let execution = execute(&mut session, &self.program, arguments);
        let primary = primary_start.map(|start| start.elapsed());
        let request_span = session.request_timer.as_ref().map(RequestTimer::span);
        let cancelled = !session.is_live();
        let trace = session.trace;

        let commits = (app.edits, app.clicks);
        let readback_start = measure.then(Instant::now);
        let draft = self.read_value(&mut app, "draft", &self.read_draft);
        let published = self.read_value(&mut app, "publish", &self.read_publication);
        let readback = readback_start.map(|start| start.elapsed());
        assert_eq!(commits, (app.edits, app.clicks));
        assert_eq!((&draft, &published), (&app.draft, &app.published));

        let (outcome, result, effects, report_after, verified) = match &execution {
            Ok(report) => {
                assert!(!cancelled, "retired transport published a report");
                assert_eq!(report["result"]["type"], "boolean");
                let result = report["result"]["value"].as_bool().unwrap();
                let effects = report["effects"].as_u64().unwrap();
                assert_eq!(
                    state_from_snapshots(&report["objects_before"]),
                    json!({"draft":"","published":"","clicks":0})
                );
                if commits == (0, 0) {
                    assert_eq!(report["objects_after"], report["objects_before"]);
                }
                let after = state_from_snapshots(&report["objects_after"]);
                let verified = result
                    && after["draft"] == self.text
                    && after["published"] == self.text
                    && after["clicks"] == 1
                    && draft == self.text
                    && published == self.text
                    && commits == (1, 1);
                (
                    "reported",
                    Some(result),
                    Some(effects),
                    Some(after),
                    verified,
                )
            }
            Err(error) => {
                assert_eq!(error.code(), ExecutionFailureCode::Failed);
                let outcome = if cancelled {
                    assert_eq!(
                        error.to_string(),
                        "JavaScript authority revoked before reply"
                    );
                    "cancelled"
                } else {
                    error.code().as_str()
                };
                (outcome, None, None, None, false)
            }
        };
        let observed = json!({"outcome":outcome, "result":result, "effects":effects, "verified":verified,
            "report_after":report_after, "readback":{"draft":draft,"published":published},
            "commits":{"edits":commits.0,"clicks":commits.1}, "trace":trace});
        assert_eq!(
            observed, case["expected"],
            "publication case {}",
            case["id"]
        );
        let timing = primary.map(|elapsed| {
            json!({
                "id":case["id"], "primary_ns":elapsed.as_nanos(),
                "request_span_ns":request_span.unwrap().as_nanos(),
                "readback_ns":readback.unwrap().as_nanos(),
            })
        });
        (observed, execution.err(), timing)
    }
}
