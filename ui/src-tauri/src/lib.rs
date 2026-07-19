//! Sanctum desktop shell.
//!
//! The UI is unprivileged and never touches admin APIs. It reaches the
//! LocalSystem service only through the local named pipe, relaying the
//! JSON command/response frames the service already speaks. This crate is a
//! thin, typeless proxy — the protocol types live in the front-end and in
//! `sanctum-core::ipc` on the service side.
//!
//! It also runs the block-moment poller (v0.1.5 §B): a background task asks the
//! service ~once a second whether an intervention is armed and, if so, raises
//! the always-on-top intervention window. A tray icon keeps the app (and this
//! poller) alive after the main window is closed.

use std::time::Duration;

use serde_json::json;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;

const PIPE_NAME: &str = r"\\.\pipe\sanctum-service";

/// The global panic hotkey (§E): summon the breathing window from anywhere,
/// even mid-scroll in another app. Ctrl+Shift+H ("help").
fn panic_hotkey() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyH)
}
/// Human-readable form, kept in sync with `panic_hotkey` for the UI hint.
pub const PANIC_HOTKEY_LABEL: &str = "Ctrl+Shift+H";
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

/// Show + focus a window by label.
fn show_window(app: &tauri::AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Raise the intervention window at Phase 1, carrying the triggering domain
/// (empty for a manual "I need help now").
///
/// The window is sized to span EVERY monitor (their union bounds), not just
/// made fullscreen on one — otherwise the urge simply moves to screen two on a
/// multi-monitor setup. Fullscreen is per-monitor, so we position + size the
/// borderless window across the whole virtual desktop instead.
fn open_intervention(app: &tauri::AppHandle, domain: &str) {
    if let Some(w) = app.get_webview_window("intervention") {
        match w.available_monitors() {
            Ok(monitors) if !monitors.is_empty() => {
                let mut left = i32::MAX;
                let mut top = i32::MAX;
                let mut right = i32::MIN;
                let mut bottom = i32::MIN;
                for m in &monitors {
                    let p = m.position();
                    let s = m.size();
                    left = left.min(p.x);
                    top = top.min(p.y);
                    right = right.max(p.x + s.width as i32);
                    bottom = bottom.max(p.y + s.height as i32);
                }
                let _ = w.set_fullscreen(false);
                let _ = w.set_position(PhysicalPosition::new(left, top));
                let _ = w.set_size(PhysicalSize::new(
                    (right - left).max(1) as u32,
                    (bottom - top).max(1) as u32,
                ));

                // So the content centers on ONE screen (not the bezel seam),
                // emit the primary monitor's frame in LOGICAL px, relative to
                // the union's top-left. Fallback: the first monitor.
                let scale = w.scale_factor().unwrap_or(1.0).max(0.1);
                let primary = w.primary_monitor().ok().flatten().or_else(|| {
                    w.available_monitors().ok().and_then(|m| m.into_iter().next())
                });
                let frame = primary.map(|m| {
                    let p = m.position();
                    let s = m.size();
                    json!({
                        "x": (p.x - left) as f64 / scale,
                        "y": (p.y - top) as f64 / scale,
                        "w": s.width as f64 / scale,
                        "h": s.height as f64 / scale,
                    })
                });
                let _ = w.set_always_on_top(true);
                let _ = w.show();
                let _ = w.set_focus();
                let _ = w.emit("intervention-open", json!({ "domain": domain, "frame": frame }));
                return;
            }
            // No monitor info: fall back to single-screen fullscreen.
            _ => {
                let _ = w.set_fullscreen(true);
            }
        }
        let _ = w.set_always_on_top(true);
        let _ = w.show();
        let _ = w.set_focus();
        // The webview is pre-loaded (hidden) at startup, so the listener is
        // already attached — this resets it to Phase 1 with the new domain.
        let _ = w.emit("intervention-open", json!({ "domain": domain, "frame": null }));
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            // Only one shortcut is ever registered, so any Pressed event is the
            // panic hotkey — raise the intervention window.
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        open_intervention(app, "");
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![get_status, send_command])
        .setup(|app| {
            // Register the global panic hotkey. Non-fatal if another app owns
            // the combo — the tray "I need help now" is always available too.
            if let Err(e) = app.global_shortcut().register(panic_hotkey()) {
                eprintln!("could not register panic hotkey ({PANIC_HOTKEY_LABEL}): {e}");
            }
            // Tray so the app (and the poller below) survive the main window
            // closing. Without it, closing the window would stop interventions.
            let open_i = MenuItem::with_id(app, "open", "Open Sanctum", true, None::<&str>)?;
            let help_i = MenuItem::with_id(app, "help", "I need help now", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_i, &help_i, &quit_i])?;
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Sanctum")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_window(app, "main"),
                    "help" => open_intervention(app, ""),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Block-moment poller (v0.1.5 §A/§B). PollIntervention clears the
            // pending flag on read, so each armed intervention raises the window
            // exactly once.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let Ok(resp) = pipe_roundtrip(&json!({ "cmd": "poll_intervention" })).await
                    else {
                        continue;
                    };
                    let body = resp.get("body");
                    let pending = body
                        .and_then(|b| b.get("pending"))
                        .and_then(|p| p.as_bool())
                        .unwrap_or(false);
                    if pending {
                        let domain = body
                            .and_then(|b| b.get("domain"))
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();
                        open_intervention(&handle, &domain);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it (keeping the tray + poller alive)
            // instead of quitting. The intervention window closes normally.
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    // Main window hides to the tray instead of quitting.
                    "main" => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    // The intervention window has no bypass: Esc / Alt+F4 / the
                    // X are all refused. It is dismissed only by the in-window
                    // "I'm okay" button, which hides it after the pause.
                    "intervention" => api.prevent_close(),
                    _ => {}
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Sanctum");
}
