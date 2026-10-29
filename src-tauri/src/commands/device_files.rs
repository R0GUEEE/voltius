use crate::commands::lan_sync::{with_manager, LanSyncState};
use crate::device_files::link::{DeviceLink, FilesSource, OnClosed};
use crate::error::AppError;
use crate::lan_sync::manager::FilesDialer;
use crate::sftp::real::RealSftp;
use crate::sftp::{emit_closed, emit_step, FileBackend, SftpManager, SftpStep};
use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

#[cfg(desktop)]
pub use serving::*;

const APPROVAL_HINT: Duration = Duration::from_millis(300);

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConnected {
    pub sftp_id: String,
    pub rtt_ms: u32,
}

#[async_trait]
impl FilesSource for LanSyncState {
    async fn dialer(&self) -> Option<FilesDialer> {
        with_manager(self, |m| Some(m.files_dialer())).await
    }
}

struct Lan(AppHandle);

#[async_trait]
impl FilesSource for Lan {
    async fn dialer(&self) -> Option<FilesDialer> {
        self.0.state::<LanSyncState>().dialer().await
    }
}

pub(crate) async fn connect_inner(
    sftp: &SftpManager,
    source: Arc<dyn FilesSource>,
    device_id: String,
    relink: Option<&str>,
    step: impl Fn(SftpStep),
    on_closed: OnClosed,
) -> Result<DeviceConnected, AppError> {
    let closed = CancellationToken::new();
    let link = Arc::new(DeviceLink::new(
        source,
        device_id,
        closed.clone(),
        on_closed,
    ));
    step(SftpStep::DeviceDialing);
    let dial = link.dial();
    tokio::pin!(dial);
    let session = tokio::select! {
        session = &mut dial => session,
        _ = tokio::time::sleep(APPROVAL_HINT) => {
            step(SftpStep::DeviceApproval);
            dial.await
        }
    }?;
    let started = Instant::now();
    session
        .canonicalize(".")
        .await
        .map_err(|e| AppError::caused("SFTP canonicalize failed", &e))?;
    let rtt_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    let backend: Arc<dyn FileBackend> = Arc::new(RealSftp::over(link.clone(), session));
    let sftp_id = match relink {
        Some(id)
            if sftp
                .relink_standalone(id, backend.clone(), closed.clone())
                .await =>
        {
            id.to_string()
        }
        _ => sftp.register_standalone(backend, closed).await,
    };
    link.bind(sftp_id.clone());
    Ok(DeviceConnected { sftp_id, rtt_ms })
}

#[tauri::command]
pub async fn device_files_connect(
    app: AppHandle,
    sftp_state: State<'_, SftpManager>,
    device_id: String,
    connect_id: String,
    relink: Option<String>,
) -> Result<DeviceConnected, AppError> {
    let detail = device_id.clone();
    let closer = app.clone();
    connect_inner(
        &sftp_state,
        Arc::new(Lan(app.clone())),
        device_id,
        relink.as_deref(),
        |step| emit_step(&app, &connect_id, step, detail.as_str()),
        Box::new(move |id| emit_closed(&closer, id)),
    )
    .await
}

#[cfg(desktop)]
mod serving {
    use crate::commands::lan_sync::{emitter, Emit, LanSyncState};
    use crate::device_files::policy::{Answer, ApprovalPrompt, FileAccessConfig};
    use crate::device_files::service::{Alert, FileService};
    use crate::device_files::DeviceFilesState;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tauri::{AppHandle, Manager, State};

    async fn offer(lan: &LanSyncState, svc: &Arc<FileService>) {
        lan.offer_files(Some(svc.clone()), svc.offers()).await;
    }

    async fn running(files: &DeviceFilesState) -> Option<Arc<FileService>> {
        files.service.lock().await.clone()
    }

    pub(crate) async fn start_inner(
        lan: &LanSyncState,
        files: &DeviceFilesState,
        dir: PathBuf,
        account: String,
        emit: Emit,
        alert: Alert,
    ) {
        let mut slot = files.service.lock().await;
        let svc = match slot.as_ref().filter(|s| s.account() == account) {
            Some(svc) => svc.clone(),
            None => {
                if let Some(old) = slot.take() {
                    old.stop();
                }
                let svc = Arc::new(FileService::new(dir, account, emit, alert));
                *slot = Some(svc.clone());
                svc
            }
        };
        offer(lan, &svc).await;
    }

    pub(crate) async fn stop_inner(lan: &LanSyncState, files: &DeviceFilesState) {
        let mut slot = files.service.lock().await;
        if let Some(svc) = slot.take() {
            svc.stop();
        }
        lan.offer_files(None, false).await;
    }

    pub(crate) async fn set_config_inner(
        lan: &LanSyncState,
        files: &DeviceFilesState,
        config: FileAccessConfig,
    ) -> Result<(), String> {
        let slot = files.service.lock().await;
        let svc = slot.as_ref().ok_or("File access is not running")?;
        svc.set_config(config)?;
        offer(lan, svc).await;
        Ok(())
    }

    fn alert(app: AppHandle) -> Alert {
        Arc::new(move |prompt: &ApprovalPrompt| {
            let focused = app
                .webview_windows()
                .values()
                .any(|w| w.is_focused().unwrap_or(false));
            if focused {
                return;
            }
            use tauri_plugin_notification::NotificationExt;
            let shown = app
                .notification()
                .builder()
                .title(&prompt.device_name)
                .body(format!("{} wants to open your files", prompt.device_name))
                .show();
            if let Err(e) = shown {
                log::warn!("device-files: notification failed: {e}");
            }
        })
    }

    #[tauri::command]
    pub async fn device_files_start(
        app: AppHandle,
        lan: State<'_, LanSyncState>,
        files: State<'_, DeviceFilesState>,
        account_id: String,
    ) -> Result<(), String> {
        let dir = crate::storage::config::config_dir();
        let (emit, alert) = (emitter(app.clone()), alert(app));
        start_inner(&lan, &files, dir, account_id, emit, alert).await;
        Ok(())
    }

    #[tauri::command]
    pub async fn device_files_stop(
        lan: State<'_, LanSyncState>,
        files: State<'_, DeviceFilesState>,
    ) -> Result<(), String> {
        stop_inner(&lan, &files).await;
        Ok(())
    }

    #[tauri::command]
    pub async fn device_files_get_config(
        files: State<'_, DeviceFilesState>,
    ) -> Result<FileAccessConfig, String> {
        Ok(running(&files)
            .await
            .map(|s| s.config())
            .unwrap_or_default())
    }

    #[tauri::command]
    pub async fn device_files_set_config(
        lan: State<'_, LanSyncState>,
        files: State<'_, DeviceFilesState>,
        config: FileAccessConfig,
    ) -> Result<(), String> {
        set_config_inner(&lan, &files, config).await
    }

    #[tauri::command]
    pub async fn device_files_answer(
        files: State<'_, DeviceFilesState>,
        request_id: String,
        answer: Answer,
    ) -> Result<bool, String> {
        Ok(running(&files)
            .await
            .is_some_and(|s| s.answer(&request_id, answer)))
    }

    #[tauri::command]
    pub async fn device_files_disconnect(
        files: State<'_, DeviceFilesState>,
        device_key: String,
    ) -> Result<(), String> {
        if let Some(svc) = running(&files).await {
            svc.disconnect(&device_key);
        }
        Ok(())
    }

    #[tauri::command]
    pub async fn device_files_pending(
        files: State<'_, DeviceFilesState>,
    ) -> Result<Vec<ApprovalPrompt>, String> {
        Ok(running(&files)
            .await
            .map(|s| s.pending())
            .unwrap_or_default())
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::commands::lan_sync::{peers_inner, start_inner as lan_start, Emit};
    use crate::commands::sftp::resume::copy_one;
    use crate::commands::sftp::resume::endpoint::LocalFs;
    use crate::commands::sftp::resume::engine_tests::noise;
    use crate::device_files::policy::{self, hex, AllowedDevice, Answer, FileAccessConfig};
    use crate::device_files::service::{FileService, REQUEST, SESSIONS};
    use crate::device_files::DeviceFilesState;
    use crate::error::ErrorCode;
    use crate::lan_sync::identity::test_identity;
    use crate::sftp::backend::test_tree::Recorder;
    use serde_json::{json, Value};
    use std::future::Future;
    use std::path::PathBuf;
    use std::sync::Mutex;

    const ACCOUNT: &str = "acct";

    type Seen = Arc<Mutex<Vec<(&'static str, Value)>>>;

    fn key_of(id: &str) -> String {
        hex(&test_identity(id).key.public)
    }

    async fn until<F: Future<Output = bool>>(what: &str, mut cond: impl FnMut() -> F) {
        let poll = async {
            while !cond().await {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        if tokio::time::timeout(Duration::from_secs(30), poll)
            .await
            .is_err()
        {
            panic!("timed out waiting for {what}");
        }
    }

    async fn lan(id: &str) -> Arc<LanSyncState> {
        let state = Arc::new(LanSyncState::default());
        lan_start(
            &state,
            test_identity(id),
            vec![vec![7; 32]],
            Arc::new(|_, _| {}),
        )
        .await
        .unwrap();
        state
    }

    struct Server {
        lan: Arc<LanSyncState>,
        files: DeviceFilesState,
        dir: tempfile::TempDir,
        seen: Seen,
    }

    fn config(share: &std::path::Path, always: &[&str]) -> FileAccessConfig {
        FileAccessConfig {
            enabled: true,
            folders: vec![share.to_string_lossy().into_owned()],
            read_only: false,
            allowed: always
                .iter()
                .map(|id| AllowedDevice {
                    device_key: key_of(id),
                    name: format!("{id} name"),
                    always: true,
                    last_opened: None,
                })
                .collect(),
            seen_keys: vec![],
        }
    }

    async fn server(always: &[&str]) -> Server {
        let dir = tempfile::tempdir().unwrap();
        let share = dir.path().join("Shared");
        std::fs::create_dir(&share).unwrap();
        std::fs::write(share.join("hello.txt"), b"hi").unwrap();
        policy::save(dir.path(), ACCOUNT, &config(&share, always)).unwrap();
        let seen = Seen::default();
        let sink = seen.clone();
        let emit: Emit = Arc::new(move |name, payload| sink.lock().unwrap().push((name, payload)));
        let lan = lan("dev-b").await;
        let files = DeviceFilesState::default();
        serving::start_inner(
            &lan,
            &files,
            dir.path().to_path_buf(),
            ACCOUNT.into(),
            emit,
            Arc::new(|_| {}),
        )
        .await;
        Server {
            lan,
            files,
            dir,
            seen,
        }
    }

    impl Server {
        fn share(&self) -> PathBuf {
            self.dir.path().join("Shared")
        }

        async fn svc(&self) -> Arc<FileService> {
            self.files.service.lock().await.clone().unwrap()
        }

        fn events(&self, name: &str) -> Vec<Value> {
            let seen = self.seen.lock().unwrap();
            seen.iter()
                .filter(|(n, _)| *n == name)
                .map(|(_, v)| v.clone())
                .collect()
        }

        fn open_keys(&self) -> Vec<String> {
            let last = self.events(SESSIONS).pop().unwrap_or(json!([]));
            let mut keys: Vec<String> = last
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["deviceKey"].as_str().unwrap().to_string())
                .collect();
            keys.sort();
            keys
        }

        async fn until_open(&self, mut want: Vec<String>) {
            want.sort();
            until(&format!("open sessions {want:?}"), || {
                let now = self.open_keys();
                let want = want.clone();
                async move { now == want }
            })
            .await;
        }

        async fn until_pending(&self) -> String {
            let svc = self.svc().await;
            until("a pending approval", || {
                let any = !svc.pending().is_empty();
                async move { any }
            })
            .await;
            svc.pending()[0].request_id.clone()
        }

        async fn set(&self, cfg: FileAccessConfig) {
            serving::set_config_inner(&self.lan, &self.files, cfg)
                .await
                .unwrap();
        }
    }

    struct Browser {
        id: &'static str,
        lan: Arc<LanSyncState>,
        sftp: SftpManager,
        steps: Arc<Mutex<Vec<Value>>>,
        closed: Arc<Mutex<Vec<String>>>,
    }

    async fn browser(id: &'static str, server: &Server) -> Browser {
        let lan = lan(id).await;
        lan.meet(&server.lan).await;
        until("dev-b listed with files on", || {
            let lan = lan.clone();
            async move {
                peers_inner(&lan)
                    .await
                    .iter()
                    .any(|p| p.device_id == "dev-b" && p.files)
            }
        })
        .await;
        Browser {
            id,
            lan,
            sftp: SftpManager::new(),
            steps: Arc::default(),
            closed: Arc::default(),
        }
    }

    impl Browser {
        async fn connect(&self) -> Result<DeviceConnected, AppError> {
            let (steps, closed) = (self.steps.clone(), self.closed.clone());
            connect_inner(
                &self.sftp,
                self.lan.clone(),
                "dev-b".into(),
                None,
                move |s| steps.lock().unwrap().push(json!(s)),
                Box::new(move |id| closed.lock().unwrap().push(id.to_string())),
            )
            .await
        }

        async fn list(&self, sftp_id: &str, path: &str) -> Result<Vec<String>, AppError> {
            let backend = self.sftp.backend(sftp_id).await.unwrap();
            let mut names: Vec<String> = backend
                .list_dir(path)
                .await?
                .into_iter()
                .map(|f| f.name)
                .collect();
            names.sort();
            Ok(names)
        }

        fn steps(&self) -> Vec<Value> {
            self.steps.lock().unwrap().clone()
        }
    }

    fn code(r: Result<DeviceConnected, AppError>) -> Option<ErrorCode> {
        r.expect_err("connected").code()
    }

    #[tokio::test]
    async fn always_allowed_device_lists_shared_folder() {
        let s = server(&["dev-a"]).await;
        let a = browser("dev-a", &s).await;
        let c = a.connect().await.unwrap();
        assert_eq!(a.steps()[0], json!("device_dialing"));
        assert_eq!(a.list(&c.sftp_id, "/").await.unwrap(), ["Shared"]);
        assert_eq!(a.list(&c.sftp_id, "/Shared").await.unwrap(), ["hello.txt"]);
        assert!(s.events(REQUEST).is_empty());
        s.until_open(vec![key_of(a.id)]).await;
    }

    #[tokio::test]
    async fn unknown_device_waits_for_approval_then_lists() {
        let s = server(&[]).await;
        let a = browser("dev-a", &s).await;
        let answer = async {
            let id = s.until_pending().await;
            assert_eq!(s.events(REQUEST)[0]["requestId"], json!(id));
            assert_eq!(s.events(REQUEST)[0]["newDevice"], json!(true));
            until("the approval step", || {
                let shown = a.steps().contains(&json!("device_approval"));
                async move { shown }
            })
            .await;
            assert!(s.svc().await.answer(&id, Answer::Once));
        };
        let (connected, ()) = tokio::join!(a.connect(), answer);
        let c = connected.unwrap();
        assert_eq!(
            a.steps(),
            [json!("device_dialing"), json!("device_approval")]
        );
        assert_eq!(a.list(&c.sftp_id, "/Shared").await.unwrap(), ["hello.txt"]);
        let cfg = s.svc().await.config();
        assert_eq!(cfg.seen_keys, [key_of("dev-a")]);
        assert!(cfg.allowed.is_empty());
    }

    #[tokio::test]
    async fn always_answer_is_persisted() {
        let s = server(&[]).await;
        let a = browser("dev-a", &s).await;
        let answer = async {
            let id = s.until_pending().await;
            assert!(s.svc().await.answer(&id, Answer::Always));
        };
        let (connected, ()) = tokio::join!(a.connect(), answer);
        connected.unwrap();
        let saved = policy::load(s.dir.path(), ACCOUNT);
        assert!(saved.always_allows(&key_of("dev-a")));
        assert_eq!(saved.seen_keys, [key_of("dev-a")]);
        a.connect().await.unwrap();
        assert_eq!(s.events(REQUEST).len(), 1);
    }

    #[tokio::test]
    async fn deny_maps_to_error_code() {
        let s = server(&[]).await;
        let a = browser("dev-a", &s).await;
        let answer = async {
            let id = s.until_pending().await;
            assert!(s.svc().await.answer(&id, Answer::Deny));
        };
        let (connected, ()) = tokio::join!(a.connect(), answer);
        assert_eq!(code(connected), Some(ErrorCode::DeviceAccessDenied));
        assert!(s.open_keys().is_empty());
    }

    #[tokio::test]
    async fn approval_timeout_maps_to_error_code() {
        let s = server(&[]).await;
        let a = browser("dev-a", &s).await;
        let pause = async {
            s.until_pending().await;
            tokio::time::pause();
        };
        let (connected, ()) = tokio::join!(a.connect(), pause);
        assert_eq!(code(connected), Some(ErrorCode::DeviceApprovalTimedOut));
        assert!(s.svc().await.pending().is_empty());
    }

    #[tokio::test]
    async fn reconnect_with_grant_skips_prompt() {
        let s = server(&[]).await;
        let big = noise(24 << 20);
        std::fs::write(s.share().join("big.bin"), &big).unwrap();
        let a = browser("dev-a", &s).await;
        let answer = async {
            let id = s.until_pending().await;
            assert!(s.svc().await.answer(&id, Answer::Once));
        };
        let (connected, ()) = tokio::join!(a.connect(), answer);
        let c = connected.unwrap();
        let src = a.sftp.backend(&c.sftp_id).await.unwrap().endpoint();
        let out = tempfile::tempdir().unwrap();
        let dst = out.path().join("big.bin");
        let (rec, token) = (Recorder::default(), CancellationToken::new());
        let copy = copy_one(
            &rec,
            &*src,
            "/Shared/big.bin",
            &LocalFs,
            dst.to_str().unwrap(),
            "device-reconnect",
            &token,
        );
        let svc = s.svc().await;
        let cut = async {
            until("a partial download", || {
                let started = std::fs::read_dir(out.path())
                    .unwrap()
                    .any(|e| e.unwrap().metadata().unwrap().len() > 0);
                async move { started }
            })
            .await;
            svc.cut();
        };
        let (copied, ()) = tokio::join!(copy, cut);
        copied.unwrap();
        assert!(std::fs::read(&dst).unwrap() == big);
        assert!(svc.admitted() >= 2, "the transfer never reconnected");
        assert_eq!(s.events(REQUEST).len(), 1);
    }

    #[tokio::test]
    async fn turning_off_closes_open_sessions() {
        let s = server(&["dev-a"]).await;
        let a = browser("dev-a", &s).await;
        let c = a.connect().await.unwrap();
        s.until_open(vec![key_of("dev-a")]).await;
        let mut off = s.svc().await.config();
        off.enabled = false;
        s.set(off).await;
        s.until_open(vec![]).await;
        until("caps off", || {
            let lan = a.lan.clone();
            async move { peers_inner(&lan).await.iter().all(|p| !p.files) }
        })
        .await;
        assert!(a.list(&c.sftp_id, "/").await.is_err());
        assert_eq!(*a.closed.lock().unwrap(), std::slice::from_ref(&c.sftp_id));
        assert_eq!(
            code(a.connect().await),
            Some(ErrorCode::DeviceFileAccessOff)
        );
    }

    #[tokio::test]
    async fn revoke_closes_that_device_only() {
        let s = server(&["dev-a", "dev-c"]).await;
        let a = browser("dev-a", &s).await;
        let c = browser("dev-c", &s).await;
        a.connect().await.unwrap();
        let on_c = c.connect().await.unwrap();
        s.until_open(vec![key_of("dev-a"), key_of("dev-c")]).await;
        s.set(config(&s.share(), &["dev-c"])).await;
        s.until_open(vec![key_of("dev-c")]).await;
        assert_eq!(c.list(&on_c.sftp_id, "/").await.unwrap(), ["Shared"]);
        assert!(a.closed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stop_closes_everything() {
        let s = server(&["dev-a"]).await;
        let a = browser("dev-a", &s).await;
        let c = browser("dev-c", &s).await;
        a.connect().await.unwrap();
        s.until_open(vec![key_of("dev-a")]).await;
        let stop = async {
            s.until_pending().await;
            serving::stop_inner(&s.lan, &s.files).await;
        };
        let (waiting, ()) = tokio::join!(c.connect(), stop);
        assert_eq!(code(waiting), Some(ErrorCode::DeviceFileAccessOff));
        s.until_open(vec![]).await;
        assert!(s.files.service.lock().await.is_none());
        until("caps off", || {
            let lan = a.lan.clone();
            async move { peers_inner(&lan).await.iter().all(|p| !p.files) }
        })
        .await;
    }
}
