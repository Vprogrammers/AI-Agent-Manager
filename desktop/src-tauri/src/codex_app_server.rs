//! Codex app-server JSON-RPC transport. Native approval requests are bridged
//! to the desktop's request-scoped, one-shot ApprovalRegistry.
use std::{path::Path, sync::Arc, time::Duration};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, sync::Notify};

fn notification(app: &AppHandle, request_id: &str, data: Value) {
    let _ = app.emit("aam-event", json!({"requestId": request_id, "data": data}));
}

fn approval_method(method: &str) -> bool {
    matches!(method,
        "item/commandExecution/requestApproval" |
        "item/fileChange/requestApproval" |
        "item/permissions/requestApproval")
}

fn approval_response(method: &str, params: &Value, allow: bool) -> Value {
    if method == "item/permissions/requestApproval" {
        // Never grant anything other than the subset explicitly requested.
        return json!({"permissions": if allow {params.get("permissions").cloned().unwrap_or(json!({}))} else {json!({})}});
    }
    json!({"decision": if allow {"accept"} else {"decline"}})
}

async fn send(writer: &mut tokio::process::ChildStdin, payload: Value) -> Result<(), String> {
    let encoded = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    writer.write_all(&encoded).await.map_err(|e| e.to_string())?;
    writer.write_all(b"\n").await.map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}

pub async fn run(
    app: &AppHandle, request_id: &str, root: &Path, prompt: &str,
    permission: &str, stop: Arc<Notify>
) -> Result<(), String> {
    let executable = super::find_cli("codex").ok_or("PRESTART: Codex CLI not installed")?;
    let args = ["app-server"];
    let mut command = super::sandbox_command(&executable, &args, root, permission, "codex")
        .map_err(|e| format!("PRESTART: {e}"))?;
    command.current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    super::isolate_process_group(&mut command);
    let mut child = command.spawn().map_err(|e| format!("PRESTART: Cannot spawn Codex app-server: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Missing Codex app-server stdin")?;
    let stdout = child.stdout.take().ok_or("Missing Codex app-server stdout")?;
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(_)) = lines.next_line().await {}
        });
    }
    let approvals = app.state::<super::approval::ApprovalRegistry>();
    let run = super::await_runtime(async {
        let mut lines = BufReader::new(stdout).lines();
        let mut thread_id: Option<String> = None;
        let mut full_answer = String::new();
        let mut delta_answer = String::new();
        let mut counter: u64 = 0;
        send(&mut stdin, json!({"id": 0, "method":"initialize", "params":{
            "clientInfo":{"name":"aam_desktop","title":"AI Agent Manager","version":"0.3.0"}
        }})).await?;
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            let message: Value = serde_json::from_str(&line).map_err(|e| format!("Invalid Codex protocol message: {e}"))?;
            if let Some(error) = message.get("error") {
                return Err(format!("Codex app-server rejected request: {}", error.get("message").and_then(Value::as_str).unwrap_or("unknown error")));
            }
            let id = &message["id"];
            if id == 0 {
                send(&mut stdin, json!({"method":"initialized","params":{}})).await?;
                send(&mut stdin, json!({"id":1,"method":"thread/start","params":{
                    "cwd":root.to_string_lossy(),
                    "approvalPolicy":"on-request",
                    // Outer Seatbelt/bwrap sandbox remains authoritative.
                    "sandbox": if permission == "full-access" {"danger-full-access"} else {"workspace-write"}
                }})).await?;
            } else if id == 1 {
                let thread = message.pointer("/result/thread/id").and_then(Value::as_str)
                    .ok_or("Codex did not return a thread ID")?.to_owned();
                thread_id = Some(thread.clone());
                send(&mut stdin, json!({"id":2,"method":"turn/start","params":{
                    "threadId":thread,
                    "input":[{"type":"text","text":prompt}],
                    "cwd":root.to_string_lossy(),
                    "approvalPolicy":"on-request",
                    // Avoid nested macOS Seatbelt: the app-server was launched inside
                    // the existing OS sandbox by sandbox_command.
                    "sandboxPolicy": if permission == "full-access" {
                        json!({"type":"dangerFullAccess"})
                    } else {
                        json!({"type":"externalSandbox","networkAccess":"restricted"})
                    }
                }})).await?;
                notification(app, request_id, json!({"kind":"provider","provider":"codex"}));
            } else if let Some(method) = message.get("method").and_then(Value::as_str) {
                if approval_method(method) && !id.is_null() {
                    let params = &message["params"];
                    if let Some(tid) = params.get("threadId").and_then(Value::as_str) {
                        if Some(tid) != thread_id.as_deref() { continue; }
                    }
                    counter += 1;
                    let approval_id = format!("{request_id}-codex-{counter}");
                    let description = params.get("reason").and_then(Value::as_str)
                        .unwrap_or("Codex requests permission for an operation");
                    let command_text = params.get("command").map(|v| {
                        if let Some(s) = v.as_str() {s.to_owned()} else {v.to_string()}
                    }).or_else(|| params.get("cwd").and_then(Value::as_str).map(str::to_owned));
                    let receiver = approvals.register(approval_id.clone(), request_id.to_owned())?;
                    notification(app, request_id, json!({
                        "kind":"approval_request","provider":"codex",
                        "approvalId":approval_id,"description":description,
                        "command":command_text,"method":method
                    }));
                    // A missing/expired UI response is a denial, never an implicit allow.
                    let allow = tokio::time::timeout(Duration::from_secs(120), receiver)
                        .await.ok().and_then(Result::ok).unwrap_or(false);
                    let reply = approval_response(method, params, allow);
                    send(&mut stdin, json!({"id":id, "result":reply})).await?;
                } else if !id.is_null() {
                    // Unsupported server-initiated request. Fail closed.
                    send(&mut stdin, json!({"id":id, "error":{"code":-32601,"message":"Unsupported interactive request"}})).await?;
                } else if method == "item/agentMessage/delta" {
                    if let Some(delta) = message.pointer("/params/delta").and_then(Value::as_str) {
                        delta_answer.push_str(delta);
                        notification(app, request_id, json!({"kind":"activity","provider":"codex",
                            "event":{"type":"response.output_text.delta","delta":delta}}));
                    }
                } else if method == "item/completed" {
                    let item = &message["params"]["item"];
                    if item.get("type").and_then(Value::as_str) == Some("agentMessage") {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            full_answer.push_str(text);
                        }
                    }
                } else if method == "turn/completed" {
                    let status = message.pointer("/params/turn/status").and_then(Value::as_str).unwrap_or("unknown");
                    if status != "completed" {
                        return Err(format!("Codex turn ended with status: {status}"));
                    }
                    let answer = if full_answer.is_empty() {delta_answer.clone()} else {full_answer.clone()};
                    notification(app, request_id, json!({
                        "kind":"result","provider":"codex","model":"Codex app-server",
                        "answer":answer,"usage":{}
                    }));
                    return Ok(());
                }
            }
        }
        Err("Codex app-server terminated before turn completion".into())
    }, stop).await;
    approvals.cancel_request(request_id);
    if run.is_err() {
        #[cfg(unix)]
        super::kill_process_group(child.id());
        let _ = child.start_kill();
    } else {
        let _ = child.start_kill(); // app-server remains persistent after turn completion
    }
    let _ = child.wait().await;
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_approval_methods_only() {
        assert!(approval_method("item/commandExecution/requestApproval"));
        assert!(approval_method("item/fileChange/requestApproval"));
        assert!(approval_method("item/permissions/requestApproval"));
        assert!(!approval_method("item/agentMessage/delta"));
    }
    #[test]
    fn decisions_are_provider_native_and_fail_closed() {
        assert_eq!(approval_response("item/fileChange/requestApproval", &json!({}), true), json!({"decision":"accept"}));
        assert_eq!(approval_response("item/fileChange/requestApproval", &json!({}), false), json!({"decision":"decline"}));
        assert_eq!(approval_response("item/permissions/requestApproval", &json!({"permissions":{"network":{"enabled":true}}}),false),json!({"permissions":{}}));
    }
}
