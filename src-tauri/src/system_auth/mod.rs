use serde::Serialize;

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "linux")]
mod linux;
// LocalAuthentication is the same framework on macOS and iOS (LAContext +
// evaluatePolicy); only the enrolment UI differs, so one module serves both.
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
pub mod seal;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
use android as backend;
#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use apple as backend;
#[cfg(target_os = "windows")]
use windows as backend;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VerifyOutcome {
    Ok,
    Cancelled,
    Failed,
    Unavailable,
}

#[tauri::command]
pub async fn system_auth_available() -> bool {
    tauri::async_runtime::spawn_blocking(backend::available)
        .await
        .unwrap_or(false)
}

#[tauri::command]
pub async fn system_auth_verify(app: tauri::AppHandle, reason: String) -> VerifyOutcome {
    backend::verify(&app, &reason).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_serialise_as_kebab_strings() {
        assert_eq!(serde_json::to_string(&VerifyOutcome::Ok).unwrap(), "\"ok\"");
        assert_eq!(
            serde_json::to_string(&VerifyOutcome::Unavailable).unwrap(),
            "\"unavailable\""
        );
    }
}
