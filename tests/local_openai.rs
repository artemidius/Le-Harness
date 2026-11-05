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
        "/usr/bin/python3",
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
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
    let child = Command::new(python)
        .args([fixture, port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    std::thread::sleep(Duration::from_millis(100));
    Some((Server(child), format!("http://127.0.0.1:{port}/v1/")))
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
