//! Volleyball match engine. A match is an append-only log of events
//! ([`log::EventLog`]); score, rotation, lineups and rally phases are a
//! deterministic fold over it ([`fold::fold`]).

pub mod court;
pub mod event;
pub mod export;
pub mod fold;
mod import;
pub mod log;
pub mod model;
pub mod stats;
pub mod validate;

pub use event::{Body, EventId, EventKind, IdGen, MatchEvent, NewEvent, Points, Producer};
pub use fold::{Folded, Phase, Rally, Score, State, fold};
pub use model::{Conventions, Match, Rules, Sides, TeamSheet};
