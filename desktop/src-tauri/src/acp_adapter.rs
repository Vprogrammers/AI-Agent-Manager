//! Shared ACP stdio bridge for OpenCode, Copilot and Antigravity.
//! Native session/request_permission requests become chat approvals and return
//! to the same agent process; unknown client methods are never auto-authorized.
use std::{path::Path, sync::Arc, time::Duration};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, sync::Notify};

fn emit(app: &AppHandle, request_id: &str, data: Value) {
    let _ = app.emit("aam-event", json!({"requestId":request_id,"data":data}));
}
fn cli_spec(provider: &str) -> Result<(&'static str, &'static [&'static str]), String> {
    match provider {
        "antigravity" => Ok(("agy", &["acp"])),
        "opencode" => Ok(("opencode", &["acp"])),
        "copilot" => Ok(("copilot", &["--acp", "--stdio"])),
        _ => Err("Unknown ACP provider".into()),
    }
}
fn option(params: &Value, allow: bool) -> Value {
    let kind = if allow { "allow_once" } else { "reject_once" };
    let choice = params.get("options").and_then(Value::as_array).and_then(|items|
        items.iter().find(|item| item.get("kind").and_then(Value::as_str) == Some(kind))
            .and_then(|item| item.get("optionId").and_then(Value::as_str)));
    match choice {
        Some(id) => json!({"outcome":{"outcome":"selected","optionId":id}}),
        None => json!({"outcome":{"outcome":"cancelled"}}),
    }
}
async fn send(writer: &mut tokio::process::ChildStdin, message: Value) -> Result<(), String> {
    let encoded = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
    writer.write_all(&encoded).await.map_err(|e| e.to_string())?;
    writer.write_all(b"\n").await.map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}
pub async fn run(app: &AppHandle, request_id: &str, root: &Path, provider: &str,
    prompt: &str, permission: &str, stop: Arc<Notify>) -> Result<(), String> {
    let (binary, args) = cli_spec(provider)?;
    let executable = super::find_cli(binary).ok_or_else(|| format!("PRESTART: {provider} CLI not installed"))?;
    let mut command = super::sandbox_command(&executable, args, root, permission, provider)
        .map_err(|e| format!("PRESTART: {e}"))?;
    command.current_dir(root).stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    super::isolate_process_group(&mut command);
    let mut child = command.spawn().map_err(|e| format!("PRESTART: Cannot launch {provider} ACP: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Missing ACP stdin")?;
    let stdout = child.stdout.take().ok_or("Missing ACP stdout")?;
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(_)) = reader.next_line().await {}
        });
    }
    let approvals = app.state::<super::approval::ApprovalRegistry>();
    let result = super::await_runtime(async {
        let mut lines = BufReader::new(stdout).lines();
        let mut session: Option<String> = None;
        let mut answer = String::new();
        let mut request_sequence: u64 = 0;
        send(&mut stdin, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":1,"clientCapabilities":{}
        }})).await?;
        emit(app, request_id, json!({"kind":"provider","provider":provider}));
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            let value: Value = serde_json::from_str(&line)
                .map_err(|e| format!("Invalid ACP protocol: {e}"))?;
            if let Some(error) = value.get("error") {
                return Err(format!("{provider} ACP error: {}",
                    error.get("message").and_then(Value::as_str).unwrap_or("request rejected")));
            }
            let id = &value["id"];
            if id == 1 && value.get("method").is_none() {
                send(&mut stdin, json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
                    "cwd":root.to_string_lossy(),"mcpServers":[]
                }})).await?;
            } else if id == 2 && value.get("method").is_none() {
                let sid = value.pointer("/result/sessionId").and_then(Value::as_str)
                    .ok_or("ACP agent did not create a session")?.to_owned();
                session = Some(sid.clone());
                send(&mut stdin, json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{
                    "sessionId":sid,"prompt":[{"type":"text","text":prompt}]
                }})).await?;
            } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                let params = &value["params"];
                if method == "session/request_permission" && !id.is_null() {
                    if params.get("sessionId").and_then(Value::as_str) != session.as_deref() {
                        send(&mut stdin, json!({"jsonrpc":"2.0","id":id,"result":{
                            "outcome":{"outcome":"cancelled"}
                        }})).await?;
                        continue;
                    }
                    request_sequence += 1;
                    let approval_id = format!("{request_id}-{provider}-{request_sequence}");
                    let description = params.get("description").and_then(Value::as_str)
                        .or_else(|| params.get("title").and_then(Value::as_str))
                        .or_else(|| params.pointer("/toolCall/title").and_then(Value::as_str))
                        .unwrap_or("Agent requests permission to run a tool");
                    let command = params.pointer("/toolCall/rawInput").map(Value::to_string);
                    let allowed = if permission == "full-access" {
                        true // Explicitly selected by the user in the left panel.
                    } else {
                        let receiver = approvals.register(approval_id.clone(), request_id.to_owned())?;
                        emit(app, request_id, json!({"kind":"approval_request","provider":provider,
                            "approvalId":approval_id,"description":description,"command":command}));
                        tokio::time::timeout(Duration::from_secs(120), receiver)
                            .await.ok().and_then(Result::ok).unwrap_or(false)
                    };
                    send(&mut stdin, json!({"jsonrpc":"2.0","id":id,"result":option(params, allowed)})).await?;
                } else if !id.is_null() {
                    send(&mut stdin, json!({"jsonrpc":"2.0","id":id,"error":{
                        "code":-32601,"message":"Unsupported ACP client request"
                    }})).await?;
                } else if method == "session/update" {
                    if params.get("sessionId").and_then(Value::as_str) != session.as_deref() { continue; }
                    let update = &params["update"];
                    if update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk") {
                        if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                            answer.push_str(text);
                            emit(app, request_id, json!({"kind":"activity","provider":provider,
                                "event":{"type":"external.chunk","text":text}}));
                        }
                    } else if let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) {
                        emit(app, request_id, json!({"kind":"activity","provider":provider,
                            "event":{"type":"acp.update","detail":kind}}));
                    }
                }
            } else if id == 3 && value.get("method").is_none() {
                let reason = value.pointer("/result/stopReason").and_then(Value::as_str).unwrap_or("unknown");
                if reason != "end_turn" {
                    return Err(format!("{provider} ACP turn stopped: {reason}"));
                }
                emit(app, request_id, json!({"kind":"result","provider":provider,
                    "model":"ACP session","answer":answer,"usage":{}}));
                return Ok(());
            }
        }
        Err(format!("{provider} ACP process terminated before the turn finished"))
    }, stop).await;
    approvals.cancel_request(request_id);
    #[cfg(unix)]
    super::kill_process_group(child.id());
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_agents_have_acp_transports() {
        assert_eq!(cli_spec("copilot").unwrap().0, "copilot");
        assert_eq!(cli_spec("opencode").unwrap().1, &["acp"]);
        assert_eq!(cli_spec("antigravity").unwrap().1, &["acp"]);
        assert!(cli_spec("sh").is_err());
    }
    #[test]
    fn permission_choices_use_the_actual_option_ids() {
        let input = json!({"options":[
            {"kind":"allow_once","optionId":"yes-23"},
            {"kind":"reject_once","optionId":"no-42"}
        ]});
        assert_eq!(option(&input, true),json!({"outcome":{"outcome":"selected","optionId":"yes-23"}}));
        assert_eq!(option(&input, false),json!({"outcome":{"outcome":"selected","optionId":"no-42"}}));
        assert_eq!(option(&json!({}), true),json!({"outcome":{"outcome":"cancelled"}}));
    }
}
