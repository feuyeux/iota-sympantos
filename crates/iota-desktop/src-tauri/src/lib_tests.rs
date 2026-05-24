use super::*;
use iota_core::daemon::{DESKTOP_PROTOCOL_VERSION, DaemonClientMessage};

#[test]
fn desktop_hello_uses_current_protocol_version() {
    let message = daemon_client::hello_message();
    assert!(matches!(
        message,
        DaemonClientMessage::Hello {
            protocol_version: DESKTOP_PROTOCOL_VERSION,
            ..
        }
    ));
}

#[test]
fn test_get_memory_context_snapshot_message_building() {
    let cwd = PathBuf::from("/tmp/workspace");
    let message = DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: cwd.clone(),
        scope_mode: iota_core::daemon::DesktopMemoryScopeMode::Workspace,
    };
    if let DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: path,
        scope_mode,
    } = message
    {
        assert_eq!(path, cwd);
        assert_eq!(
            scope_mode,
            iota_core::daemon::DesktopMemoryScopeMode::Workspace
        );
    } else {
        panic!("expected GetMemoryContextSnapshot message");
    }
}
