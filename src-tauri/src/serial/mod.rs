// The real backend links IOKit, which iOS does not export — see connect_ios.rs.
#[cfg(not(target_os = "ios"))]
pub mod connect;
#[cfg(target_os = "ios")]
#[path = "connect_ios.rs"]
pub mod connect;

#[cfg(all(test, unix, not(target_os = "ios")))]
mod connect_tests;
