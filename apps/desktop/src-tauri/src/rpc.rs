//! Only initialize, account/read and account/rateLimits/read are sent on this connection.
use pulse_core::quota::{QuotaBucket, normalize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub type ActiveChild = Arc<Mutex<Option<Child>>>;
#[cfg(test)]
thread_local! {
    static LIFECYCLE_MARKS: std::cell::RefCell<Vec<(&'static str, Instant)>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}
#[cfg(test)]
fn lifecycle_mark(phase: &'static str) {
    LIFECYCLE_MARKS.with(|marks| marks.borrow_mut().push((phase, Instant::now())));
}
#[cfg(test)]
mod command_tests {
    use super::*;
    /// Explicit, opt-in live benchmark of the production refresh path. No auth files are read
    /// by this test and no account response / quota values are written to its report.
    #[test]
    #[ignore = "live signed-in CLI query; run explicitly with PULSE_BENCH_REPORT"]
    fn measure_live_refresh_lifecycle() {
        let report = std::env::var_os("PULSE_BENCH_REPORT")
            .expect("set PULSE_BENCH_REPORT to an ignored local output path");
        let windows_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE")).join(".codex")
            });
        let mut scopes = vec![Scope {
            source_id: "windows".into(),
            home: windows_home,
            wsl: None,
        }];
        if let Some(home) = std::env::var_os("PULSE_BENCH_WSL_HOME") {
            scopes.push(Scope {
                source_id: "wsl".into(),
                home: PathBuf::from(home),
                wsl: Some(crate::source_config::WslTarget {
                    distro: std::env::var("PULSE_BENCH_WSL_DISTRO").expect("WSL distro"),
                    user: std::env::var("PULSE_BENCH_WSL_USER").unwrap_or_default(),
                }),
            });
        }
        let stopping = AtomicBool::new(false);
        let active = Arc::new(Mutex::new(None));
        let mut samples = Vec::new();
        // The network worker refreshes sources sequentially, just like this loop.
        for round in 1..=3 {
            let batch_start = Instant::now();
            for scope in &scopes {
                LIFECYCLE_MARKS.with(|marks| marks.borrow_mut().clear());
                let started = Instant::now();
                let result = read(scope, &stopping, &active);
                let finished = Instant::now();
                let elapsed_ms = finished.duration_since(started).as_secs_f64() * 1000.0;
                let mut last = started;
                let mut phases = serde_json::Map::new();
                LIFECYCLE_MARKS.with(|marks| {
                    for (phase, instant) in marks.borrow_mut().drain(..) {
                        phases.insert(
                            phase.into(),
                            json!(instant.duration_since(last).as_secs_f64() * 1000.0),
                        );
                        last = instant;
                    }
                });
                phases.insert(
                    "cleanup".into(),
                    json!(finished.duration_since(last).as_secs_f64() * 1000.0),
                );
                let status = match result {
                    Ok(buckets) if !buckets.is_empty() => "success".to_owned(),
                    Ok(_) => "empty_quota".to_owned(),
                    Err(code) => code,
                };
                assert!(active.lock().unwrap().is_none(), "own child was not reaped");
                println!(
                    "{} round {round}: {elapsed_ms:.2} ms, {status}",
                    scope.source_id
                );
                samples.push(json!({
                    "source": scope.source_id,
                    "round": round,
                    "elapsed_ms": elapsed_ms,
                    "status": status,
                    "child_reaped": true,
                    "phases_ms": phases,
                }));
            }
            let elapsed_ms = batch_start.elapsed().as_secs_f64() * 1000.0;
            println!("all sources round {round}: {elapsed_ms:.2} ms");
            samples.push(json!({"source":"all", "round":round,"elapsed_ms":elapsed_ms}));
            if round != 3 {
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        fs::write(report, serde_json::to_vec_pretty(&json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "rounds": 3,
            "includes": "source readiness, launch, initialize, account/read, rateLimits/read, normalize, kill/wait",
            "samples": samples,
        })).unwrap()).unwrap();
    }
    #[test]
    fn configured_wsl_user_and_home_are_separate_arguments_in_the_expected_distro() {
        let target = crate::source_config::WslTarget {
            distro: "Ubuntu".into(),
            user: "dev".into(),
        };
        let cmd = wsl_command(
            &target,
            Path::new(r"\\wsl.localhost\Ubuntu\home\dev\codex data"),
        )
        .unwrap();
        let args = cmd
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "-d",
                "Ubuntu",
                "--user",
                "dev",
                "--exec",
                "/bin/sh",
                "-c",
                WSL_START_SCRIPT,
                "codexpulse",
                "/home/dev/codex data"
            ]
        );
        assert!(wsl_command(&target, Path::new(r"\\wsl.localhost\UbuntuOther\home\dev")).is_err());
        assert!(wsl_command(&target, Path::new(r"\\wsl.localhost\Ubuntu\..\Other\home")).is_err());
    }
}
// WSL --exec does not load the user's shell environment. In particular, NVM installs
// may exist only in the default shell's interactive PATH. Keep all user input in argv;
// apply the selected CODEX_HOME after shell startup so profiles cannot change its scope.
const WSL_START_SCRIPT: &str =
    r#"exec "${SHELL:-/bin/sh}" -lic 'exec env CODEX_HOME="$1" codex app-server' codexpulse "$1""#;
fn wsl_command(target: &crate::source_config::WslTarget, path: &Path) -> Result<Command, String> {
    let prefix = format!("\\\\wsl.localhost\\{}", target.distro);
    let home = path
        .to_string_lossy()
        .strip_prefix(&prefix)
        .ok_or("invalid_wsl_home")?
        .replace('\\', "/");
    if !crate::source_config::valid_linux_home(&home) {
        return Err("invalid_wsl_home".into());
    }
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-d", &target.distro]);
    if !target.user.is_empty() {
        cmd.args(["--user", &target.user]);
    }
    cmd.args([
        "--exec",
        "/bin/sh",
        "-c",
        WSL_START_SCRIPT,
        "codexpulse",
        &home,
    ]);
    Ok(cmd)
}
#[derive(Clone)]
pub struct Scope {
    pub source_id: String,
    pub home: PathBuf,
    pub wsl: Option<crate::source_config::WslTarget>,
}
pub fn read(
    scope: &Scope,
    stopping: &AtomicBool,
    active: &ActiveChild,
) -> Result<Vec<QuotaBucket>, String> {
    let mut cmd = if let Some(target) = &scope.wsl {
        let distro = &target.distro;
        if !crate::platform::wsl_is_running(distro) {
            return Err("wsl_stopped".into());
        }
        wsl_command(target, &scope.home)?
    } else {
        let mut cmd = Command::new(codex_executable().ok_or("codex_not_found")?);
        cmd.arg("app-server").env("CODEX_HOME", &scope.home);
        cmd
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(test)]
    lifecycle_mark("resolve_and_readiness");
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "codex_launch_failed")?;
    #[cfg(test)]
    lifecycle_mark("spawn");
    let mut stdin = child.stdin.take().ok_or("rpc_stdin")?;
    let stdout = child.stdout.take().ok_or("rpc_stdout")?;
    *active.lock().map_err(|_| "child_lock")? = Some(child);
    let _guard = ChildGuard(active.clone());
    let (sender, receiver) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut reader = BufReader::with_capacity(64 * 1024, stdout);
        loop {
            let mut bytes = Vec::new();
            // Reject a malformed/unsupported server response at 1 MiB, instead of retaining
            // arbitrary stdout or printing messages that could contain account information.
            use std::io::Read;
            match reader
                .by_ref()
                .take(1024 * 1024 + 1)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if bytes.len() > 1024 * 1024 {
                        break;
                    }
                    if let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                        && value.get("id").is_some()
                        && sender.try_send(value).is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut call = |id: u32, method: &str, params: Value| -> Result<Value, String> {
        let message = if method == "initialized" {
            json!({"method":method})
        } else {
            json!({"id":id,"method":method,"params":params})
        };
        let body = serde_json::to_vec(&message).map_err(|_| "rpc_encode")?;
        stdin
            .write_all(&body)
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|_| "rpc_write")?;
        if method == "initialized" {
            return Ok(Value::Null);
        }
        loop {
            if stopping.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            if Instant::now() >= deadline {
                return Err("quota_timeout".into());
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(value) if value["id"].as_u64() == Some(id as u64) => {
                    if value.get("error").is_some() {
                        return Err(format!(
                            "rpc_error_{}",
                            value["error"]["code"].as_i64().unwrap_or(0)
                        ));
                    }
                    return value
                        .get("result")
                        .cloned()
                        .ok_or_else(|| "rpc_missing_result".into());
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("codex_disconnected".into()),
            }
        }
    };
    call(
        1,
        "initialize",
        json!({"clientInfo":{"name":"codexpulse","title":"CodexPulse","version":"0.1.0"},"capabilities":{"experimentalApi":false}}),
    )?;
    call(0, "initialized", Value::Null)?;
    #[cfg(test)]
    lifecycle_mark("initialize");
    // No turn or auth mutation is initiated; this process uses the source environment's existing sign-in.
    let account = call(2, "account/read", json!({"refreshToken":false}))?;
    #[cfg(test)]
    lifecycle_mark("account_read");
    if account["account"].is_null() {
        return Err("not_signed_in".into());
    }
    let response = call(
        3,
        "account/rateLimits/read",
        json!({"excludeResetCreditDetails":true,"supportsLunaReserve":false}),
    )?;
    #[cfg(test)]
    lifecycle_mark("rate_limits_read");
    let buckets = normalize(
        &scope.source_id,
        &account,
        &response,
        &chrono::Utc::now().to_rfc3339(),
    );
    #[cfg(test)]
    lifecycle_mark("normalize");
    Ok(buckets)
}
struct ChildGuard(ActiveChild);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        stop_child(&self.0);
    }
}
pub fn stop_child(active: &ActiveChild) {
    if let Ok(mut slot) = active.lock()
        && let Some(mut child) = slot.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}
fn codex_executable() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("OpenAI/Codex/bin");
    let mut files = fs::read_dir(base)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path().join("codex.exe"))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    files.sort_by_key(|p| p.metadata().ok().and_then(|m| m.modified().ok()));
    files.pop().or_else(|| {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join("codex.exe"))
                .find(|p| Path::new(p).is_file())
        })
    })
}
