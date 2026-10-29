//! Serial console on iOS — compiled out, but still answerable.
//!
//! Two independent reasons this cannot run on iOS: the sandbox exposes no
//! `/dev/tty.*` nodes and no USB-serial API, and `serialport`'s Darwin backend
//! links IOKit symbols (`IOServiceMatching`, `kIOMasterPortDefault`) that iOS
//! does not export — the archive fails at link time if the crate is compiled at
//! all. The dependency is therefore target-gated in Cargo.toml and this module
//! replaces `connect.rs`, keeping every command registered so the UI gets a
//! readable refusal instead of "command not found".

use serde::Serialize;
use tauri::AppHandle;

const NOT_AVAILABLE: &str =
    "Serial ports are not available on iOS: the app sandbox exposes no /dev/tty nodes \
     and no USB-serial accessory API. Connect with SSH or SFTP instead.";

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SerialPortInfo {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialLines {
    pub dtr: bool,
    pub rts: bool,
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SerialLine {
    Dtr,
    Rts,
}

/// Placeholder manager so `app.manage(..)` and the shutdown hook keep the same
/// shape on every target.
#[derive(Default)]
pub struct SerialSessionManager;

impl SerialSessionManager {
    pub fn new() -> Self {
        Self
    }

    pub fn release_all(&self) {}
}

#[tauri::command]
pub fn serial_list_ports() -> Result<Vec<SerialPortInfo>, String> {
    // An empty list (not an error): the picker asks on mount and renders "no
    // ports"; the connect itself is where the explanation belongs.
    Ok(Vec::new())
}

#[tauri::command]
pub fn serial_connect(
    _app: AppHandle,
    _state: tauri::State<'_, SerialSessionManager>,
    _session_id: String,
    _port: String,
    _baud: u32,
    _data_bits: Option<u8>,
    _parity: Option<String>,
    _stop_bits: Option<u8>,
    _flow_control: Option<String>,
) -> Result<SerialLines, String> {
    Err(NOT_AVAILABLE.into())
}

#[tauri::command]
pub fn serial_write(
    _state: tauri::State<'_, SerialSessionManager>,
    _session_id: String,
    _data: Vec<u8>,
) -> Result<(), String> {
    Err(NOT_AVAILABLE.into())
}

#[tauri::command]
pub fn serial_set_line(
    _state: tauri::State<'_, SerialSessionManager>,
    _session_id: String,
    _line: SerialLine,
    _level: bool,
) -> Result<SerialLines, String> {
    Err(NOT_AVAILABLE.into())
}

#[tauri::command]
pub async fn serial_send_break(
    _state: tauri::State<'_, SerialSessionManager>,
    _session_id: String,
) -> Result<(), String> {
    Err(NOT_AVAILABLE.into())
}

#[tauri::command]
pub fn serial_disconnect(
    _state: tauri::State<'_, SerialSessionManager>,
    _session_id: String,
) -> Result<(), String> {
    Ok(())
}
