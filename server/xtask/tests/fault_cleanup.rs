//! 使用替身 Docker 命令验证故障演练的恢复路径，不停止真实服务。

#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("silva-fault-test-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&directory).unwrap();
        let docker = directory.join("docker");
        fs::write(
            &docker,
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$SILVA_XTASK_TEST_LOG"
case "$*" in
  *" ps "*) printf '%s\n' worker postgres api prometheus alertmanager alert-receiver ;;
  *" stop "*) test "${SILVA_XTASK_TEST_FAIL_STOP:-0}" = 0 ;;
  *" start postgres") test "${SILVA_XTASK_TEST_FAIL_RESTORE:-0}" = 0 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(docker, fs::Permissions::from_mode(0o755)).unwrap();
        Self { directory }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_silva-xtask"));
        command
            .arg("fault-smoke")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.directory.display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("SILVA_XTASK_TEST_LOG", self.directory.join("compose.log"))
            .env("SILVA_SMOKE_API", "http://127.0.0.1:1")
            .env("SILVA_XTASK_TEST_FAIL_STOP", "0")
            .env("SILVA_XTASK_TEST_FAIL_RESTORE", "0");
        command
    }

    fn log(&self) -> String {
        fs::read_to_string(self.directory.join("compose.log")).unwrap_or_default()
    }

    fn assert_restored(&self) {
        let log = self.log();
        assert!(log.contains(" start postgres\n"), "{log}");
        assert!(log.contains(" start worker\n"), "{log}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn partially_failed_stop_restores_both_services() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .env("SILVA_XTASK_TEST_FAIL_STOP", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    fixture.assert_restored();
}

#[test]
fn failed_postgres_recovery_still_attempts_worker_recovery() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .env("SILVA_XTASK_TEST_FAIL_STOP", "1")
        .env("SILVA_XTASK_TEST_FAIL_RESTORE", "1")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("恢复失败"));
    fixture.assert_restored();
}

#[test]
fn termination_signal_restores_both_services() {
    let fixture = Fixture::new();
    let mut child = fixture
        .command()
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fixture.log().contains(" stop worker postgres\n") {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("故障演练没有进入停止阶段");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("故障演练收到信号后没有退出");
        }
        thread::sleep(Duration::from_millis(20));
    }
    fixture.assert_restored();
}
