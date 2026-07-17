//! Sanctum desktop shell.
//!
//! The UI is unprivileged and never touches admin APIs. It reaches the
//! LocalSystem service only through the local named pipe, relaying the
//! JSON command/response frames the service already speaks. This crate is a
//! thin, typeless proxy — the protocol types live in the front-end and in
//! `sanctum-core::ipc` on the service side.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;

const PIPE_NAME: &str = r"\\.\pipe\sanctum-service";
const MAX_FRAME: usize = 1 << 20;

async fn pipe_roundtrip(cmd: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut client = ClientOptions::new()
        .open(PIPE_NAME)
        .map_err(|e| format!("Sanctum service unavailable: {e}"))?;

    let payload = serde_json::to_vec(cmd).map_err(|e| e.to_string())?;
    client
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    client.write_all(&payload).await.map_err(|e| e.to_string())?;
    client.flush().await.map_err(|e| e.to_string())?;

    let mut len = [0u8; 4];
    client.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err("invalid response frame".into());
    }
    let mut buf = vec![0u8; n];
    client.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

/// Relay an arbitrary command and return the raw `Response` JSON.
#[tauri::command]
async fn send_command(command: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_roundtrip(&command).await
}

/// Convenience: fetch status and unwrap the `Status` body for the home screen.
#[tauri::command]
async fn get_status() -> Result<serde_json::Value, String> {
    let resp = pipe_roundtrip(&serde_json::json!({ "cmd": "get_status" })).await?;
    Ok(resp.get("body").cloned().unwrap_or(resp))
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_status, send_command])
        .run(tauri::generate_context!())
        .expect("error while running Sanctum");
}
