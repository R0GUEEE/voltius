use super::link::Wire;
use super::policy::{
    self, decide, hex, AllowedDevice, Answer, ApprovalPrompt, Broker, Decision, FileAccessConfig,
    Grants, APPROVAL_WAIT,
};
use super::roots::SharedRoots;
use super::server::SharedFs;
use crate::commands::lan_sync::Emit;
use crate::lan_sync::handshake::Session;
use crate::lan_sync::manager::InboundFiles;
use crate::lan_sync::protocol::{FilesReply, FilesRequest, Frame, Hello, Refusal};
use crate::lan_sync::route::Route;
use crate::lan_sync::stream::SessionStream;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub const REQUEST: &str = "device-files://request";
pub const SESSIONS: &str = "device-files://sessions";
pub const CONFIG: &str = "device-files://config";
const MAX_SESSIONS: usize = 8;
const MAX_PENDING: usize = 4;

pub type Alert = Arc<dyn Fn(&ApprovalPrompt) + Send + Sync>;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSession {
    pub device_key: String,
    pub device_name: String,
    pub since: String,
}

struct Live {
    info: ActiveSession,
    grant: Option<String>,
    stop: CancellationToken,
}

#[derive(Default)]
struct Book {
    cfg: FileAccessConfig,
    grants: Grants,
    sessions: HashMap<u64, Live>,
    next: u64,
}

impl Book {
    fn close(&self, which: impl Fn(&Live) -> bool) {
        for live in self.sessions.values().filter(|l| which(l)) {
            live.stop.cancel();
        }
    }

    fn listing(&self) -> Vec<ActiveSession> {
        let mut list: Vec<ActiveSession> = self.sessions.values().map(|l| l.info.clone()).collect();
        list.sort_by(|a, b| a.since.cmp(&b.since));
        list
    }
}

struct Core {
    book: Mutex<Book>,
    emit: Emit,
}

impl Core {
    fn lock(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn publish(&self, book: MutexGuard<'_, Book>) {
        let list = book.listing();
        drop(book);
        (self.emit)(SESSIONS, json!(list));
    }

    fn end(&self, id: u64) {
        let mut book = self.lock();
        let Some(live) = book.sessions.remove(&id) else {
            return;
        };
        if let Some(grant) = &live.grant {
            book.grants.touch(grant, Instant::now());
        }
        self.publish(book);
    }
}

struct Ending {
    core: Arc<Core>,
    id: u64,
}

impl Drop for Ending {
    fn drop(&mut self) {
        self.core.end(self.id);
    }
}

struct Admitted {
    read_only: bool,
    grant: Option<String>,
    roots: SharedRoots,
    stop: CancellationToken,
    ending: Ending,
}

pub struct FileService {
    core: Arc<Core>,
    broker: Broker,
    dir: PathBuf,
    account: String,
    alert: Alert,
    stopped: CancellationToken,
}

fn offers(cfg: &FileAccessConfig) -> bool {
    cfg.enabled && !cfg.folders.is_empty()
}

fn remember(cfg: &mut FileAccessConfig, key: &str, name: &str, always: bool) {
    if !cfg.seen_keys.iter().any(|k| k == key) {
        cfg.seen_keys.push(key.to_string());
    }
    if !always {
        return;
    }
    match cfg.allowed.iter_mut().find(|d| d.device_key == key) {
        Some(device) => {
            device.always = true;
            device.name = name.to_string();
        }
        None => cfg.allowed.push(AllowedDevice {
            device_key: key.to_string(),
            name: name.to_string(),
            always: true,
            last_opened: None,
        }),
    }
}

impl FileService {
    pub fn new(dir: PathBuf, account: String, emit: Emit, alert: Alert) -> Self {
        let cfg = policy::load(&dir, &account);
        Self {
            core: Arc::new(Core {
                book: Mutex::new(Book {
                    cfg,
                    ..Book::default()
                }),
                emit,
            }),
            broker: Broker::default(),
            dir,
            account,
            alert,
            stopped: CancellationToken::new(),
        }
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn config(&self) -> FileAccessConfig {
        self.core.lock().cfg.clone()
    }

    pub fn offers(&self) -> bool {
        offers(&self.core.lock().cfg)
    }

    pub fn pending(&self) -> Vec<ApprovalPrompt> {
        self.broker.pending()
    }

    pub fn answer(&self, request_id: &str, answer: Answer) -> bool {
        self.broker.answer(request_id, answer)
    }

    pub fn set_config(&self, mut cfg: FileAccessConfig) -> Result<(), String> {
        let mut guard = self.core.lock();
        let book = &mut *guard;
        for key in &book.cfg.seen_keys {
            if !cfg.seen_keys.contains(key) {
                cfg.seen_keys.push(key.clone());
            }
        }
        policy::save(&self.dir, &self.account, &cfg)?;
        let old = std::mem::replace(&mut book.cfg, cfg);
        let new = &book.cfg;
        if !offers(new) {
            book.grants.clear();
            book.close(|_| true);
            return Ok(());
        }
        if old.folders != new.folders || old.read_only != new.read_only {
            book.close(|_| true);
        }
        let revoked: Vec<String> = old
            .allowed
            .iter()
            .filter(|d| d.always && !new.always_allows(&d.device_key))
            .map(|d| d.device_key.clone())
            .collect();
        for key in &revoked {
            book.grants.revoke_key(key);
            book.close(|l| l.info.device_key == *key);
        }
        Ok(())
    }

    pub fn disconnect(&self, key: &str) {
        let mut book = self.core.lock();
        book.grants.revoke_key(key);
        book.close(|l| l.info.device_key == key);
    }

    pub fn stop(&self) {
        self.stopped.cancel();
        let mut book = self.core.lock();
        book.grants.clear();
        book.close(|_| true);
    }

    fn check(&self, book: &mut Book, key: &str, req: &FilesRequest) -> Result<Decision, Refusal> {
        if self.stopped.is_cancelled() {
            return Err(Refusal::Off);
        }
        let now = Instant::now();
        for grant in book.sessions.values().filter_map(|l| l.grant.as_ref()) {
            book.grants.touch(grant, now);
        }
        let open = book
            .sessions
            .values()
            .filter(|l| l.info.device_key == key)
            .count();
        match decide(&book.cfg, &book.grants, key, req, open, now) {
            Decision::Refuse(reason) => Err(reason),
            _ if book.sessions.len() >= MAX_SESSIONS => Err(Refusal::Busy),
            decision => Ok(decision),
        }
    }

    async fn ask(
        &self,
        peer: &Hello,
        key: &str,
        route: Route,
        new_device: bool,
    ) -> Result<Option<String>, Refusal> {
        let prompt = ApprovalPrompt {
            request_id: policy::new_request_id(),
            device_key: key.to_string(),
            device_name: peer.device_name.clone(),
            platform: peer.platform.clone(),
            route,
            new_device,
        };
        let pending = {
            let _book = self.core.lock();
            if self.broker.pending().len() >= MAX_PENDING {
                return Err(Refusal::Busy);
            }
            self.broker.register(prompt.clone())
        };
        (self.core.emit)(REQUEST, json!(prompt));
        (self.alert)(&prompt);
        let answer = tokio::select! {
            answer = pending.wait(APPROVAL_WAIT) => answer,
            _ = self.stopped.cancelled() => return Err(Refusal::Off),
        };
        let always = match answer {
            None => return Err(Refusal::TimedOut),
            Some(Answer::Deny) => return Err(Refusal::Denied),
            Some(Answer::Once) => false,
            Some(Answer::Always) => true,
        };
        let mut book = self.core.lock();
        remember(&mut book.cfg, key, &peer.device_name, always);
        if let Err(e) = policy::save(&self.dir, &self.account, &book.cfg) {
            log::warn!("device-files: saving the approval failed: {e}");
        }
        let grant = (!always).then(|| book.grants.issue(key));
        let cfg = book.cfg.clone();
        drop(book);
        (self.core.emit)(CONFIG, json!(cfg));
        Ok(grant)
    }

    fn admit(&self, key: &str, name: &str, req: &FilesRequest) -> Result<Admitted, Refusal> {
        let mut book = self.core.lock();
        let Decision::Accept { read_only, grant } = self.check(&mut book, key, req)? else {
            return Err(Refusal::Denied);
        };
        if let Some(grant) = &grant {
            book.grants.touch(grant, Instant::now());
        }
        book.next += 1;
        let id = book.next;
        let stop = CancellationToken::new();
        let info = ActiveSession {
            device_key: key.to_string(),
            device_name: name.to_string(),
            since: chrono::Utc::now().to_rfc3339(),
        };
        book.sessions.insert(
            id,
            Live {
                info,
                grant: grant.clone(),
                stop: stop.clone(),
            },
        );
        let folders: Vec<PathBuf> = book.cfg.folders.iter().map(PathBuf::from).collect();
        self.core.publish(book);
        Ok(Admitted {
            read_only,
            grant,
            roots: SharedRoots::new(&folders),
            stop,
            ending: Ending {
                core: self.core.clone(),
                id,
            },
        })
    }

    async fn authorize(
        &self,
        peer: &Hello,
        route: Route,
        mut req: FilesRequest,
    ) -> Result<Admitted, Refusal> {
        let key = hex(&peer.device_key);
        let decision = self.check(&mut self.core.lock(), &key, &req)?;
        if let Decision::Ask { new_device } = decision {
            req.grant = self.ask(peer, &key, route, new_device).await?;
        }
        self.admit(&key, &peer.device_name, &req)
    }
}

#[cfg(test)]
impl FileService {
    pub(crate) fn cut(&self) {
        self.core.lock().close(|_| true);
    }

    pub(crate) fn admitted(&self) -> u64 {
        self.core.lock().next
    }
}

#[async_trait::async_trait]
impl InboundFiles for FileService {
    async fn serve(&self, mut session: Session, peer: Hello, route: Route, req: FilesRequest) {
        let admitted = self.authorize(&peer, route, req).await;
        let reply = match &admitted {
            Ok(a) => FilesReply::Accepted {
                read_only: a.read_only,
                grant: a.grant.clone(),
            },
            Err(reason) => FilesReply::Refused { reason: *reason },
        };
        if session.send(&Frame::FilesReply(reply)).await.is_err() {
            return;
        }
        let Ok(a) = admitted else {
            return;
        };
        let wire = Wire::new(SessionStream::new(session), a.stop, a.ending);
        russh_sftp::server::run(wire, SharedFs::new(a.roots, a.read_only)).await;
    }
}
