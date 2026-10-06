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
#[derive(Clone)]
pub struct Scope {
    pub source_id: String,
    pub home: PathBuf,
}
pub struct Request {
    pub scopes: Vec<Scope>,
}
pub struct ResultSet {
    pub source_id: String,
    pub result: Result<Vec<QuotaBucket>, String>,
}
pub fn read(
    scope: &Scope,
    stopping: &AtomicBool,
    active: &ActiveChild,
) -> Result<Vec<QuotaBucket>, String> {
    let mut cmd = if let Some(distro) = scope.source_id.strip_prefix("wsl:") {
        if !crate::platform::wsl_is_running(distro) {
            return Err("wsl_stopped".into());
        }
        let prefix = format!("\\\\wsl.localhost\\{distro}");
        let path = scope.home.to_string_lossy();
        let home = path
            .strip_prefix(&prefix)
            .ok_or("invalid_wsl_home")?
            .replace('\\', "/");
        let mut cmd = Command::new("wsl.exe");
        cmd.args([
            "-d",
            distro,
            "--exec",
            "env",
            &format!("CODEX_HOME={home}"),
            "codex",
            "app-server",
        ]);
        cmd
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
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "codex_launch_failed")?;
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
    // No turn or auth mutation is initiated; this process uses the source environment's existing sign-in.
    let account = call(2, "account/read", json!({"refreshToken":false}))?;
    if account["account"].is_null() {
        return Err("not_signed_in".into());
    }
    let response = call(
        3,
        "account/rateLimits/read",
        json!({"excludeResetCreditDetails":true,"supportsLunaReserve":false}),
    )?;
    Ok(normalize(
        &scope.source_id,
        &account,
        &response,
        &chrono::Utc::now().to_rfc3339(),
    ))
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
