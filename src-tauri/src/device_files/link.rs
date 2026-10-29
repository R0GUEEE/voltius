use crate::error::{AppError, ErrorCode};
use crate::lan_sync::manager::{FilesDialError, FilesDialer};
use crate::lan_sync::protocol::{FilesReply, FilesRequest, Refusal};
use crate::lan_sync::stream::SessionStream;
use crate::sftp::link::SftpReopen;
use async_trait::async_trait;
use russh_sftp::client::SftpSession;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

// russh_sftp loops forever on any read error but EOF, so every failure and `stop` read as EOF.
pub(crate) struct Wire<G = ()> {
    stream: SessionStream,
    stop: Pin<Box<WaitForCancellationFutureOwned>>,
    _guard: G,
}

impl<G> Wire<G> {
    pub(crate) fn new(stream: SessionStream, stop: CancellationToken, guard: G) -> Self {
        Self {
            stream,
            stop: Box::pin(stop.cancelled_owned()),
            _guard: guard,
        }
    }
}

fn ended() -> io::Error {
    io::ErrorKind::UnexpectedEof.into()
}

impl<G: Unpin> Wire<G> {
    fn live(&mut self, cx: &mut Context<'_>) -> io::Result<Pin<&mut SessionStream>> {
        match self.stop.as_mut().poll(cx) {
            Poll::Ready(()) => Err(ended()),
            Poll::Pending => Ok(Pin::new(&mut self.stream)),
        }
    }
}

impl<G: Unpin> AsyncRead for Wire<G> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut().live(cx) {
            Ok(stream) => stream.poll_read(cx, buf).map_err(|_| ended()),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl<G: Unpin> AsyncWrite for Wire<G> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut().live(cx) {
            Ok(stream) => stream.poll_write(cx, buf).map_err(|_| ended()),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[async_trait]
pub trait FilesSource: Send + Sync {
    async fn dialer(&self) -> Option<FilesDialer>;
}

pub type OnClosed = Box<dyn Fn(&str) + Send + Sync>;

pub struct DeviceLink {
    source: Arc<dyn FilesSource>,
    device_id: String,
    grant: Mutex<Option<String>>,
    closed: CancellationToken,
    sftp_id: OnceLock<String>,
    on_closed: OnClosed,
}

fn unreachable(why: impl std::fmt::Display) -> AppError {
    AppError::coded(
        ErrorCode::DeviceUnreachable,
        format!("The device can't be reached: {why}"),
    )
}

pub fn refused(reason: Refusal) -> AppError {
    let (code, message) = match reason {
        Refusal::Off => (
            ErrorCode::DeviceFileAccessOff,
            "File access is off on that device",
        ),
        Refusal::Denied => (ErrorCode::DeviceAccessDenied, "The device denied access"),
        Refusal::TimedOut => (
            ErrorCode::DeviceApprovalTimedOut,
            "Nobody answered the approval request on the device",
        ),
        Refusal::Busy => (
            ErrorCode::DeviceBusy,
            "The device has too many open file sessions",
        ),
    };
    AppError::coded(code, message)
}

impl DeviceLink {
    pub fn new(
        source: Arc<dyn FilesSource>,
        device_id: String,
        closed: CancellationToken,
        on_closed: OnClosed,
    ) -> Self {
        Self {
            source,
            device_id,
            grant: Mutex::default(),
            closed,
            sftp_id: OnceLock::new(),
            on_closed,
        }
    }

    pub fn bind(&self, sftp_id: String) {
        let _ = self.sftp_id.set(sftp_id);
    }

    fn grant(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.grant.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub async fn dial(&self) -> Result<SftpSession, AppError> {
        let dialer = self
            .source
            .dialer()
            .await
            .ok_or_else(|| unreachable("local sync is off"))?;
        let req = FilesRequest {
            grant: self.grant().clone(),
        };
        let (stream, reply) = match dialer.open_files(&self.device_id, req).await {
            Ok(opened) => opened,
            Err(FilesDialError::Refused(reason)) => {
                self.closed.cancel();
                if let Some(id) = self.sftp_id.get() {
                    (self.on_closed)(id);
                }
                return Err(refused(reason));
            }
            Err(e) => return Err(unreachable(e)),
        };
        if let FilesReply::Accepted { grant: Some(g), .. } = reply {
            *self.grant() = Some(g);
        }
        SftpSession::new(Wire::new(stream, self.closed.clone(), ()))
            .await
            .map_err(unreachable)
    }
}

#[async_trait]
impl SftpReopen for DeviceLink {
    async fn open(&self) -> Result<SftpSession, String> {
        self.dial().await.map_err(String::from)
    }

    fn closed_now(&self) -> bool {
        self.closed.is_cancelled()
    }

    fn closed(&self) -> &CancellationToken {
        &self.closed
    }
}
