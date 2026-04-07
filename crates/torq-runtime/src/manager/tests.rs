use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout};
use torq_core::{ControlAvailability, TorEvent};

use super::bootstrap::bootstrap_phase_event;
use super::TorManager;
use crate::config::{LogMode, TorRuntimeConfig};
use crate::control::{TorBootstrapPhase, TorControlAuth, TorControlConfig};

#[tokio::test]
async fn runtime_state_defaults_to_unconfigured_control_without_config() {
    let manager = TorManager::new(TorRuntimeConfig::new(
        "tor.exe",
        unique_test_path("idle-runtime-state.log"),
    ))
    .await
    .unwrap();

    let runtime_state = manager.current_runtime_state();
    assert_eq!(
        runtime_state.control().port(),
        ControlAvailability::Unconfigured
    );
    assert_eq!(
        runtime_state.control().bootstrap_observation(),
        ControlAvailability::Unconfigured
    );
    assert!(!runtime_state.control_configured());
    assert!(!runtime_state.new_identity_available());
    assert!(!runtime_state.uses_control_bootstrap_observation());
}

#[tokio::test]
async fn runtime_config_can_be_updated_while_stopped() {
    let initial_log_path = unique_test_path("config-stopped-initial.log");
    let manager = TorManager::new(
        TorRuntimeConfig::new("tor.exe", &initial_log_path)
            .with_stop_timeout(Duration::from_secs(5)),
    )
    .await
    .unwrap();

    let next_log_path = unique_test_path("config-stopped-next.log");
    let next_config = TorRuntimeConfig::new("tor.exe", &next_log_path)
        .with_torrc_path("C:/Tor/torrc")
        .with_use_torrc(true)
        .with_stop_timeout(Duration::from_secs(2))
        .with_log_poll_interval(Duration::from_millis(100));

    manager
        .set_runtime_config(next_config.clone())
        .await
        .unwrap();

    let current = manager.current_config();
    assert_eq!(current.tor_path, next_config.tor_path);
    assert_eq!(current.log_path, next_config.log_path);
    assert_eq!(current.log_mode, next_config.log_mode);
    assert_eq!(current.torrc_path, next_config.torrc_path);
    assert_eq!(current.use_torrc, next_config.use_torrc);
    assert_eq!(current.args, next_config.args);
    assert_eq!(current.working_dir, next_config.working_dir);
    assert_eq!(current.control, next_config.control);
    assert_eq!(current.stop_timeout, next_config.stop_timeout);
    assert_eq!(current.log_poll_interval, next_config.log_poll_interval);
}

#[tokio::test]
async fn runtime_config_rejects_empty_tor_path() {
    let manager = TorManager::new(TorRuntimeConfig::new(
        "tor.exe",
        unique_test_path("config-validation.log"),
    ))
    .await
    .unwrap();

    let invalid_config = TorRuntimeConfig::new("", unique_test_path("config-validation-next.log"));

    let error = manager
        .set_runtime_config(invalid_config)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("tor_path must not be empty"));
}

#[tokio::test]
async fn runtime_config_rejects_empty_control_host() {
    let manager = TorManager::new(TorRuntimeConfig::new(
        "tor.exe",
        unique_test_path("config-validation-control.log"),
    ))
    .await
    .unwrap();

    let invalid_config = TorRuntimeConfig::new(
        "tor.exe",
        unique_test_path("config-validation-control-next.log"),
    )
    .with_control(TorControlConfig::new(" ", 9051, TorControlAuth::Null));

    let error = manager
        .set_runtime_config(invalid_config)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("control.host must not be empty"));
}

#[tokio::test]
async fn new_identity_without_running_tor_emits_warning() {
    let manager = TorManager::new(TorRuntimeConfig::new(
        "tor.exe",
        unique_test_path("idle.log"),
    ))
    .await
    .unwrap();
    let mut events = manager.subscribe_events();

    manager.new_identity().await.unwrap();

    assert_eq!(
        recv_matching_event(&mut events, |event| matches!(
            event,
            TorEvent::Warning(message) if message == "tor is not running"
        ))
        .await,
        TorEvent::Warning("tor is not running".to_string())
    );
}

#[tokio::test]
async fn new_identity_without_control_config_emits_warning() {
    let script = mock_tor_script_path();
    let log_path = unique_test_path("missing-control.log");
    let config = TorRuntimeConfig::new("cmd.exe", &log_path)
        .with_args(["/C", script.to_string_lossy().as_ref()])
        .with_stop_timeout(Duration::from_secs(5));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

    manager.new_identity().await.unwrap();

    assert_eq!(
        recv_matching_event(&mut events, |event| matches!(
            event,
            TorEvent::Warning(message)
                if message == "new identity requires ControlPort configuration"
        ))
        .await,
        TorEvent::Warning("new identity requires ControlPort configuration".to_string())
    );
    assert_eq!(
        manager.current_runtime_state().control().port(),
        ControlAvailability::Unconfigured
    );
    assert!(!manager.current_runtime_state().new_identity_available());

    manager.stop().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
    cleanup(&log_path).await;
}

#[tokio::test]
async fn runtime_config_update_is_rejected_while_running() {
    let script = mock_tor_script_path();
    let log_path = unique_test_path("config-running.log");
    let config = TorRuntimeConfig::new("cmd.exe", &log_path)
        .with_args(["/C", script.to_string_lossy().as_ref()])
        .with_stop_timeout(Duration::from_secs(5));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Bootstrap(100)).await;

    assert_eq!(
        manager.current_runtime_state().tor().status(),
        torq_core::RuntimeStatus::Running
    );

    let next_config =
        TorRuntimeConfig::new("cmd.exe", unique_test_path("config-while-running.log"));
    let error = manager.set_runtime_config(next_config).await.unwrap_err();

    assert!(error
        .to_string()
        .contains("runtime config cannot be changed while Tor is Starting or Running"));
    assert_eq!(manager.current_config().log_path, log_path);

    manager.stop().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
    cleanup(&log_path).await;
}

#[tokio::test]
async fn new_identity_success_emits_identity_renewed() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut tasks = Vec::new();

        for _ in 0..2 {
            let (socket, _) = listener.accept().await.unwrap();
            tasks.push(tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(socket);
                let mut line = String::new();

                reader.read_line(&mut line).await.unwrap();
                assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
                reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();

                line.clear();
                reader.read_line(&mut line).await.unwrap();
                match line.trim_end_matches(['\r', '\n']) {
                    "GETINFO status/bootstrap-phase" => {
                        reader
                            .get_mut()
                            .write_all(
                                b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\"\r\n250 OK\r\n",
                            )
                            .await
                            .unwrap();
                    }
                    "SIGNAL NEWNYM" => {
                        reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();
                    }
                    other => panic!("unexpected control command: {other}"),
                }
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }
    });

    let script = mock_tor_script_path();
    let log_path = unique_test_path("identity-renewed.log");
    let control = TorControlConfig::new(
        address.ip().to_string(),
        address.port(),
        TorControlAuth::Null,
    );
    let config = TorRuntimeConfig::new("cmd.exe", &log_path)
        .with_args(["/C", script.to_string_lossy().as_ref()])
        .with_control(control)
        .with_stop_timeout(Duration::from_secs(5));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

    manager.new_identity().await.unwrap();

    assert_eq!(
        recv_matching_event(&mut events, |event| *event == TorEvent::IdentityRenewed).await,
        TorEvent::IdentityRenewed
    );
    assert_eq!(
        manager.current_runtime_state().control().port(),
        ControlAvailability::Available
    );
    assert!(manager.current_runtime_state().new_identity_available());

    manager.stop().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
    server.await.unwrap();
    cleanup(&log_path).await;
}

#[test]
fn bootstrap_phase_event_only_emits_forward_progress() {
    assert_eq!(
        bootstrap_phase_event(
            None,
            &TorBootstrapPhase {
                progress: 15,
                tag: Some("conn".to_string()),
                summary: None,
            }
        ),
        Some(TorEvent::Bootstrap(15))
    );
    assert_eq!(
        bootstrap_phase_event(
            Some(15),
            &TorBootstrapPhase {
                progress: 15,
                tag: Some("conn".to_string()),
                summary: None,
            }
        ),
        None
    );
    assert_eq!(
        bootstrap_phase_event(
            Some(40),
            &TorBootstrapPhase {
                progress: 20,
                tag: Some("conn".to_string()),
                summary: None,
            }
        ),
        None
    );
    assert_eq!(
        bootstrap_phase_event(
            Some(40),
            &TorBootstrapPhase {
                progress: 41,
                tag: Some("conn".to_string()),
                summary: None,
            }
        ),
        Some(TorEvent::Bootstrap(41))
    );
}

#[tokio::test]
async fn control_bootstrap_observer_emits_bootstrap_event() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut reader = tokio::io::BufReader::new(socket);
        let mut line = String::new();

        reader.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
        reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            line.trim_end_matches(['\r', '\n']),
            "GETINFO status/bootstrap-phase"
        );
        reader
            .get_mut()
            .write_all(
                b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=25 TAG=starting SUMMARY=\"Starting\"\r\n250 OK\r\n",
            )
            .await
            .unwrap();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }

            if line.trim_end_matches(['\r', '\n']) == "GETINFO status/bootstrap-phase"
                && reader
                    .get_mut()
                    .write_all(
                        b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=25 TAG=starting SUMMARY=\"Starting\"\r\n250 OK\r\n",
                    )
                    .await
                    .is_err()
            {
                break;
            }
        }
    });

    let log_path = unique_test_path("control-bootstrap-observer.log");
    let control = TorControlConfig::new(
        address.ip().to_string(),
        address.port(),
        TorControlAuth::Null,
    );
    let config = TorRuntimeConfig::new("powershell.exe", &log_path)
        .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
        .with_log_mode(LogMode::External)
        .with_control(control)
        .with_stop_timeout(Duration::from_secs(5));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

    assert_eq!(
        recv_matching_event(&mut events, |event| *event == TorEvent::Bootstrap(25)).await,
        TorEvent::Bootstrap(25)
    );
    assert_eq!(
        manager.current_runtime_state().control().port(),
        ControlAvailability::Available
    );
    assert_eq!(
        manager
            .current_runtime_state()
            .control()
            .bootstrap_observation(),
        ControlAvailability::Available
    );
    assert!(manager
        .current_runtime_state()
        .uses_control_bootstrap_observation());

    manager.stop().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
    server.await.unwrap();
    cleanup(&log_path).await;
}

#[tokio::test]
async fn bootstrap_observer_failure_marks_control_unavailable() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let log_path = unique_test_path("control-bootstrap-unavailable.log");
    let control = TorControlConfig::new(
        address.ip().to_string(),
        address.port(),
        TorControlAuth::Null,
    );
    let config = TorRuntimeConfig::new("powershell.exe", &log_path)
        .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
        .with_log_mode(LogMode::External)
        .with_control(control)
        .with_stop_timeout(Duration::from_secs(5));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;
    let _ = recv_matching_event(&mut events, |event| {
        matches!(
            event,
            TorEvent::Warning(message)
                if message.starts_with("bootstrap observation via ControlPort is unavailable:")
        )
    })
    .await;

    assert_eq!(
        manager.current_runtime_state().control().port(),
        ControlAvailability::Unavailable
    );
    assert_eq!(
        manager
            .current_runtime_state()
            .control()
            .bootstrap_observation(),
        ControlAvailability::Unavailable
    );
    assert!(!manager.current_runtime_state().new_identity_available());
    assert!(!manager
        .current_runtime_state()
        .uses_control_bootstrap_observation());

    manager.stop().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
    cleanup(&log_path).await;
}

#[tokio::test]
async fn stop_completes_even_when_control_observer_is_stuck() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        sleep(Duration::from_secs(30)).await;
    });

    let log_path = unique_test_path("control-bootstrap-stuck.log");
    let control = TorControlConfig::new(
        address.ip().to_string(),
        address.port(),
        TorControlAuth::Null,
    );
    let config = TorRuntimeConfig::new("powershell.exe", &log_path)
        .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
        .with_log_mode(LogMode::External)
        .with_control(control)
        .with_stop_timeout(Duration::from_millis(750));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

    timeout(Duration::from_secs(5), manager.stop())
        .await
        .expect("stop should complete even if the control observer is hung")
        .unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;

    server.abort();
    cleanup(&log_path).await;
}

#[tokio::test]
async fn new_identity_failure_marks_control_unavailable() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (observer_socket, _) = listener.accept().await.unwrap();
        let mut observer = tokio::io::BufReader::new(observer_socket);
        let mut line = String::new();

        observer.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
        observer.get_mut().write_all(b"250 OK\r\n").await.unwrap();

        line.clear();
        observer.read_line(&mut line).await.unwrap();
        assert_eq!(
            line.trim_end_matches(['\r', '\n']),
            "GETINFO status/bootstrap-phase"
        );
        observer
            .get_mut()
            .write_all(
                b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=25 TAG=starting SUMMARY=\"Starting\"\r\n250 OK\r\n",
            )
            .await
            .unwrap();

        let (action_socket, _) = listener.accept().await.unwrap();
        let mut action = tokio::io::BufReader::new(action_socket);

        line.clear();
        action.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
        action.get_mut().write_all(b"250 OK\r\n").await.unwrap();

        line.clear();
        action.read_line(&mut line).await.unwrap();
        assert_eq!(line.trim_end_matches(['\r', '\n']), "SIGNAL NEWNYM");
        action
            .get_mut()
            .write_all(b"552 Unrecognized signal\r\n")
            .await
            .unwrap();
    });

    let log_path = unique_test_path("newnym-control-unavailable.log");
    let control = TorControlConfig::new(
        address.ip().to_string(),
        address.port(),
        TorControlAuth::Null,
    );
    let config = TorRuntimeConfig::new("powershell.exe", &log_path)
        .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
        .with_log_mode(LogMode::External)
        .with_control(control)
        .with_stop_timeout(Duration::from_secs(5));
    let manager = TorManager::new(config).await.unwrap();
    let mut events = manager.subscribe_events();

    manager.start().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Bootstrap(25)).await;
    assert_eq!(
        manager.current_runtime_state().control().port(),
        ControlAvailability::Available
    );

    manager.new_identity().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| {
        matches!(
            event,
            TorEvent::Error(message)
                if message.starts_with("failed to request new identity via ControlPort:")
        )
    })
    .await;

    assert_eq!(
        manager.current_runtime_state().control().port(),
        ControlAvailability::Unavailable
    );
    assert!(!manager.current_runtime_state().new_identity_available());

    manager.stop().await.unwrap();
    let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
    server.await.unwrap();
    cleanup(&log_path).await;
}

async fn recv_matching_event<F>(
    events: &mut broadcast::Receiver<TorEvent>,
    predicate: F,
) -> TorEvent
where
    F: Fn(&TorEvent) -> bool,
{
    timeout(Duration::from_secs(15), async {
        loop {
            match events.recv().await {
                Ok(event) if predicate(&event) => return event,
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => panic!("event channel closed"),
            }
        }
    })
    .await
    .expect("timed out while waiting for matching runtime event")
}

fn mock_tor_script_path() -> PathBuf {
    workspace_root().join("scripts").join("mock-tor.cmd")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

async fn cleanup(log_path: &Path) {
    let _ = tokio::fs::remove_file(log_path).await;
}

fn unique_test_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("torq-manager-{label}-{nonce}.log"))
}
