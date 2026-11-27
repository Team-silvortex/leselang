//! This application keeps its own state across independent registered sessions.
use leselang_registered_host::{Scope, Transport};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Fault {
    None,
    WrongPublishTarget,
    UnversionedEdit,
    RejectClick,
    UnacknowledgedClick,
    AcknowledgeWithoutEdit,
    FinalReadInvalid,
    PublicationSuperseded,
    DisabledDraft,
    ReadonlyDraft,
    CancelBeforeEdit,
    CancelAfterEdit,
    ReplacedDraftAtWrite,
    ExternalEditAtWrite,
    WriteGrantRevoked,
}

impl Fault {
    pub(crate) fn parse(name: &str) -> Self {
        match name {
            "none" => Self::None,
            "wrong_publish_target" => Self::WrongPublishTarget,
            "unversioned_edit" => Self::UnversionedEdit,
            "reject_click" => Self::RejectClick,
            "unacknowledged_click" => Self::UnacknowledgedClick,
            "acknowledge_without_edit" => Self::AcknowledgeWithoutEdit,
            "final_read_invalid" => Self::FinalReadInvalid,
            "publication_superseded" => Self::PublicationSuperseded,
            "disabled_draft" => Self::DisabledDraft,
            "readonly_draft" => Self::ReadonlyDraft,
            "cancel_before_edit" => Self::CancelBeforeEdit,
            "cancel_after_edit" => Self::CancelAfterEdit,
            "replaced_draft_at_write" => Self::ReplacedDraftAtWrite,
            "external_edit_at_write" => Self::ExternalEditAtWrite,
            "write_grant_revoked" => Self::WriteGrantRevoked,
            _ => panic!("unknown publication fault {name}"),
        }
    }
}

pub(crate) struct Article {
    pub(crate) draft: String,
    pub(crate) published: String,
    pub(crate) edits: u32,
    pub(crate) clicks: u32,
    draft_generation: u32,
    draft_revision: u32,
    publish_revision: u32,
    enabled: bool,
    editable: bool,
    write_granted: bool,
}

pub(crate) struct Session<'a> {
    app: &'a mut Article,
    fault: Fault,
    text: &'a str,
    superseding_text: &'a str,
    pub(crate) scope: Scope,
    pub(crate) trace: Vec<String>,
    pub(crate) read_only: bool,
    pub(crate) request_timer: Option<RequestTimer>,
    live: bool,
}

#[derive(Default)]
pub(crate) struct RequestTimer {
    entries: Option<(Instant, Instant)>,
}

impl RequestTimer {
    fn record_entry(&mut self) {
        let now = Instant::now();
        let (_, last) = self.entries.get_or_insert((now, now));
        *last = now;
    }

    pub(crate) fn span(&self) -> Duration {
        let (first, last) = self.entries.expect("benchmark requires a GUI request");
        last.duration_since(first)
    }
}

impl Article {
    pub(crate) fn new(fault: Fault) -> Self {
        Self {
            draft: String::new(),
            published: String::new(),
            edits: 0,
            clicks: 0,
            draft_generation: 7,
            draft_revision: 30,
            publish_revision: 80,
            enabled: fault != Fault::DisabledDraft,
            editable: fault != Fault::ReadonlyDraft,
            write_granted: true,
        }
    }

    pub(crate) fn session<'a>(
        &'a mut self,
        name: &str,
        fault: Fault,
        text: &'a str,
        superseding_text: &'a str,
    ) -> Session<'a> {
        Session {
            app: self,
            fault,
            text,
            superseding_text,
            scope: Scope {
                window: "editor".into(),
                epoch: "opened".into(),
                session: name.into(),
            },
            trace: Vec::new(),
            read_only: false,
            request_timer: None,
            live: true,
        }
    }

    fn target(&self, name: &str) -> Value {
        let generation = match name {
            "draft" => self.draft_generation,
            "publish" => 13,
            _ => panic!("unknown article control {name}"),
        };
        json!({"name":name, "generation":generation})
    }
}

impl Session<'_> {
    fn snapshot(&self, name: &str) -> Value {
        let app = &self.app;
        let (role, value, revision, clicks) = match name {
            "draft" => ("text_field", &app.draft, app.draft_revision, 0),
            "publish" => ("button", &app.published, app.publish_revision, app.clicks),
            _ => panic!("unknown article control {name}"),
        };
        json!({"identifier":name,"generation":app.target(name)["generation"],
            "role":role,"title":name,"value":value,
            "enabled":name!="draft" || app.enabled,
            "editable":name=="draft" && app.editable,
            "revision":revision,"clicks":clicks})
    }
}

impl Transport for Session<'_> {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        if let Some(timer) = &mut self.request_timer {
            timer.record_entry();
        }
        let args = &request["arguments"];
        let operation = request["operation"].as_str().unwrap();
        assert!(self.live);
        assert!(
            !self.read_only || matches!(operation, "gui.list" | "gui.find" | "gui.observe"),
            "unexpected write in a read-only publication session"
        );
        let id = self.trace.len() + 1;
        assert_eq!(
            request,
            json!({"transport_version":1,"window":self.scope.window,
            "epoch":self.scope.epoch, "session":self.scope.session, "request_id":id,
            "budget_ms":5000,"operation":operation,"arguments":args})
        );
        let mut error = None;
        let (trace, value) = match operation {
            "gui.list" => {
                assert_eq!(args, &json!({}));
                ("list".into(), json!(["draft", "publish"]))
            }
            "gui.find" => {
                let name = args["name"].as_str().unwrap();
                assert_eq!(args, &json!({"name":name}));
                let rebound = self.fault == Fault::WrongPublishTarget
                    && self.app.edits > 0
                    && name == "publish";
                (
                    format!("find:{name}"),
                    self.app.target(if rebound { "draft" } else { name }),
                )
            }
            "gui.observe" => {
                let name = args["target"]["name"].as_str().unwrap();
                assert_eq!(args, &json!({"target":self.app.target(name)}));
                let mut snapshot = self.snapshot(name);
                if name == "draft" && self.app.edits > 0 && self.fault == Fault::UnversionedEdit {
                    snapshot["revision"] = json!(30);
                }
                // The host revisits the draft after the program has checked its publication.
                if name == "draft" && self.app.clicks == 1 {
                    match self.fault {
                        Fault::FinalReadInvalid => snapshot["identifier"] = json!("foreign"),
                        Fault::PublicationSuperseded => {
                            self.app.published = self.superseding_text.into();
                            self.app.publish_revision += 1;
                        }
                        _ => {}
                    }
                }
                (format!("observe:{name}"), snapshot)
            }
            "gui.set_text" => {
                assert_eq!(
                    args,
                    &json!({"target":self.app.target("draft"),"expected_revision":self.app.draft_revision,"text":self.text})
                );
                match self.fault {
                    Fault::ReplacedDraftAtWrite => self.app.draft_generation += 1,
                    Fault::ExternalEditAtWrite => {
                        self.app.draft = self.superseding_text.into();
                        self.app.draft_revision += 1;
                    }
                    Fault::CancelBeforeEdit => self.live = false,
                    Fault::WriteGrantRevoked => self.app.write_granted = false,
                    _ => {}
                }
                if !self.app.write_granted {
                    error = Some("permission_denied");
                } else if args["target"] != self.app.target("draft") {
                    error = Some("stale_target");
                } else if args["expected_revision"] != self.app.draft_revision {
                    error = Some("stale_revision");
                } else if self.live && self.fault != Fault::AcknowledgeWithoutEdit {
                    assert!(self.app.enabled && self.app.editable);
                    self.app.draft = args["text"].as_str().unwrap().into();
                    self.app.edits += 1;
                    self.app.draft_revision += 1;
                    if self.fault == Fault::CancelAfterEdit {
                        self.live = false;
                    }
                }
                (
                    "set_text:draft".into(),
                    if error.is_some() {
                        Value::Null
                    } else {
                        json!(true)
                    },
                )
            }
            "gui.click" => {
                assert_eq!(
                    args,
                    &json!({"target":self.app.target("publish"),"expected_revision":self.app.publish_revision})
                );
                let value = if self.fault == Fault::RejectClick {
                    error = Some("cannot_publish");
                    Value::Null
                } else {
                    self.app.published.clone_from(&self.app.draft);
                    self.app.clicks += 1;
                    self.app.publish_revision += 1;
                    json!(self.fault != Fault::UnacknowledgedClick)
                };
                ("click:publish".into(), value)
            }
            _ => panic!("unexpected operation {operation}"),
        };
        self.trace.push(trace);
        Ok(
            json!({"transport_version":1,"window":self.scope.window,"epoch":self.scope.epoch,
            "session":self.scope.session,"request_id":id,"status":if error.is_some() {"rejected"} else {"ok"},
            "error":error,"value":value}),
        )
    }

    fn is_live(&self) -> bool {
        self.live
    }
}
