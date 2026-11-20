//! Session-scoped Claude PermissionRequest hook bridge. The CLI asks via its
//! documented hook protocol; a request-scoped local socket forwards the choice
//! to ApprovalRegistry and returns that decision to the SAME CLI process.
#![cfg(unix)]
use std::{fs, io::{Read, Write}, os::unix::net::{UnixListener, UnixStream},
    path::PathBuf, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
    thread, time::Duration};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

pub struct HookGateway {
    pub path: PathBuf,
    pub token: String,
    alive: Arc<AtomicBool>,
}
fn secret() -> Result<String, String> {
    use std::io::Read as _;
    let mut bytes = [0u8; 24];
    fs::File::open("/dev/urandom").map_err(|e| e.to_string())?
        .read_exact(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn handle(mut socket: UnixStream, app: AppHandle, request_id: String, token: String, sequence: Arc<AtomicU64>) {
    let _ = socket.set_read_timeout(Some(Duration::from_secs(125)));
    let _ = socket.set_write_timeout(Some(Duration::from_secs(5)));
    let mut buffer = vec![0u8; 65_536];
    let n = match socket.read(&mut buffer) { Ok(0) | Err(_) => return, Ok(n) => n };
    let parsed: Value = match serde_json::from_slice(&buffer[..n]) { Ok(v) => v, Err(_) => return };
    if parsed.get("token").and_then(Value::as_str) != Some(token.as_str()) { return; }
    let tool = parsed.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let input = parsed.get("input").cloned().unwrap_or(json!({}));
    let id = format!("{request_id}-claude-{}", sequence.fetch_add(1, Ordering::Relaxed));
    let approvals = app.state::<super::approval::ApprovalRegistry>();
    let rx = match approvals.register(id.clone(), request_id.clone()) {
        Ok(rx) => rx,
        Err(_) => return,
    };
    let preview = if tool == "Bash" { input.get("command").and_then(Value::as_str).unwrap_or("") }
        else { input.get("file_path").and_then(Value::as_str).unwrap_or("") };
    let _ = app.emit("aam-event", json!({"requestId":request_id,"data":{
        "kind":"approval_request","provider":"claude","approvalId":id,
        "description":format!("Claude requests permission for {tool}"),
        "command":preview,
    }}));
    let (tx, wait) = std::sync::mpsc::channel();
    thread::spawn(move || { let _ = tx.send(rx.blocking_recv().unwrap_or(false)); });
    let allow = wait.recv_timeout(Duration::from_secs(120)).unwrap_or(false);
    let _ = socket.write_all(if allow { b"{\"allow\":true}\n" } else { b"{\"allow\":false}\n" });
    let _ = socket.flush();
}

impl HookGateway {
    pub fn start(app: &AppHandle, request_id: &str) -> Result<Self, String> {
        let token = secret()?;
        let path = std::env::temp_dir().join(format!("aam-approval-{}-{}.sock", std::process::id(), &token[..20]));
        let listener = UnixListener::bind(&path).map_err(|e| format!("Approval listener: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let alive = Arc::new(AtomicBool::new(true));
        let running = alive.clone();
        let owner = app.clone();
        let run = request_id.to_owned();
        let auth = token.clone();
        thread::spawn(move || {
            let counter = Arc::new(AtomicU64::new(1));
            while running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let app = owner.clone();
                        let id = run.clone();
                        let secret = auth.clone();
                        let seq = counter.clone();
                        thread::spawn(move || handle(client, app, id, secret, seq));
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock =>
                        thread::sleep(Duration::from_millis(40)),
                    Err(_) => break,
                }
            }
        });
        Ok(Self { path, token, alive })
    }
}
impl Drop for HookGateway {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        let _ = fs::remove_file(&self.path);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_use_os_randomness_and_have_expected_length() {
        let a = secret().unwrap();
        let b = secret().unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }
}
