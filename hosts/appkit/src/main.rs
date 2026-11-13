#[cfg(target_os = "macos")]
mod driver;
#[cfg(any(target_os = "macos", test))]
mod options;
#[cfg(target_os = "macos")]
mod self_test;

fn main() {
    #[cfg(target_os = "macos")]
    if let Err(error) = driver::main() {
        eprintln!("AppKit host: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("This host requires macOS AppKit and a logged-in desktop session.");
        std::process::exit(2);
    }
}
