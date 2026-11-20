//! Compatibility facade for the shared registered-object host and form examples.
pub use leselang_registered_host::host::*;

pub const PROGRAM: &str = r#"
fn main() = bind(
    target: gui.find(name: "save"),
    body: bind(
        before: gui.observe(target: target),
        body: bind(
            submitted: gui.click(target: target, expected_revision: field(value: before, name: "revision")),
            body: bind(
                after: gui.observe(target: target),
                body: choose(
                    when: eq(left: field(value: after, name: "clicks"), right: add(left: field(value: before, name: "clicks"), right: 1)),
                    then: field(value: after, name: "title"),
                    otherwise: "verification failed",
                ),
            ),
        ),
    ),
)
"#;
/// Discover identifiers as scalar metadata, then resolve fresh object references.
/// The final value is read from Save's actual submitted-value snapshot.
pub const FORM_PROGRAM: &str = include_str!("../examples/form.lese");
