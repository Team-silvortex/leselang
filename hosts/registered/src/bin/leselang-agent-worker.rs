//! Standalone registered-object worker using the shared explicit VM driver.
fn main() -> std::process::ExitCode {
    leselang_registered_host::worker_main()
}
