use super::*;

#[cfg(unix)]
#[test]
fn run_command_times_out_without_waiting_for_child_completion() {
    let started = Instant::now();
    let err = run_command(
        "sh",
        &[OsString::from("-c"), OsString::from("sleep 5")],
        None,
        100,
    )
    .unwrap_err();

    assert!(err.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(windows)]
#[test]
fn run_command_times_out_without_waiting_for_child_completion() {
    let started = Instant::now();
    let err = run_command(
        "cmd",
        &[
            OsString::from("/C"),
            OsString::from("ping -n 6 127.0.0.1 >NUL"),
        ],
        None,
        100,
    )
    .unwrap_err();

    assert!(err.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}
