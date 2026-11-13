mod options;

fn main() {
    if let Err(error) = execute() {
        eprintln!("LeseLang Accessibility host: {error}");
        std::process::exit(1);
    }
}

fn execute() -> Result<(), String> {
    let options = options::Options::parse(std::env::args_os().skip(1))?;
    match options {
        options::Options::Help => {
            println!("{}", options::HELP);
            Ok(())
        }
        options::Options::Prepare(path) => {
            options::prepare(&path)?;
            println!(
                "Created {path:?}. Open this file in TextEdit, then run with --document FILE."
            );
            Ok(())
        }
        options::Options::Permission => check_permission(),
        options::Options::Run { document, source } => {
            // File admission happens before native access, including permission queries.
            let custom_source = source.as_deref().map(options::load_source).transpose()?;
            run(&document, custom_source.as_deref())
        }
    }
}
#[cfg(target_os = "macos")]
fn check_permission() -> Result<(), String> {
    if leselang_macos_ax_host::access::Access::trusted() {
        println!("Accessibility permission is available for this process.");
        Ok(())
    } else {
        Err(leselang_macos_ax_host::Error::PermissionDenied.to_string())
    }
}
#[cfg(not(target_os = "macos"))]
fn check_permission() -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}

#[cfg(target_os = "macos")]
fn run(document: &std::path::Path, source: Option<&str>) -> Result<(), String> {
    use leselang_macos_ax_host::{access::Access, driver, host};
    use leselang_vm::native::core::ScalarValue;
    let backend = Access::connect(document).map_err(|error| error.to_string())?;
    let summary = driver::run(&backend, source.unwrap_or(host::PROGRAM))?;
    if source.is_none() && summary.value != ScalarValue::String(options::EXPECTED.into()) {
        return Err(format!(
            "demo postcondition failed: {}",
            options::render_value(&summary.value)
        ));
    }
    println!(
        "Completed {} Effects; Fuel remaining: {}\nLeseLang result: {}",
        summary.effects,
        summary.fuel_remaining,
        options::render_value(&summary.value)
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn run(_document: &std::path::Path, _source: Option<&str>) -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}
