pub mod link;
#[cfg_attr(not(desktop), allow(dead_code))]
pub mod policy;
#[cfg(desktop)]
pub mod roots;
#[cfg(desktop)]
pub mod server;
#[cfg(desktop)]
pub mod service;

#[cfg(desktop)]
#[derive(Default)]
pub struct DeviceFilesState {
    pub(crate) service: tokio::sync::Mutex<Option<std::sync::Arc<service::FileService>>>,
}
