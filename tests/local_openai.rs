use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures_util::StreamExt;
use le_harness::local::{LocalModelConfig, LocalOpenAiAdapter};
use le_harness::model::{ChatTurn, ModelAdapter, ModelEvent};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server() -> Option<(Server, String)> {
    let python = [
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_file())?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    let fixture = format!(
        "{}/tests/fixtures/openai_server.py",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut child = Command::new(python)
        .args([fixture, port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut ready = false;
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        if child.try_wait().ok().flatten().is_some() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some((Server(child), format!("http://127.0.0.1:{port}/v1/")))
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_le-harness"))
}

#[test]
fn local_only_cli_never_requests_a_mistral_key() {
    let Some((_server, base_url)) = start_server() else {
        return;
    };
    let sessions = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .args([
            "--no-input",
            "run",
            "answer locally",
            "--mode",
            "inspect",
            "--json",
            "--local-only",
            "--local-base-url",
            &base_url,
            "--local-model",
            "local-test",
            "--session-dir",
        ])
        .arg(sessions.path())
        .env_remove("MISTRAL_API_KEY")
        .env("CI", "true")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "local-only run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Mistral API key"));
}

#[tokio::test]
async fn probes_streams_and_completes_fim_against_local_fixture() {
    let Some((_server, base_url)) = start_server() else {
        return;
    };
    let adapter = LocalOpenAiAdapter::new(LocalModelConfig {
        base_url,
        chat_model: "local-test".into(),
        fim_model: Some("local-test".into()),
        api_key: None,
        tool_calling: true,
    })
    .unwrap();
    assert_eq!(adapter.list_models().await.unwrap(), vec!["local-test"]);
    let mut stream = adapter.stream_chat(ChatTurn::user("hello")).await.unwrap();
    assert!(matches!(
        stream.next().await.unwrap().unwrap(),
        ModelEvent::Text { ref text, .. } if text == "local answer"
    ));
    assert_eq!(
        adapter
            .complete_fim("before".into(), "after".into())
            .await
            .unwrap(),
        "middle"
    );
}

#[tokio::test]
async fn refuses_fim_when_local_capability_is_not_declared() {
    let Some((_server, base_url)) = start_server() else {
        return;
    };
    let adapter = LocalOpenAiAdapter::new(LocalModelConfig {
        base_url,
        chat_model: "local-test".into(),
        fim_model: None,
        api_key: None,
        tool_calling: true,
    })
    .unwrap();
    assert!(adapter.complete_fim("a".into(), "b".into()).await.is_err());
}
