//! A match: the two team sheets and the event log.

use vb_grammar::{Skill, Team};

use crate::log::EventLog;

/// Where a player starts a set, as in the `.dvw` player table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartingZone {
    /// On court in zone 1–6 at the start of the set.
    Zone(u8),
    /// Came on as a substitute.
    Substitute,
    /// Did not play, or not recorded.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterPlayer {
    pub number: u8,
    /// Sets 1–5.
    pub starting_zones: [StartingZone; 5],
    pub libero: bool,
    /// `.dvw` role: 1 libero, 2 outside, 3 opposite, 4 middle, 5 setter,
    /// 6 other; `None` when the team sheet doesn't say.
    pub role: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamSheet {
    pub players: Vec<RosterPlayer>,
}

impl TeamSheet {
    pub fn player(&self, number: u8) -> Option<&RosterPlayer> {
        self.players.iter().find(|p| p.number == number)
    }

    pub fn is_libero(&self, number: u8) -> bool {
        self.player(number).is_some_and(|p| p.libero)
    }

    /// Whether the team sheet says `number` played in `set` (1-based):
    /// started in a zone or came on as a substitute.
    pub fn played_in(&self, number: u8, set: u8) -> bool {
        let Some(i) = usize::from(set).checked_sub(1) else {
            return false;
        };
        self.player(number).is_some_and(|p| {
            matches!(
                p.starting_zones.get(i),
                Some(StartingZone::Zone(_) | StartingZone::Substitute)
            )
        })
    }

    /// Players in zones 1–6 at the start of `set` (1-based). Zones nobody
    /// starts in are `None`.
    pub fn starting_lineup(&self, set: u8) -> [Option<u8>; 6] {
        let mut court = [None; 6];
        let Some(i) = usize::from(set).checked_sub(1) else {
            return court;
        };
        for p in &self.players {
            if let Some(StartingZone::Zone(z @ 1..=6)) = p.starting_zones.get(i) {
                court[usize::from(*z) - 1] = Some(p.number);
            }
        }
        court
    }
}

/// A pair of values, one per team.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Sides<T> {
    pub home: T,
    pub visiting: T,
}

impl<T> Sides<T> {
    pub fn get(&self, team: Team) -> &T {
        match team {
            Team::Home => &self.home,
            Team::Visiting => &self.visiting,
        }
    }

    pub fn get_mut(&mut self, team: Team) -> &mut T {
        match team {
            Team::Home => &mut self.home,
            Team::Visiting => &mut self.visiting,
        }
    }
}

/// How the program that wrote the codes orders them, where that changes
/// their meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Conventions {
    /// A setter change written straight after a rotation (`*z2`, `*P21`)
    /// names the setter that rotation refers to. VolleyStation does this;
    /// in DataVolley the rotation refers to the setter before the change.
    pub setter_change_follows_rotation: bool,
}

/// Scoring system, from `[3MATCH]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Regulation {
    /// Rally point, sets to 25 (deciding set 15), best of five.
    #[default]
    IndoorRallyPoint,
    /// Side-out scoring: only the serving team scores.
    IndoorSideout,
    /// Sets to 21 (deciding set 15), best of three.
    Beach,
}

impl Regulation {
    /// Sets needed to win the match.
    pub fn sets_to_win(self) -> u8 {
        match self {
            Regulation::Beach => 2,
            _ => 3,
        }
    }

    /// Points needed to win `set` (1-based), with a two-point lead.
    pub fn set_target(self, set: u8) -> u8 {
        let deciding = set == self.sets_to_win() * 2 - 1;
        match self {
            _ if deciding => 15,
            Regulation::Beach => 21,
            Regulation::IndoorSideout => 15,
            Regulation::IndoorRallyPoint => 25,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The touching team wins the rally.
    Win,
    /// The touching team loses the rally.
    Lose,
}

/// Which evaluations end a rally, per skill (`[3WINNINGSYMBOLS]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningSymbols {
    /// (skill, losing symbols, winning symbols)
    table: Vec<(Skill, Vec<char>, Vec<char>)>,
}

impl Default for WinningSymbols {
    /// DataVolley's defaults, as in every corpus file.
    fn default() -> Self {
        let row =
            |skill, lose: &str, win: &str| (skill, lose.chars().collect(), win.chars().collect());
        WinningSymbols {
            table: vec![
                row(Skill::Serve, "=", "#"),
                row(Skill::Reception, "=", ""),
                row(Skill::Attack, "=/", "#"),
                row(Skill::Block, "=/", "#"),
                row(Skill::Dig, "=", ""),
                row(Skill::Set, "=", ""),
                row(Skill::Freeball, "=", ""),
            ],
        }
    }
}

impl WinningSymbols {
    pub fn new(table: Vec<(Skill, Vec<char>, Vec<char>)>) -> Self {
        WinningSymbols { table }
    }

    pub fn outcome(&self, skill: Skill, evaluation: char) -> Option<Outcome> {
        let (_, lose, win) = self.table.iter().find(|(s, ..)| *s == skill)?;
        if win.contains(&evaluation) {
            Some(Outcome::Win)
        } else if lose.contains(&evaluation) {
            Some(Outcome::Lose)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Match {
    pub teams: Sides<TeamSheet>,
    pub regulation: Regulation,
    pub winning_symbols: WinningSymbols,
    pub conventions: Conventions,
    /// The competition's rule options the validator checks against.
    pub rules: Rules,
    pub log: EventLog,
}

/// Rule options that differ between competitions; set from the season
/// settings, not read from a `.dvw`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// Substitutions per team and set (6 under FIVB rules; some leagues
    /// allow more).
    pub substitutions: u8,
    /// A libero may serve (allowed in some national competitions).
    pub libero_may_serve: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            substitutions: 6,
            libero_may_serve: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineup_from_starting_zones() {
        let p = |number, zones: [StartingZone; 5]| RosterPlayer {
            number,
            starting_zones: zones,
            libero: false,
            role: None,
        };
        use StartingZone::{None as N, Substitute as S, Zone as Z};
        let sheet = TeamSheet {
            players: vec![
                p(8, [Z(1), Z(6), N, N, N]),
                p(12, [Z(2), Z(1), N, N, N]),
                p(5, [S, Z(2), N, N, N]),
            ],
        };
        assert_eq!(
            sheet.starting_lineup(1),
            [Some(8), Some(12), None, None, None, None]
        );
        assert_eq!(
            sheet.starting_lineup(2),
            [Some(12), Some(5), None, None, None, Some(8)]
        );
        assert_eq!(sheet.starting_lineup(0), [None; 6]);
        assert_eq!(sheet.starting_lineup(6), [None; 6]);
    }

    #[test]
    fn set_targets() {
        let r = Regulation::IndoorRallyPoint;
        assert_eq!(
            (r.set_target(1), r.set_target(4), r.set_target(5)),
            (25, 25, 15)
        );
        let b = Regulation::Beach;
        assert_eq!((b.set_target(2), b.set_target(3)), (21, 15));
    }

    #[test]
    fn default_winning_symbols() {
        let w = WinningSymbols::default();
        assert_eq!(w.outcome(Skill::Serve, '#'), Some(Outcome::Win));
        assert_eq!(w.outcome(Skill::Attack, '/'), Some(Outcome::Lose));
        assert_eq!(w.outcome(Skill::Reception, '#'), None);
    }
}
