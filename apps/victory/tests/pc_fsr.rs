#![cfg(windows)]

use std::process::Command;

#[test]
fn series_s_pc_fsr_requires_a_provider_folder_only_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_victory"));
    command
        .current_dir(temp.path())
        .env_remove("VICTORY_CONFIG")
        .env_remove("VICTORY_FIDELITYFX_DIR")
        .env_remove("VICTORY_FSR_4K")
        .env_remove("VICTORY_RELAUNCHED_FROM")
        .env("VICTORY_SKIP_CPU_CHECK", "1")
        .env("VICTORY_CACHE_DIR", temp.path().join("cache"))
        .env("VICTORY_DATA_DIR", temp.path().join("data"))
        .args([
            "run",
            "synthetic-missing.exe",
            "--execute",
            "--console",
            "series-s",
        ]);

    let enabled = command.env("VICTORY_PC_FSR", "1").output().unwrap();
    assert!(!enabled.status.success(), "{enabled:?}");
    let stderr = String::from_utf8_lossy(&enabled.stderr);
    assert!(stderr.contains("PC FidelityFX requires"), "{stderr}");
    assert!(stderr.contains("VICTORY_FIDELITYFX_DIR"), "{stderr}");

    let disabled = command.env("VICTORY_PC_FSR", "0").output().unwrap();
    assert!(!disabled.status.success(), "{disabled:?}");
    let stderr = String::from_utf8_lossy(&disabled.stderr);
    assert!(stderr.contains("reading synthetic-missing.exe"), "{stderr}");

    let preset = command
        .env("VICTORY_PC_FSR", "1")
        .env("VICTORY_FSR_4K", "quality")
        .output()
        .unwrap();
    assert!(!preset.status.success(), "{preset:?}");
    let stderr = String::from_utf8_lossy(&preset.stderr);
    assert!(stderr.contains("FSR 4K requires Series X mode"), "{stderr}");
}
