//! Match events: the append-only record a match is folded from (PRD
//! domain model). Score, rotation and everything else derived is computed
//! from these, never stored.

use std::fmt;

use ulid::Ulid;
use vb_grammar::{Code, Team};

/// Unique, sortable event id. Ids from one [`IdGen`] sort in creation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(pub Ulid);

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Makes monotonic [`EventId`]s from a fixed timestamp and a counter, so
/// the engine needs no clock or randomness (it also runs in WASM). The
/// caller supplies the time; ids are unique per generator.
#[derive(Debug, Clone)]
pub struct IdGen {
    time_ms: u64,
    counter: u128,
}

impl IdGen {
    pub fn new(time_ms: u64) -> Self {
        IdGen {
            time_ms,
            counter: 0,
        }
    }

    pub fn next_id(&mut self) -> EventId {
        self.counter += 1;
        EventId(Ulid::from_parts(self.time_ms, self.counter))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Producer {
    Keyboard,
    Tap,
    Import(ImportFormat),
    /// Written by the engine or a repair tool, not a person.
    Repair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Dvw,
    Vsm,
}

/// Wall-clock time of day, as written in the `[3SCOUT]` time column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Code(Code),
    /// Free text from the scout; ignored by the fold.
    Comment(String),
    /// A correction that removes its target without replacing it.
    Removed,
    /// A team's six at the start of a set, zones 1–6, entered in the
    /// console. Imported matches take theirs from the team sheet's
    /// starting zones; this event overrides those for its set. It has no
    /// `[3SCOUT]` line of its own: export writes it as starting zones.
    StartingSix {
        team: Team,
        court: [u8; 6],
    },
    /// The set after it is a golden set (CEV two-legged ties): played
    /// after the match is decided, to 15. Entered in the console. How
    /// DataVolley marks one in a `.dvw` is not known yet (no file in the
    /// corpus has one); export writes no `[3SCOUT]` line for it.
    GoldenSet,
}

/// What an event means to the fold, derived from its [`Body`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    Touch,
    Point,
    /// A setter or setter position code marked `>LUp`: part of the
    /// starting lineup of a set.
    Lineup,
    /// `*P08` without `>LUp`: a change of setter (FORMAT-NOTES "Decisions").
    SetterChange,
    /// `*z1` without `>LUp`: the setter is now in this zone (a rotation).
    SetterPosition,
    Substitution,
    Timeout,
    SetEnd,
    Comment,
    /// Never seen by the fold: see [`Body::Removed`].
    Removed,
    /// See [`Body::StartingSix`].
    StartingSix,
    /// See [`Body::GoldenSet`].
    GoldenSet,
    Unrecognised,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchEvent {
    pub id: EventId,
    /// Position in the log, from 0.
    pub seq: u32,
    pub body: Body,
    /// Offset into the primary camera.
    pub video_ms: Option<u64>,
    pub clock: Option<ClockTime>,
    pub producer: Producer,
    /// Set on a correction: the event this one replaces. The replaced
    /// event stays in the log but the fold skips it.
    pub supersedes: Option<EventId>,
    /// Set on an insertion: the event this one goes before in the match.
    /// Always an original or inserted event, never a correction.
    pub before: Option<EventId>,
    /// For imported events, the index of the source `[3SCOUT]` line.
    pub source_line: Option<usize>,
    /// Where the play started and ended on the court, as `.dvw` grid
    /// cells ([`crate::court`]). A stamp like the video time: set in place,
    /// kept by a correction.
    pub points: Points,
    /// On a set end: the set's length in minutes as the scout gave it,
    /// which the export writes in place of the clock's. A stamp like
    /// `points`: set in place, kept by a correction (#21).
    pub minutes: Option<u16>,
}

/// A play's court positions as `.dvw` grid cells ([`crate::court`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Points {
    pub start: Option<u16>,
    pub mid: Option<u16>,
    pub end: Option<u16>,
}

impl Points {
    /// A `[3SCOUT]` line's columns 4–6, keeping only cells on the grid.
    pub fn of_line(line: &dvw_codec::scout::ScoutLine) -> Self {
        let cell = |c: Option<u16>| c.filter(|c| crate::court::position(*c).is_some());
        Points {
            start: cell(line.start_coordinate()),
            mid: cell(line.mid_coordinate()),
            end: cell(line.end_coordinate()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start.is_none() && self.mid.is_none() && self.end.is_none()
    }
}

impl MatchEvent {
    pub fn code(&self) -> Option<&Code> {
        match &self.body {
            Body::Code(c) => Some(c),
            Body::Comment(_) | Body::Removed | Body::StartingSix { .. } | Body::GoldenSet => None,
        }
    }

    pub fn kind(&self) -> EventKind {
        let Some(code) = self.code() else {
            return match self.body {
                Body::Removed => EventKind::Removed,
                Body::StartingSix { .. } => EventKind::StartingSix,
                Body::GoldenSet => EventKind::GoldenSet,
                _ => EventKind::Comment,
            };
        };
        match code {
            Code::Touch(_) => EventKind::Touch,
            Code::Point { .. } => EventKind::Point,
            Code::SetterPosition { note, .. } | Code::SetterPlayer { note, .. }
                if is_lineup(note) =>
            {
                EventKind::Lineup
            }
            Code::SetterPosition { .. } => EventKind::SetterPosition,
            Code::SetterPlayer { .. } => EventKind::SetterChange,
            Code::Substitution { .. } => EventKind::Substitution,
            Code::Timeout { .. } => EventKind::Timeout,
            Code::SetEnd { .. } => EventKind::SetEnd,
            // A free-text line among the codes: no part of play.
            Code::Comment(_) => EventKind::Comment,
            Code::Unrecognised(_) => EventKind::Unrecognised,
        }
    }
}

/// `>LUp` (any case) marks a starting-lineup code.
pub(crate) fn is_lineup(note: &str) -> bool {
    note.to_ascii_lowercase().contains(">lup")
}

/// Everything a new event needs except what the log assigns (id, seq).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub body: Body,
    pub video_ms: Option<u64>,
    pub clock: Option<ClockTime>,
    pub producer: Producer,
}

impl NewEvent {
    pub fn code(code: Code, producer: Producer) -> Self {
        NewEvent {
            body: Body::Code(code),
            video_ms: None,
            clock: None,
            producer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_monotonic() {
        let mut ids = IdGen::new(1_700_000_000_000);
        let a = ids.next_id();
        let b = ids.next_id();
        assert!(a < b);
    }

    #[test]
    fn kinds() {
        let kind = |s: &str| {
            MatchEvent {
                id: IdGen::new(0).next_id(),
                seq: 0,
                body: Body::Code(Code::parse(s)),
                video_ms: None,
                clock: None,
                producer: Producer::Keyboard,
                supersedes: None,
                before: None,
                source_line: None,
                points: Points::default(),
                minutes: None,
            }
            .kind()
        };
        assert_eq!(kind("*P08>LUp"), EventKind::Lineup);
        assert_eq!(kind("az2>LUp"), EventKind::Lineup);
        assert_eq!(kind("aP11"), EventKind::SetterChange);
        assert_eq!(kind("*z6"), EventKind::SetterPosition);
        assert_eq!(kind("a10AH#"), EventKind::Touch);
        assert_eq!(kind("*p25:20"), EventKind::Point);
        assert_eq!(kind("**1set"), EventKind::SetEnd);
        assert_eq!(kind("??"), EventKind::Unrecognised);
    }
}
