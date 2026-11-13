fn main() {
    #[cfg(all(target_os = "macos", feature = "desktop"))]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "bridge_ready",
            "bridge_reply",
            "bridge_closed",
            "agent_describe",
            "agent_start",
            "agent_start_task",
            "agent_reply",
            "agent_cancel",
            "agent_closed",
        ]),
    ))
    .expect("Tauri bridge permission configuration must be valid");
}
