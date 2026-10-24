//! Parser for the DataVolley positional scouting code, with dialect tables,
//! and the console's shorthand: default expansion and compound (dot) codes
//! ([`expand`]).

pub mod code;
pub mod dialect;
pub mod expand;

pub use code::{Code, Player, Skill, Team, Touch};
pub use dialect::{Dialect, EvaluationTable};
