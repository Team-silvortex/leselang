#[cfg(target_os = "macos")]
mod driver;
mod options;
#[cfg(any(target_os = "macos", test))]
mod report;
#[cfg(target_os = "macos")]
mod self_test;

fn main() {
    let options = match options::Options::parse(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("AppKit host: {error}");
            std::process::exit(1);
        }
    };
    if options.mode == options::Mode::Help {
        println!("{}", options::HELP);
        return;
    }
    if options.mode == options::Mode::Describe {
        use std::io::Write;
        let result = (|| -> Result<(), String> {
            let json = leselang_appkit_host::describe_json()?;
            let mut output = std::io::stdout().lock();
            output
                .write_all(json.as_bytes())
                .and_then(|()| output.flush())
                .map_err(|error| {
                    format!("cannot write catalog description: {error}; output may be partial")
                })
        })();
        if let Err(error) = result {
            eprintln!("AppKit host: {error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = driver::main(options) {
        eprintln!("AppKit host: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("This host requires macOS AppKit and a logged-in desktop session.");
        std::process::exit(2);
    }
}
