use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(0);

fn temporary_runtime_directory() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wlapse-cli-test-{}-{}",
        std::process::id(),
        NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).expect("create runtime directory");
    path
}

#[test]
fn reports_missing_runtime_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_wlapse"))
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run wlapse");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "wlapse: XDG_RUNTIME_DIR is not set\n"
    );
}

#[test]
fn prints_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_wlapse"))
        .arg("--help")
        .output()
        .expect("run wlapse");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "wlapse - A lightweight, on-demand stopwatch overlay for Wayland\n\nUsage:\n  wlapse\n  wlapse --help\n  wlapse --version\n\nRun without arguments to show or stop the stopwatch.\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn prints_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_wlapse"))
        .arg("--version")
        .output()
        .expect("run wlapse");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("wlapse {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_unknown_command_line_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_wlapse"))
        .arg("--unknown")
        .output()
        .expect("run wlapse");

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "wlapse: this command does not accept arguments\n"
    );
}

#[test]
fn reports_missing_persistent_state_directory() {
    let runtime = temporary_runtime_directory();
    let output = Command::new(env!("CARGO_BIN_EXE_wlapse"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env_remove("XDG_STATE_HOME")
        .env_remove("HOME")
        .output()
        .expect("run wlapse");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "wlapse: XDG_STATE_HOME and HOME do not provide an absolute state directory\n"
    );
    fs::remove_dir(runtime).expect("remove runtime directory");
}

#[test]
fn reports_invalid_color_configuration() {
    let runtime = temporary_runtime_directory();
    let config_home = runtime.join("config-home");
    let config_directory = config_home.join("wlapse");
    fs::create_dir_all(&config_directory).expect("create config directory");
    fs::write(config_directory.join("config"), "background_color = blue\n").expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_wlapse"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", runtime.join("state"))
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("run wlapse");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "wlapse: cannot load config: invalid config at line 1: color must start with '#'\n"
    );
    fs::remove_dir_all(runtime).expect("remove runtime directory");
}
