//! Courtside's engine for other languages. The JSON views here (plays,
//! statistics, validation issues) are what the CLI prints and what the
//! WebAssembly build returns, so every surface reports the same thing
//! (PRD architecture: one core for desktop, CLI, web and iPad).
//!
//! Built for `wasm32-unknown-unknown`, the crate exports [`wasm`]'s
//! functions through wasm-bindgen; `tools/wasm/build.sh` generates the JS
//! glue.

pub mod plays;
pub mod stats;
pub mod validate;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

/// The engine version, shared by all crates in the workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
