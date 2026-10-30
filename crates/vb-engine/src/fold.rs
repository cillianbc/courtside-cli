//! The fold: visible events in, match state per event out.
//!
//! Rules, each checked against the corpus (FORMAT-NOTES "Engine fold"):
//! - A set's lineup comes from the team sheets' starting zones and applies
//!   from the first event after the previous set end (or the first event).
//!   A [`StartingSix`](crate::event::Body::StartingSix) event (a lineup
//!   entered in the console) replaces that team's six where it stands.
//! - `z` without `>LUp` rotates the court so the current setter is in that
//!   zone (or the setter named just after it, under some [`Conventions`](crate::model::Conventions)); if the setter isn't known or on court, by the change in setter
//!   position.
//! - `P` sets the setter; `c` swaps a player on court in place, and the
//!   player coming on for the setter is the setter.
//! - Point codes carry the score; one that can't be read adds a point.
//! - State is what holds *after* the event, as in the `.dvw` columns.
//!
//! The fold never rejects anything; the validator checks the rules.

use vb_grammar::code::Digits;
use vb_grammar::{Code, Dialect, EvaluationTable, Skill, Team};

use crate::event::{Body, EventKind, MatchEvent};
use crate::model::{Match, Sides};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Score {
    pub home: u8,
    pub visiting: u8,
}

impl Score {
    pub fn get(&self, team: Team) -> u8 {
        match team {
            Team::Home => self.home,
            Team::Visiting => self.visiting,
        }
    }
}

/// Match state after one event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// The set in progress, from 1. A set end moves it on, so the set end
    /// event itself already shows the next set (as `.dvw` does).
    pub set: u8,
    /// Score within the set.
    pub score: Score,
    /// Zone (1–6) of each team's setter; `None` until known in this set.
    pub setter_position: Sides<Option<u8>>,
    /// Shirt number of each team's setter; `None` until known in this set.
    pub setter: Sides<Option<u8>>,
    /// Shirt numbers in zones 1–6. All `None` on a set end.
    pub court: Sides<[Option<u8>; 6]>,
    /// The rally this event belongs to, as an index into [`Folded::rallies`].
    /// `None` for timeouts, set ends and comments.
    pub rally: Option<usize>,
    /// Consecutive events by one team within a rally, split at a second
    /// attack. `None` for events without a team or outside a rally.
    pub team_touch: Option<usize>,
    /// For skill touches (not green codes).
    pub phase: Option<Phase>,
    /// An attack that won the point outright or was dug or blocked in error.
    pub winning_attack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Serve,
    /// The receiving team's first contacts (reception and the set and
    /// attack that follow it), and a block of that attack.
    Reception,
    Transition,
}

/// A rally: from the first event after the previous point (including any
/// rotation, lineup or substitution codes) to its point code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rally {
    pub set: u8,
    /// Index of the first and last event, into the visible events.
    pub first: usize,
    pub last: usize,
    /// Team of the first serve in the rally.
    pub serving: Option<Team>,
    /// Team of the rally's point code.
    pub won_by: Option<Team>,
    pub score_before: Score,
    pub score_after: Score,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folded {
    /// One per visible event, in the same order.
    pub states: Vec<State>,
    pub rallies: Vec<Rally>,
    /// The set played as a golden set, if one was started.
    pub golden_set: Option<u8>,
}

/// Points that win a golden set.
pub const GOLDEN_SET_TARGET: u8 = 15;

impl Folded {
    /// Points needed to win `set`, with a two-point lead: the regulation's,
    /// or [`GOLDEN_SET_TARGET`] for a golden set.
    pub fn set_target(&self, m: &Match, set: u8) -> u8 {
        if self.golden_set == Some(set) {
            GOLDEN_SET_TARGET
        } else {
            m.regulation.set_target(set)
        }
    }
}

pub fn fold(m: &Match) -> Folded {
    fold_events(m, &m.log.visible())
}

pub fn fold_events(m: &Match, events: &[&MatchEvent]) -> Folded {
    let evaluations = EvaluationTable::new(Dialect::Standard);
    let mut states: Vec<State> = Vec::with_capacity(events.len());
    let mut rallies: Vec<Rally> = Vec::new();
    let mut golden_set = None;
    let mut st = State {
        set: 1,
        score: Score::default(),
        setter_position: Sides::default(),
        setter: Sides::default(),
        court: Sides::default(),
        rally: None,
        team_touch: None,
        phase: None,
        winning_attack: false,
    };
    let mut new_set = true;
    let mut open_rally: Option<usize> = None;
    // (team, rally, had an attack) of the current team touch.
    let mut touch: Option<(Team, usize, bool)> = None;
    let mut team_touches = 0usize;

    for (i, ev) in events.iter().enumerate() {
        let kind = ev.kind();
        let code = ev.code();
        if new_set && !matches!(kind, EventKind::Comment | EventKind::Removed) {
            new_set = false;
            st.score = Score::default();
            st.setter_position = Sides::default();
            st.setter = Sides::default();
            st.court = Sides {
                home: m.teams.home.starting_lineup(st.set),
                visiting: m.teams.visiting.starting_lineup(st.set),
            };
        }
        st.phase = None;
        st.winning_attack = false;

        // Rally membership.
        st.rally = match kind {
            EventKind::Timeout
            | EventKind::SetEnd
            | EventKind::Comment
            | EventKind::Removed
            | EventKind::StartingSix
            | EventKind::GoldenSet => None,
            _ => Some(*open_rally.get_or_insert_with(|| {
                rallies.push(Rally {
                    set: st.set,
                    first: i,
                    last: i,
                    serving: None,
                    won_by: None,
                    score_before: st.score,
                    score_after: st.score,
                });
                rallies.len() - 1
            })),
        };

        if let Body::StartingSix { team, court } = &ev.body {
            *st.court.get_mut(*team) = court.map(Some);
        }
        if ev.body == Body::GoldenSet {
            golden_set = Some(st.set);
        }
        match code {
            Some(Code::Point {
                team,
                home,
                visiting,
                ..
            }) => {
                st.score = match (shirt(home), shirt(visiting)) {
                    (Some(home), Some(visiting)) => Score { home, visiting },
                    _ => {
                        let mut s = st.score;
                        match team {
                            Team::Home => s.home = s.home.saturating_add(1),
                            Team::Visiting => s.visiting = s.visiting.saturating_add(1),
                        }
                        s
                    }
                };
            }
            Some(Code::SetterPlayer { team, player, .. }) => {
                *st.setter.get_mut(*team) = shirt(player);
            }
            Some(Code::SetterPosition { team, position, .. }) => {
                if let Some(p) = shirt(position).filter(|p| (1..=6).contains(p)) {
                    if kind == EventKind::SetterPosition {
                        let following = m
                            .conventions
                            .setter_change_follows_rotation
                            .then(|| following_setter_change(events, i, *team))
                            .flatten();
                        let setter = following.or(*st.setter.get(*team));
                        rotate_to(&mut st, *team, p, setter);
                    }
                    *st.setter_position.get_mut(*team) = Some(p);
                }
            }
            Some(Code::Substitution {
                team, out, r#in, ..
            }) => {
                if let (Some(out), Some(r#in)) = (shirt(out), shirt(r#in)) {
                    let court = st.court.get_mut(*team);
                    if let Some(slot) = court.iter_mut().find(|p| **p == Some(out)) {
                        *slot = Some(r#in);
                    }
                    // The player coming on takes the setter's place.
                    let setter = st.setter.get_mut(*team);
                    if *setter == Some(out) {
                        *setter = Some(r#in);
                    }
                }
            }
            Some(Code::SetEnd { .. }) => {
                st.set = st.set.saturating_add(1);
                st.court = Sides::default();
                new_set = true;
            }
            _ => {}
        }

        // Team touches: consecutive events by one team in one rally.
        st.team_touch = None;
        if let (Some(team), Some(rally)) = (code.and_then(Code::team), st.rally) {
            let is_attack = matches!(code, Some(Code::Touch(t)) if t.skill == Skill::Attack);
            let same = touch.is_some_and(|(t, r, had_attack)| {
                t == team && r == rally && !(is_attack && had_attack)
            });
            if !same {
                team_touches += 1;
                touch = Some((team, rally, false));
            }
            if let Some(t) = touch.as_mut() {
                t.2 = is_attack;
            }
            st.team_touch = Some(team_touches - 1);
        }

        if let Some(r) = st.rally.and_then(|r| rallies.get_mut(r)) {
            r.last = i;
            r.score_after = st.score;
            if let Some(Code::Touch(t)) = code {
                if t.skill == Skill::Serve && r.serving.is_none() {
                    r.serving = Some(t.team);
                }
            }
            if let Some(Code::Point { team, .. }) = code {
                r.won_by = Some(*team);
                open_rally = None;
            }
        }
        if kind == EventKind::SetEnd {
            open_rally = None;
        }
        states.push(st.clone());
    }

    // Phases need the whole team touch: a set or attack is in reception
    // phase if its team touch contains a reception.
    let mut reception_touches = std::collections::HashSet::new();
    for (ev, s) in events.iter().zip(&states) {
        if let (Some(Code::Touch(t)), Some(tt)) = (ev.code(), s.team_touch) {
            if t.skill == Skill::Reception {
                reception_touches.insert(tt);
            }
        }
    }
    for i in 0..states.len() {
        let Some(Code::Touch(t)) = events[i].code() else {
            continue;
        };
        if t.skill == Skill::Green {
            continue;
        }
        // A block straight after a reception-phase attack defends the
        // side-out, so it is in reception phase too.
        let blocks_reception_attack = t.skill == Skill::Block
            && i > 0
            && matches!(events[i - 1].code(), Some(Code::Touch(a)) if a.skill == Skill::Attack)
            && states[i - 1].phase == Some(Phase::Reception);
        let s = &mut states[i];
        s.phase = Some(if t.skill == Skill::Serve {
            Phase::Serve
        } else if blocks_reception_attack
            || s.team_touch
                .is_some_and(|tt| reception_touches.contains(&tt))
        {
            Phase::Reception
        } else {
            Phase::Transition
        });
        if t.skill == Skill::Attack {
            let won = t
                .evaluation()
                .and_then(|e| evaluations.meaning(Skill::Attack, e))
                == Some("Kill");
            let next_error = events.get(i + 1).and_then(|e| e.code()).is_some_and(|c| {
                matches!(c, Code::Touch(n)
                    if matches!(n.skill, Skill::Dig | Skill::Block)
                        && n.evaluation().and_then(|e| evaluations.meaning(n.skill, e)) == Some("Error"))
            });
            states[i].winning_attack = won || next_error;
        }
    }

    Folded {
        states,
        rallies,
        golden_set,
    }
}

/// A setter change for `team` written straight after the rotation at
/// `i`, before play resumes (see [`Conventions`](crate::model::Conventions)).
fn following_setter_change(events: &[&MatchEvent], i: usize, team: Team) -> Option<u8> {
    events
        .iter()
        .skip(i + 1)
        .take_while(|e| {
            matches!(
                e.kind(),
                EventKind::SetterChange | EventKind::SetterPosition | EventKind::Substitution
            )
        })
        .find_map(|e| match e.code() {
            Some(Code::SetterPlayer {
                team: t, player, ..
            }) if *t == team => shirt(player),
            _ => None,
        })
}

/// A shirt number, score or zone that fits in a `u8`.
fn shirt(d: &Digits) -> Option<u8> {
    d.value().and_then(|v| u8::try_from(v).ok())
}

/// Rotates `team` so `setter` ends up in zone `p`; by the change in setter
/// position if the setter isn't known or isn't on court.
fn rotate_to(st: &mut State, team: Team, p: u8, setter: Option<u8>) {
    let court = st.court.get_mut(team);
    let setter_zone = setter
        .and_then(|s| court.iter().position(|c| *c == Some(s)))
        .map(|i| i + 1);
    let from = setter_zone.or_else(|| st.setter_position.get(team).map(usize::from));
    if let Some(from) = from {
        let d = (from + 6 - usize::from(p)) % 6;
        court.rotate_left(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{IdGen, NewEvent, Producer};
    use crate::model::{RosterPlayer, StartingZone, TeamSheet};

    fn sheet(lineup: [u8; 6]) -> TeamSheet {
        TeamSheet {
            players: lineup
                .iter()
                .enumerate()
                .map(|(i, n)| RosterPlayer {
                    number: *n,
                    starting_zones: [StartingZone::Zone(i as u8 + 1); 5],
                    libero: false,
                    role: None,
                })
                .collect(),
        }
    }

    fn game(codes: &[&str]) -> (Match, Folded) {
        let mut m = Match {
            teams: Sides {
                home: sheet([8, 12, 13, 11, 3, 14]),
                visiting: sheet([16, 11, 18, 6, 10, 13]),
            },
            ..Match::default()
        };
        let mut ids = IdGen::new(0);
        for c in codes {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        let f = fold(&m);
        (m, f)
    }

    #[test]
    fn a_starting_six_replaces_the_team_sheet_lineup() {
        let (mut m, _) = game(&["*08SH-", "a13RH#", "a10AH#", "ap00:01", "**1set"]);
        let mut ids = IdGen::new(1);
        m.log.append(
            &mut ids,
            NewEvent {
                body: Body::StartingSix {
                    team: Team::Home,
                    court: [3, 8, 12, 13, 11, 14],
                },
                video_ms: None,
                clock: None,
                producer: Producer::Keyboard,
            },
        );
        let f = fold(&m);
        let last = f.states.last().expect("state");
        assert_eq!(last.set, 2);
        assert_eq!(last.court.home, [3, 8, 12, 13, 11, 14].map(Some));
        assert_eq!(
            last.court.visiting,
            [16, 11, 18, 6, 10, 13].map(Some),
            "visiting keeps the team sheet's six"
        );
        assert_eq!(last.rally, None);
    }

    #[test]
    fn rotation_score_and_rallies() {
        let (_, f) = game(&[
            "*P08>LUp", "*z1>LUp", "aP11>LUp", "az2>LUp", // lineup
            "*08SH-", "a13RH#", "a10AH#", "*$$&H=", "ap00:01", // side-out
            "az1", "a11SH=", "*$$&H#", "*p01:01", // side-out
            "*z6", "*c13:05", "*12SH+", "a18RH-", "*T", "**1set", "*P08>LUp",
        ]);
        let s = &f.states;
        assert_eq!(
            s[3].setter_position,
            Sides {
                home: Some(1),
                visiting: Some(2)
            }
        );
        assert_eq!(s[3].court.home, [8, 12, 13, 11, 3, 14].map(Some));
        assert_eq!(
            s[8].score,
            Score {
                home: 0,
                visiting: 1
            }
        );
        // az1: setter 11 from zone 2 to zone 1.
        assert_eq!(s[9].court.visiting, [11, 18, 6, 10, 13, 16].map(Some));
        assert_eq!(s[9].setter_position.visiting, Some(1));
        // *z6, then 13 replaced by 5 in place.
        assert_eq!(s[14].court.home, [12, 5, 11, 3, 14, 8].map(Some));
        assert_eq!(s[17].rally, None, "timeout");
        assert_eq!(s[18].set, 2);
        assert_eq!(s[18].court.home, [None; 6]);
        assert_eq!(s[19].court.home, [8, 12, 13, 11, 3, 14].map(Some));
        assert_eq!(s[19].score, Score::default());
        assert_eq!(s[19].setter_position.home, None);

        assert_eq!(f.rallies.len(), 4);
        let r = &f.rallies[0];
        assert_eq!(
            (r.first, r.last, r.serving, r.won_by),
            (0, 8, Some(Team::Home), Some(Team::Visiting))
        );
        assert_eq!(
            f.rallies[1].score_before,
            Score {
                home: 0,
                visiting: 1
            }
        );

        assert_eq!(s[4].phase, Some(Phase::Serve));
        assert_eq!(s[5].phase, Some(Phase::Reception));
        assert_eq!(s[6].phase, Some(Phase::Reception));
        assert!(s[6].winning_attack);
        assert_eq!(s[7].phase, None, "green code");
    }

    #[test]
    fn a_substitute_for_the_setter_is_the_setter() {
        let (_, f) = game(&["*P08>LUp", "*z1>LUp", "*c08:05", "*c12:07"]);
        assert_eq!(f.states[2].setter.home, Some(5));
        assert_eq!(f.states[3].setter.home, Some(5), "another player off");
        assert_eq!(f.states[3].court.home[0], Some(5));
    }

    #[test]
    fn rotation_follows_a_setter_change() {
        // 3 (zone 5) becomes setter; z4 is one rotation, not three.
        let (_, f) = game(&["*P08>LUp", "*z1>LUp", "*P03", "*z4"]);
        assert_eq!(f.states[3].court.home, [12, 13, 11, 3, 14, 8].map(Some));
    }
}
