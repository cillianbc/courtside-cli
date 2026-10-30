//! `[3SCOUT]` lines: the raw scouting code plus metadata columns.
//!
//! This layer splits columns only; the code string itself is parsed by
//! vb-grammar. Column meanings follow openvolley (`read_main` in
//! `datavolley/R/plays.R`); 0-based:
//!
//! | col   | meaning                                               |
//! |-------|-------------------------------------------------------|
//! | 0     | code                                                  |
//! | 1     | point phase: `p` breakpoint, `s` sideout              |
//! | 2     | attack phase: `r` reception, `s`/`p` transition       |
//! | 3     | ?                                                     |
//! | 4–6   | start, mid, end coordinate (`-1-1` = none)            |
//! | 7     | wall-clock time `HH.MM.SS`                            |
//! | 8     | set number                                            |
//! | 9, 10 | home, visiting setter position                        |
//! | 11    | video file number                                     |
//! | 12    | video time, seconds                                   |
//! | 13    | ?                                                     |
//! | 14–19 | home players on court, positions 1–6                  |
//! | 20–25 | visiting players on court, positions 1–6              |

use crate::text::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoutLine(pub Row);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointPhase {
    Breakpoint,
    Sideout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackPhase {
    Reception,
    TransitionSideout,
    TransitionBreakpoint,
}

impl ScoutLine {
    pub fn code(&self) -> &str {
        self.0.get(0)
    }
    pub fn point_phase(&self) -> Option<PointPhase> {
        match self.0.get(1) {
            "p" => Some(PointPhase::Breakpoint),
            "s" => Some(PointPhase::Sideout),
            _ => None,
        }
    }
    pub fn attack_phase(&self) -> Option<AttackPhase> {
        match self.0.get(2) {
            "r" => Some(AttackPhase::Reception),
            "s" => Some(AttackPhase::TransitionSideout),
            "p" => Some(AttackPhase::TransitionBreakpoint),
            _ => None,
        }
    }
    /// Court grid index, or `None` for empty, `-1-1` and other values with
    /// a `-` (openvolley treats e.g. `80-1` the same way).
    pub fn start_coordinate(&self) -> Option<u16> {
        coordinate(self.0.get(4))
    }
    pub fn mid_coordinate(&self) -> Option<u16> {
        coordinate(self.0.get(5))
    }
    pub fn end_coordinate(&self) -> Option<u16> {
        coordinate(self.0.get(6))
    }
    /// Wall-clock time as written, e.g. `15.33.42`.
    pub fn time(&self) -> &str {
        self.0.get(7)
    }
    /// `(hours, minutes, seconds)` from [`ScoutLine::time`].
    pub fn time_hms(&self) -> Option<(u8, u8, u8)> {
        let mut parts = self.time().split(['.', ':']);
        let mut next = || parts.next()?.trim().parse::<u8>().ok();
        let hms = (next()?, next()?, next()?);
        (hms.0 < 24 && hms.1 < 60 && hms.2 < 60).then_some(hms)
    }
    pub fn set_number(&self) -> Option<u8> {
        self.0.int(8)
    }
    pub fn home_setter_position(&self) -> Option<u8> {
        self.0.int(9)
    }
    pub fn visiting_setter_position(&self) -> Option<u8> {
        self.0.int(10)
    }
    pub fn video_file_number(&self) -> Option<u32> {
        self.0.int(11)
    }
    /// Seconds from the start of the video.
    pub fn video_time(&self) -> Option<f64> {
        self.0.int(12)
    }
    /// Shirt numbers on court in positions 1–6; `None` where empty.
    pub fn home_players(&self) -> [Option<u8>; 6] {
        std::array::from_fn(|i| self.0.int(14 + i))
    }
    pub fn visiting_players(&self) -> [Option<u8>; 6] {
        std::array::from_fn(|i| self.0.int(20 + i))
    }
}

fn coordinate(field: &str) -> Option<u16> {
    if field.contains('-') {
        return None;
    }
    field.trim().parse().ok().filter(|&c| c >= 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> ScoutLine {
        ScoutLine(Row::parse(s))
    }

    #[test]
    fn reads_columns() {
        let l = line(
            "a10AH#;s;r;;4868;-1-1;80-1;15.33.42;1;1;2;1;236;;8;12;13;11;3;14;16;11;18;6;10;13;",
        );
        assert_eq!(l.code(), "a10AH#");
        assert_eq!(l.point_phase(), Some(PointPhase::Sideout));
        assert_eq!(l.attack_phase(), Some(AttackPhase::Reception));
        assert_eq!(l.start_coordinate(), Some(4868));
        assert_eq!(l.mid_coordinate(), None);
        assert_eq!(l.end_coordinate(), None);
        assert_eq!(l.time_hms(), Some((15, 33, 42)));
        assert_eq!(l.set_number(), Some(1));
        assert_eq!(l.visiting_setter_position(), Some(2));
        assert_eq!(l.video_file_number(), Some(1));
        assert_eq!(l.video_time(), Some(236.0));
        assert_eq!(l.home_players(), [8, 12, 13, 11, 3, 14].map(Some));
        assert_eq!(l.visiting_players()[5], Some(13));
    }

    #[test]
    fn set_end_line_has_no_players() {
        let l = line("**3set;;;;;;;20.10.05;4;5;4;;;;;;;;;;;;;;;;");
        assert_eq!(l.code(), "**3set");
        assert_eq!(l.home_players(), [None; 6]);
        assert_eq!(l.point_phase(), None);
    }

    #[test]
    fn short_lines_are_fine() {
        let l = line("*z1");
        assert_eq!(l.time(), "");
        assert_eq!(l.time_hms(), None);
        assert_eq!(l.home_players(), [None; 6]);
    }
}
