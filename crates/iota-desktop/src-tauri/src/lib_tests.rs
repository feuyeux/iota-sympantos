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
