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
        options::Options::Windows(application) => list_windows(&application),
        options::Options::AuxiliaryProcesses {
            application,
            window,
        } => list_auxiliary_processes(&application, &window),
        options::Options::Run { document, source } => {
            // File admission happens before native access, including permission queries.
            let custom_source = source.as_deref().map(options::load_source).transpose()?;
            run(&document, custom_source.as_deref())
        }
        options::Options::Calculator { source } => {
            let custom_source = source.as_deref().map(options::load_source).transpose()?;
            run_calculator(custom_source.as_deref())
        }
        options::Options::Controls { scope, input } => {
            let source = match input {
                options::ControlInput::Discover => None,
                options::ControlInput::Run(path) => Some(options::load_source(&path)?),
            };
            run_controls(scope, source.as_deref())
        }
        options::Options::Widgets {
            application,
            window,
            auxiliaries,
            input,
        } => {
            let source = match input {
                options::ControlInput::Discover => None,
                options::ControlInput::Run(path) => Some(options::load_source(&path)?),
            };
            run_widgets(&application, &window, auxiliaries, source.as_deref())
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

#[cfg(target_os = "macos")]
fn run_calculator(source: Option<&str>) -> Result<(), String> {
    use leselang_macos_ax_host::{calculator, calculator_access::CalculatorAccess};
    use leselang_vm::native::core::ScalarValue;
    let backend = CalculatorAccess::connect().map_err(|error| error.to_string())?;
    let summary = calculator::run(&backend, source.unwrap_or(calculator::PROGRAM))?;
    if source.is_none() && summary.value != ScalarValue::String("3".into()) {
        return Err(format!(
            "calculator demo postcondition failed: {}",
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
fn run_calculator(_source: Option<&str>) -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}

#[cfg(target_os = "macos")]
fn run_controls(scope: options::ControlScope, source: Option<&str>) -> Result<(), String> {
    use leselang_macos_ax_host::{
        controls::{self},
        controls_access::ControlsAccess,
        scope_access::ApplicationAccess,
    };
    let backend = match scope {
        options::ControlScope::Document(path) => ControlsAccess::document(&path),
        options::ControlScope::Calculator => ControlsAccess::calculator(),
        options::ControlScope::Application {
            application,
            window,
        } => ApplicationAccess::connect(&application)
            .and_then(|application| application.controls(&window)),
    }
    .map_err(|error| error.to_string())?;
    if let Some(source) = source {
        let summary = controls::run(&backend, source)?;
        println!(
            "Completed {} Effects; Fuel remaining: {}\nLeseLang result: {}",
            summary.effects,
            summary.fuel_remaining,
            options::render_value(&summary.value)
        );
    } else {
        let state = controls::discover(&backend).map_err(|error| error.to_string())?;
        println!(
            "Discovered {} controls; scope revision: {}",
            state.controls.len(),
            state.revision
        );
        for control in state.controls {
            let actions: Vec<_> = [(control.pressable, "press"), (control.editable, "set_text")]
                .into_iter()
                .filter_map(|(available, name)| available.then_some(name))
                .collect();
            println!(
                "key={:?} role={:?} identifier={:?} label={:?} value={:?} enabled={} actions={actions:?}",
                control.key,
                control.role,
                control.identifier,
                control.label,
                control.value,
                control.enabled
            );
        }
        println!("Keys belong to this connection. Use exact metadata selectors in later runs.");
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn run_controls(_scope: options::ControlScope, _source: Option<&str>) -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}

#[cfg(target_os = "macos")]
fn list_windows(spec: &leselang_macos_ax_host::scope::ApplicationSpec) -> Result<(), String> {
    use leselang_macos_ax_host::scope_access::ApplicationAccess;
    let application = ApplicationAccess::connect(spec).map_err(|error| error.to_string())?;
    let windows = application.windows().map_err(|error| error.to_string())?;
    println!(
        "Discovered {} windows; pinned PID: {}",
        windows.len(),
        application.pid()
    );
    for window in windows {
        println!(
            "key={:?} title={:?} identifier={:?} document={:?}",
            window.key, window.title, window.identifier, window.document
        );
    }
    println!(
        "Window keys belong to this connection. Use exact metadata selectors in later CLI runs."
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn list_windows(_spec: &leselang_macos_ax_host::scope::ApplicationSpec) -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}

#[cfg(target_os = "macos")]
fn list_auxiliary_processes(
    spec: &leselang_macos_ax_host::scope::ApplicationSpec,
    window: &leselang_macos_ax_host::scope::WindowSelector,
) -> Result<(), String> {
    use leselang_macos_ax_host::scope_access::ApplicationAccess;
    let application = ApplicationAccess::connect(spec).map_err(|error| error.to_string())?;
    let candidates = application
        .auxiliary_processes(window)
        .map_err(|error| error.to_string())?;
    println!(
        "Discovered {} auxiliary process candidates; pinned primary PID: {}",
        candidates.len(),
        application.pid()
    );
    for candidate in candidates {
        println!(
            "pid={} start_seconds={} start_micros={} executable={:?}",
            candidate.pid,
            candidate.start_seconds,
            candidate.start_micros,
            candidate.executable_path
        );
    }
    println!(
        "Discovery grants no access to these processes. Explicitly pass reviewed identity tuples with --allow-auxiliary PID START_SECONDS START_MICROS EXECUTABLE."
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn list_auxiliary_processes(
    _spec: &leselang_macos_ax_host::scope::ApplicationSpec,
    _window: &leselang_macos_ax_host::scope::WindowSelector,
) -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}

#[cfg(target_os = "macos")]
fn run_widgets(
    spec: &leselang_macos_ax_host::scope::ApplicationSpec,
    window: &leselang_macos_ax_host::scope::WindowSelector,
    auxiliaries: Vec<leselang_macos_ax_host::scope::AuxiliaryProcessSpec>,
    source: Option<&str>,
) -> Result<(), String> {
    use leselang_macos_ax_host::{scope::AuxiliaryGrant, scope_access::ApplicationAccess, widgets};
    let application = ApplicationAccess::connect(spec).map_err(|error| error.to_string())?;
    let backend = if auxiliaries.is_empty() {
        application.widgets(window)
    } else {
        let grant = AuxiliaryGrant::new(auxiliaries).map_err(|error| error.to_string())?;
        application.widgets_with_auxiliary(window, &grant)
    }
    .map_err(|error| error.to_string())?;
    if let Some(source) = source {
        let summary = widgets::run(&backend, source)?;
        println!(
            "Completed {} Effects; Fuel remaining: {}\nLeseLang result: {}",
            summary.effects,
            summary.fuel_remaining,
            options::render_value(&summary.value)
        );
    } else {
        let state = widgets::discover(&backend).map_err(|error| error.to_string())?;
        println!(
            "Discovered {} widgets; scope revision: {}",
            state.widgets.len(),
            state.revision
        );
        for widget in &state.widgets {
            let c = &widget.control;
            let parent = widget
                .parent
                .map(|identity| {
                    state
                        .widget(identity)
                        .map(|parent| parent.control.key.as_str())
                })
                .transpose()
                .map_err(|error| error.to_string())?;
            println!(
                "key={:?} role={:?} identifier={:?} label={:?} value={:?} enabled={} checked={:?} selected={:?} position={:?} orientation={:?} parent={parent:?} actions={:?}",
                c.key,
                c.role,
                c.identifier,
                c.label,
                c.value,
                c.enabled,
                widget.checked,
                widget.selected,
                widget.position,
                widget.orientation,
                widget.actions()
            );
        }
        println!(
            "Widget keys belong to this connection. Use exact metadata selectors in later runs."
        );
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn run_widgets(
    _spec: &leselang_macos_ax_host::scope::ApplicationSpec,
    _window: &leselang_macos_ax_host::scope::WindowSelector,
    _auxiliaries: Vec<leselang_macos_ax_host::scope::AuxiliaryProcessSpec>,
    _source: Option<&str>,
) -> Result<(), String> {
    Err("this adapter requires macOS 14+".into())
}
