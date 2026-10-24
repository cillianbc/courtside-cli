//! Lossless read and write of DataVolley `.dvw` and VolleyStation `.vsm` files.
//!
//! The format is undocumented. The golden corpus in `testdata/` is the
//! specification and openvolley is the oracle; every quirk found goes in
//! `docs/FORMAT-NOTES.md`.

pub mod date;
pub mod dvw;
pub mod encoding;
pub mod scout;
pub mod sections;
pub mod team_file;
pub mod text;
pub mod vsm;

pub use date::Date;
pub use dvw::Dvw;
pub use encoding::TextEncoding;
pub use sections::RawFile;
pub use vsm::RawVsm;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid .vsm JSON: {0}")]
    VsmJson(#[from] serde_json::Error),
    #[error("unknown code page {0:?} in [3MATCH]; pass an encoding explicitly")]
    UnknownCodepage(String),
    /// DataVolley 2 / pre-2007 layout, first line `[Match]`.
    #[error("legacy DataVolley file (pre-2007 [Match] format); not supported")]
    LegacyFormat,
    /// No `[3…]` section at all: DataVolley's encrypted scout files look
    /// like this, as does anything that isn't a scout file.
    #[error("not a readable DataVolley file: no [3...] sections (encrypted, or another format)")]
    NotDataVolley,
    #[error("[{section}] line {line} is not valid {encoding}")]
    Undecodable {
        encoding: &'static str,
        section: String,
        line: usize,
    },
    #[error("[{section}] line {line} has text that {encoding} cannot represent")]
    Unencodable {
        encoding: &'static str,
        section: String,
        line: usize,
    },
}
