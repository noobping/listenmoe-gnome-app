#[cfg(debug_assertions)]
pub const APP_ID: &str = concat!("dev.noobping.", env!("CARGO_PKG_NAME"), ".develop");

#[cfg(not(debug_assertions))]
pub const APP_ID: &str = concat!("dev.noobping.", env!("CARGO_PKG_NAME"));

pub const RESOURCE_ID: &str = concat!("/dev/noobping/", env!("CARGO_PKG_NAME"));
