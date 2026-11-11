//! GUI and CLI reference models using one local host-protocol contract.
//! These are deterministic models, not an operating-system GUI or process driver.
#[path = "../tests/support/protocol_host.rs"]
mod protocol_host;

fn main() -> Result<(), String> {
    for (name, summary) in [
        ("gui", protocol_host::run_reference_gui()?),
        ("cli", protocol_host::run_reference_cli()?),
    ] {
        if summary.text != "ready" || summary.observations != 2 || summary.submissions != 1 {
            return Err(format!(
                "{name}: observe/action/observe contract did not complete"
            ));
        }
        println!(
            "{name}: text={:?}, observations={}, submissions={}, pumps={}, fuel_remaining={}",
            summary.text,
            summary.observations,
            summary.submissions,
            summary.pumps,
            summary.fuel_remaining,
        );
    }
    Ok(())
}
