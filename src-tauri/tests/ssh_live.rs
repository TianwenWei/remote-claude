//! Explicit, opt-in verification against a real SSH Claude workspace.
//! Run with --features remote-vps --test ssh_live -- --ignored --nocapture.
//! CCGUI_SSH_LIVE_KEY and CCGUI_SSH_LIVE_HOST must identify an authorized host.
use ccgui_next_lib::config::ConfigStore;
use ccgui_next_lib::db::Db;
use ccgui_next_lib::engine::{self, ProcessRegistry};
use ccgui_next_lib::event_sink::{EventSink, ENGINE_EVENT_NAME};
use ccgui_next_lib::AppState;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{Listener, Manager};
fn build_app(
    home: &std::path::Path,
) -> (tauri::App<tauri::test::MockRuntime>, Arc<Mutex<Vec<Value>>>) {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = AppState {
        db: Arc::new(Db::open_at(&home.join("app.db")).unwrap()),
        sink: EventSink::new(Arc::new(app.handle().clone())),
        terminal_sink: EventSink::with_name(
            Arc::new(app.handle().clone()),
            ccgui_next_lib::terminal::TERMINAL_OUTPUT_EVENT,
        ),
        plugin_sink: EventSink::with_name(
            Arc::new(app.handle().clone()),
            ccgui_next_lib::event_sink::PLUGIN_AGENT_EVENT_NAME,
        ),
        mission_sink: EventSink::with_name(
            Arc::new(app.handle().clone()),
            ccgui_next_lib::event_sink::MISSION_AGENT_EVENT_NAME,
        ),
        terminals: ccgui_next_lib::terminal::TerminalRegistry::default(),
        processes: Arc::new(ProcessRegistry::default()),
        emitters: ccgui_next_lib::event_sink::BroadcastEmit::new(Arc::new(app.handle().clone())),
        web: ccgui_next_lib::web::WebAccessState::default(),
        relay: ccgui_next_lib::relay::RelayState::default(),
        dsh_host: Arc::new(ccgui_next_lib::dsh_host::DshHostState::default()),
        opencode_server: std::sync::Arc::new(
            ccgui_next_lib::engine::opencode_server::OpencodeServerState::default(),
        ),
        worktree_creations: ccgui_next_lib::git_worktree::CreationRegistry::default(),
    };
    app.manage(Arc::clone(&state.db));
    app.manage(state);
    app.manage(ConfigStore::default());

    let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let handle = app.handle().clone();
    handle.listen_any(ENGINE_EVENT_NAME, move |event: tauri::Event| {
        if let Ok(batch) = serde_json::from_str::<Vec<Value>>(event.payload()) {
            captured.lock().unwrap().extend(batch);
        }
    });
    (app, events)
}

#[tokio::test]
#[ignore = "requires explicitly configured live SSH credentials and uses Claude"]
async fn live_ssh_claude_stream_history_and_terminal() {
    use ccgui_next_lib::{engine::remote_workspace, history::reader, terminal};
    use std::time::{Duration, Instant};
    assert!(
        cfg!(feature = "remote-vps"),
        "live test requires isolated remote-vps profile"
    );
    let key = std::env::var("CCGUI_SSH_LIVE_KEY").expect("set CCGUI_SSH_LIVE_KEY");
    let host = std::env::var("CCGUI_SSH_LIVE_HOST").expect("set CCGUI_SSH_LIVE_HOST");
    let user = std::env::var("CCGUI_SSH_LIVE_USER").unwrap_or_else(|_| "cc".into());
    let workspace =
        std::env::var("CCGUI_SSH_LIVE_WORKSPACE").expect("set CCGUI_SSH_LIVE_WORKSPACE");
    let scratch = std::env::temp_dir().join(format!("ccgui-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).unwrap();
    let (app, events) = build_app(&scratch);
    let connection = remote_workspace::SshConnectionInput {
        host,
        user,
        port: 22,
        identity_file: Some(key),
    };
    let probe = remote_workspace::probe_ssh_connection(connection.clone(), Some(workspace.clone()))
        .await
        .unwrap();
    assert!(probe.claude_available);
    remote_workspace::add_ssh_workspace(app.state(), connection, workspace.clone())
        .await
        .unwrap();
    assert!(
        !std::path::Path::new(&workspace).exists(),
        "test workspace must exist only remotely"
    );

    let marker = format!("CCGUI_REMOTE_VERIFIED_{}", uuid::Uuid::new_v4().simple());
    let sent = engine::send_message(
        app.state(),
        "claude".into(),
        workspace.clone(),
        None,
        format!("Reply with exactly {marker}. Do not use tools."),
        Vec::new(),
        None,
        None,
        None,
        None,
        Some("manual".into()),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let (session_id, delta) = loop {
        let collected = events.lock().unwrap().clone();
        let error = collected
            .iter()
            .find(|e| e["runId"] == sent.run_id && e["kind"] == "error");
        assert!(error.is_none(), "remote send error: {error:?}");
        if let Some(done) = collected
            .iter()
            .find(|e| e["runId"] == sent.run_id && e["kind"] == "done")
        {
            let delta: String = collected
                .iter()
                .filter(|e| e["runId"] == sent.run_id && e["kind"] == "delta")
                .filter_map(|e| e["data"].as_str())
                .collect();
            break (done["sessionId"].as_str().unwrap().to_string(), delta);
        }
        assert!(
            Instant::now() < deadline,
            "no remote done event: {collected:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(delta.contains(&marker), "stream text: {delta:?}");
    let sessions = remote_workspace::list_ssh_sessions(app.state(), workspace.clone())
        .await
        .unwrap();
    let row = sessions
        .iter()
        .find(|s| s.session_id == session_id)
        .expect("new conversation present in SSH history");
    let page = reader::load_remote_session_page(
        app.state(),
        workspace.clone(),
        "claude".into(),
        session_id.clone(),
        row.remote_path.clone().unwrap(),
        Some(100),
        None,
    )
    .await
    .unwrap();
    assert!(
        serde_json::to_string(&page).unwrap().contains(&marker),
        "transcript contains response"
    );

    // A manual Write must pause for an explicit one-time grant. The generated
    // verification file is read back and moved to remote Trash afterwards.
    let approval_marker = format!("CCGUI_APPROVAL_{}", uuid::Uuid::new_v4().simple());
    let approval_file = format!(
        "{workspace}/.ccgui-approval-{}.txt",
        uuid::Uuid::new_v4().simple()
    );
    let approval = engine::send_message(
        app.state(), "claude".into(), workspace.clone(), Some(session_id.clone()),
        format!("Use only the Write tool to create {approval_file} with exactly this content: {approval_marker}. After it succeeds reply DONE. Do not use any other tools."),
        Vec::new(), None, None, None, None, Some("manual".into()), None, None, None, None,
    ).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut approved = false;
    loop {
        let collected = events.lock().unwrap().clone();
        if !approved {
            if let Some(ask) = collected.iter().find(|event| {
                event["runId"] == approval.run_id && event["kind"] == "tool_permission"
            }) {
                assert_eq!(ask["data"]["toolName"], "Write");
                assert!(ask["data"]["inputJson"]
                    .as_str()
                    .unwrap()
                    .contains(&approval_marker));
                engine::answer_question(
                    app.state(),
                    approval.run_id.clone(),
                    ask["data"]["requestId"].as_str().unwrap().into(),
                    Some(serde_json::json!({"__ccgui_tool_permission":"allow_once"})),
                )
                .await
                .unwrap();
                approved = true;
            }
        }
        let error = collected
            .iter()
            .find(|event| event["runId"] == approval.run_id && event["kind"] == "error");
        assert!(error.is_none(), "approval run error: {error:?}");
        if collected
            .iter()
            .any(|event| event["runId"] == approval.run_id && event["kind"] == "done")
        {
            assert!(
                approved,
                "manual Write did not ask for approval: {collected:?}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tool approval timed out: {collected:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let written = ccgui_next_lib::files::read_file(app.state(), approval_file.clone())
        .await
        .unwrap();
    assert_eq!(written.text.as_deref(), Some(approval_marker.as_str()));
    ccgui_next_lib::files::trash_item(app.state(), approval_file)
        .await
        .unwrap();

    let output: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&output);
    app.handle()
        .listen_any(terminal::TERMINAL_OUTPUT_EVENT, move |event| {
            if let Ok(batch) = serde_json::from_str::<Vec<Value>>(event.payload()) {
                captured.lock().unwrap().extend(batch);
            }
        });
    terminal::terminal_open("ssh-live".into(), workspace.clone(), 80, 24, app.state())
        .await
        .unwrap();
    terminal::terminal_write(
        "ssh-live".into(),
        "printf 'REMOTE-PWD:'; pwd; printf 'REMOTE-USER:'; whoami\n".into(),
        app.state(),
    )
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let received: String = output
            .lock()
            .unwrap()
            .iter()
            .filter_map(|row| row["data"].as_str())
            .collect();
        if received.contains(&format!("REMOTE-PWD:{workspace}"))
            && received.contains("REMOTE-USER:cc")
        {
            break;
        }
        assert!(Instant::now() < deadline, "terminal output: {received:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    terminal::terminal_resize("ssh-live".into(), 120, 40, app.state())
        .await
        .unwrap();
    terminal::terminal_close("ssh-live".into(), app.state())
        .await
        .unwrap();
    eprintln!("VERIFIED: SSH Claude streamed response, manual tool approval, remote history replay and remote PTY; session {session_id}");
    std::fs::remove_dir_all(scratch).unwrap();
}
