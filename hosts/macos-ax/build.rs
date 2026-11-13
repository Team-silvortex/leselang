use std::{env, path::PathBuf, process::Command};

fn run(command: &mut Command) {
    let status = command
        .status()
        .expect("failed to start Apple developer tool");
    assert!(
        status.success(),
        "Accessibility bridge tool failed: {command:?}"
    );
}

fn deployment_target() -> String {
    let value = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "14.0".into());
    let pieces: Vec<_> = value.split('.').collect();
    assert!(
        (1..=3).contains(&pieces.len())
            && pieces.iter().all(|part| {
                !part.is_empty()
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && part.parse::<u32>().is_ok()
            }),
        "MACOSX_DEPLOYMENT_TARGET must be a numeric macOS version"
    );
    assert!(
        pieces[0].parse::<u32>().unwrap() >= 14,
        "the Accessibility reference host requires macOS 14.0 or newer"
    );
    value
}

fn main() {
    println!("cargo:rerun-if-changed=native/bridge.m");
    println!("cargo:rerun-if-changed=native/bridge.h");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let deployment = deployment_target();
    // Keep this isolated adapter's native and Rust deployment targets aligned;
    // the language crates retain their own broader platform requirements.
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET={deployment}");
    println!("cargo:rustc-link-arg=-mmacosx-version-min={deployment}");
    let target = env::var("TARGET").expect("Cargo target");
    let host = env::var("HOST").expect("Cargo host");
    assert_eq!(
        target, host,
        "the Accessibility reference host currently requires a native macOS build"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let object = out.join("bridge.o");
    let library = out.join("libleselang_ax_bridge.a");
    run(Command::new("xcrun")
        .args([
            "clang",
            "-fobjc-arc",
            "-fobjc-arc-exceptions",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "native/bridge.m",
            "-o",
        ])
        .arg(&object)
        .arg(format!("-mmacosx-version-min={deployment}")));
    run(Command::new("xcrun")
        .args(["ar", "rcs"])
        .arg(&library)
        .arg(&object));
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=leselang_ax_bridge");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=ApplicationServices");
    println!("cargo:rustc-link-lib=objc");
}
