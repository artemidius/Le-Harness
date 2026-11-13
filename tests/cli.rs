use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_le-harness"))
}

#[test]
fn help_exposes_primary_workflows() {
    let output = Command::new(binary()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in ["doctor", "run", "eval", "session", "tui"] {
        assert!(stdout.contains(command), "help omitted {command}");
    }
}

#[test]
fn doctor_redacts_configured_secret() {
    let output = Command::new(binary())
        .arg("doctor")
        .env("MISTRAL_API_KEY", "never-print-this-value")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("MISTRAL_API_KEY: set"));
    assert!(!stdout.contains("never-print-this-value"));
}

#[test]
fn no_input_fails_fast_when_a_cloud_key_is_missing() {
    let output = Command::new(binary())
        .args(["--no-input", "doctor", "--live"])
        .env_remove("MISTRAL_API_KEY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("interactive input is unavailable"));
    assert!(stderr.contains("MISTRAL_API_KEY"));
}

#[test]
fn json_cloud_run_never_waits_for_a_key() {
    let output = Command::new(binary())
        .args(["run", "inspect", "--json"])
        .env_remove("MISTRAL_API_KEY")
        .env("LE_HARNESS_CHAT_MODEL", "fixture-chat")
        .env("LE_HARNESS_FIM_MODEL", "fixture-fim")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("interactive input is unavailable"));
}

#[cfg(unix)]
#[test]
fn interactive_key_is_hidden_restored_and_session_scoped() {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/credential_prompt.py");
    let output = match Command::new("python3").arg(script).arg(binary()).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("could not start PTY acceptance test: {error}"),
    };
    assert!(
        output.status.success(),
        "PTY check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fixture_eval_and_empty_session_listing_are_machine_readable() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/smoke.json");
    let output = Command::new(binary())
        .args(["eval", manifest.to_str().unwrap(), "--mode", "fim"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mode"], "fim");

    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .args(["session", "list", "--session-dir"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
}
