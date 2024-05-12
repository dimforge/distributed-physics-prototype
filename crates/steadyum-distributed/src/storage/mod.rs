#[cfg(not(target_arch = "wasm32"))]
pub use db::{DbCommand, DbContext, DbStats, NewObjectCommand};

pub use plugin::{SaveFileData, StoragePlugin};

#[cfg(not(target_arch = "wasm32"))]
mod db;
mod plugin;
mod position_interpolation;
#[cfg(not(target_arch = "wasm32"))]
mod systems;
