//! WebAssembly exports. Inputs are file bytes; outputs are JSON strings
//! in the same shapes as `courtside convert`/`stats --json`/`validate`.

use std::path::Path;

use dvw_codec::Dvw;
use wasm_bindgen::prelude::*;

use crate::stats::Symbols;

fn parse(bytes: &[u8]) -> Result<Dvw, JsError> {
    Dvw::parse(bytes).map_err(|e| JsError::new(&e.to_string()))
}

fn json(value: &serde_json::Value) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|e| JsError::new(&e.to_string()))
}

/// The engine version.
#[wasm_bindgen]
pub fn version() -> String {
    crate::VERSION.to_owned()
}

/// Issues in a `.dvw` or `.vsm` file (chosen by `file_name`'s
/// extension), as a JSON array. Never throws: an unreadable file is an
/// error issue.
#[wasm_bindgen]
pub fn validate(file_name: &str, bytes: &[u8]) -> Result<String, JsError> {
    let issues = crate::validate::validate(Path::new(file_name), bytes);
    json(&crate::validate::to_json(&issues))
}

/// A `.dvw` as JSON in openvolley's plays layout.
#[wasm_bindgen]
pub fn plays(bytes: &[u8]) -> Result<String, JsError> {
    json(&crate::plays::to_json(&parse(bytes)?))
}

/// Set scores and skill statistics of a `.dvw` as JSON. `symbols` is
/// `"datavolley"` or `"volleystation"`.
#[wasm_bindgen]
pub fn stats(bytes: &[u8], symbols: &str) -> Result<String, JsError> {
    let symbols = Symbols::from_name(symbols)
        .ok_or_else(|| JsError::new("symbols must be \"datavolley\" or \"volleystation\""))?;
    json(&crate::stats::report(&parse(bytes)?, symbols).to_json())
}
