//! Compatibility entry; the standalone worker owns the implementation.
fn main() -> std::process::ExitCode {
    leselang_registered_host::worker_main()
}
