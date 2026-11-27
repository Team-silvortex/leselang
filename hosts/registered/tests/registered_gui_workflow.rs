//! The independent Rust application drives the real registered gateway and VM.
#[path = "consumer/src/article.rs"]
mod article;
#[path = "consumer/src/publication.rs"]
mod publication;

use article::{Article, Fault};
use leselang_registered_host::{
    Error,
    gateway::{Backend, RemoteGui},
};
use publication::Fixture;
use std::path::Path;

fn fixture() -> Fixture {
    Fixture::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/registered-gui/publication.json"),
    )
}

#[test]
fn publication_verification_reads_the_two_controls_again_after_the_last_effect() {
    let fixture = fixture();
    for fault in ["none", "publication_superseded"] {
        let case = fixture.cases.iter().find(|c| c["fault"] == fault).unwrap();
        let (row, failure, _) = fixture.check(case, false);
        assert!(failure.is_none());
        assert_eq!(row["result"], true);
        assert_eq!(row["verified"], fault == "none");
    }
}

#[test]
fn acknowledged_text_without_the_requested_value_stops_before_publication() {
    let fixture = fixture();
    let case = fixture
        .cases
        .iter()
        .find(|c| c["fault"] == "acknowledge_without_edit")
        .unwrap();
    let (row, failure, _) = fixture.check(case, false);
    assert!(failure.is_none());
    assert_eq!(row["result"], false);
    assert_eq!(row["effects"], 5);
}

#[test]
fn a_write_on_the_last_wire_request_survives_failed_verification() {
    const REQUEST_LIMIT: usize = 256;
    let fixture = fixture();
    let mut app = Article::new(Fault::None);
    let mut session = app.session(
        "budget",
        Fault::None,
        &fixture.text,
        &fixture.superseding_text,
    );
    {
        let scope = session.scope.clone();
        let gui = RemoteGui::registered(&mut session, scope).unwrap();
        let slot = gui.find("draft").unwrap();
        let identity = gui.snapshot(slot).unwrap().identity;
        // Bootstrap lists once and finds and observes each object. We then found the draft.
        let requests_used = 1 + 2 * gui.object_count() + 1;
        for _ in requests_used..REQUEST_LIMIT - 1 {
            gui.list().unwrap();
        }
        gui.set_text(slot, identity, &fixture.text).unwrap();
        assert!(matches!(
            gui.verify_snapshots(),
            Err(Error::Native("wire request limit"))
        ));
        assert!(!gui.is_open().unwrap());
        assert!(gui.verify_snapshots().is_err());
    }
    assert_eq!(session.trace.len(), REQUEST_LIMIT);
    assert_eq!(session.trace.last().unwrap(), "set_text:draft");
    assert_eq!(app.draft, fixture.text);
    assert_eq!((app.edits, app.clicks), (1, 0));
}

#[test]
fn a_failed_publication_stage_stops_at_that_request_and_keeps_only_committed_writes() {
    let fixture = fixture();
    for (fault, message) in [
        ("wrong_publish_target", "foreign JavaScript target"),
        (
            "unversioned_edit",
            "JavaScript revision did not cover state changes",
        ),
        ("reject_click", "JavaScript rejected: cannot_publish"),
        (
            "unacknowledged_click",
            "JavaScript action was not acknowledged",
        ),
        (
            "final_read_invalid",
            "foreign or malformed TypeScript snapshot",
        ),
    ] {
        let case = fixture.cases.iter().find(|c| c["fault"] == fault).unwrap();
        let (_, failure, _) = fixture.check(case, false);
        assert_eq!(failure.unwrap().to_string(), message, "{fault}");
    }
}
