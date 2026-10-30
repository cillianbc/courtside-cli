//! Match statistics from the fold: per-set scores and, per player and per
//! team, how often each skill was played with each evaluation.
//!
//! "Efficiency" is not one number across tools: DataVolley and
//! VolleyStation count different positive/negative symbols per skill.
//! Both are here as [`SymbolSets`]; whatever reports a
//! number should name the set and the formula it used.

use std::collections::BTreeMap;

use vb_grammar::{Code, Player, Skill, Team};

use crate::fold::fold_events;
use crate::model::Match;

/// Skills in the order DataVolley lists them.
pub const SKILLS: [Skill; 7] = [
    Skill::Serve,
    Skill::Reception,
    Skill::Attack,
    Skill::Block,
    Skill::Dig,
    Skill::Set,
    Skill::Freeball,
];

/// Which evaluations count as positive and negative for each skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolSets {
    pub name: &'static str,
    /// (skill, positive symbols, negative symbols), in [`SKILLS`] order.
    pub skills: [(Skill, &'static str, &'static str); 7],
}

impl SymbolSets {
    /// DataVolley 4's default efficiency table.
    pub const DATAVOLLEY: SymbolSets = SymbolSets {
        name: "DataVolley 4 defaults",
        skills: [
            (Skill::Serve, "#/", "="),
            (Skill::Reception, "#+", "="),
            (Skill::Attack, "#", "/="),
            (Skill::Block, "#", "/="),
            (Skill::Dig, "#+", "/="),
            (Skill::Set, "#+", "/="),
            (Skill::Freeball, "#+", "/="),
        ],
    };

    /// VolleyStation Pro's default winning/losing symbols.
    pub const VOLLEYSTATION: SymbolSets = SymbolSets {
        name: "VolleyStation Pro defaults",
        skills: [
            (Skill::Serve, "#+/", "="),
            (Skill::Reception, "#+", ""),
            (Skill::Attack, "#", "/="),
            (Skill::Block, "#+", "/="),
            (Skill::Dig, "#+/", ""),
            (Skill::Set, "#+", "/="),
            (Skill::Freeball, "#+", "/="),
        ],
    };

    /// Positive and negative symbols for `skill`.
    pub fn for_skill(&self, skill: Skill) -> (&'static str, &'static str) {
        self.skills
            .iter()
            .find(|(s, _, _)| *s == skill)
            .map_or(("", ""), |(_, p, n)| (*p, *n))
    }
}

/// How often one skill was played, by evaluation symbol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: u32,
    /// Evaluation symbol (`#`, `+`, `!`, `-`, `/`, `=`) → count. Touches
    /// without an evaluation count only in `total`.
    pub by_evaluation: BTreeMap<char, u32>,
}

impl Counts {
    fn add(&mut self, evaluation: Option<char>) {
        self.total += 1;
        if let Some(e) = evaluation {
            *self.by_evaluation.entry(e).or_default() += 1;
        }
    }

    /// Touches whose evaluation is one of `symbols`.
    pub fn with(&self, symbols: &str) -> u32 {
        symbols
            .chars()
            .filter_map(|c| self.by_evaluation.get(&c))
            .sum()
    }

    /// Positive touches / total.
    pub fn positive_rate(&self, skill: Skill, sets: &SymbolSets) -> Option<f64> {
        let (positive, _) = sets.for_skill(skill);
        (self.total > 0).then(|| f64::from(self.with(positive)) / f64::from(self.total))
    }

    /// (positive − negative) / total.
    pub fn efficiency(&self, skill: Skill, sets: &SymbolSets) -> Option<f64> {
        let (positive, negative) = sets.for_skill(skill);
        (self.total > 0).then(|| {
            (f64::from(self.with(positive)) - f64::from(self.with(negative)))
                / f64::from(self.total)
        })
    }
}

/// Counts per skill letter.
pub type SkillCounts = BTreeMap<char, Counts>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerStats {
    pub team: Team,
    pub number: u8,
    pub skills: SkillCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetScore {
    pub set: u8,
    pub home: u8,
    pub visiting: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchStats {
    /// Final score of each set with at least one rally.
    pub sets: Vec<SetScore>,
    /// Players with at least one touch, home first, by shirt number.
    pub players: Vec<PlayerStats>,
    /// Team totals, including touches by an unknown player (`$$`).
    pub home: SkillCounts,
    pub visiting: SkillCounts,
}

/// Statistics for the visible events. Green codes (`$$&`) are point
/// attributions, not touches, and are not counted.
pub fn stats(m: &Match) -> MatchStats {
    let events = m.log.visible();
    let folded = fold_events(m, &events);
    let mut out = MatchStats::default();
    for r in &folded.rallies {
        let score = SetScore {
            set: r.set,
            home: r.score_after.home,
            visiting: r.score_after.visiting,
        };
        match out.sets.last_mut() {
            Some(last) if last.set == r.set => *last = score,
            _ => out.sets.push(score),
        }
    }
    let mut players: BTreeMap<(u8, u8), SkillCounts> = BTreeMap::new();
    for e in &events {
        let Some(Code::Touch(t)) = e.code() else {
            continue;
        };
        if t.skill == Skill::Green {
            continue;
        }
        let skill = t.skill.as_char();
        let evaluation = t.evaluation();
        let team = match t.team {
            Team::Home => &mut out.home,
            Team::Visiting => &mut out.visiting,
        };
        team.entry(skill).or_default().add(evaluation);
        if let Player::Number(d) = &t.player
            && let Some(n) = d.value().and_then(|v| u8::try_from(v).ok())
        {
            let side = u8::from(t.team == Team::Visiting);
            players
                .entry((side, n))
                .or_default()
                .entry(skill)
                .or_default()
                .add(evaluation);
        }
    }
    out.players = players
        .into_iter()
        .map(|((side, number), skills)| PlayerStats {
            team: if side == 0 {
                Team::Home
            } else {
                Team::Visiting
            },
            number,
            skills,
        })
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{IdGen, NewEvent, Producer};

    #[test]
    fn counts_touches_scores_and_efficiency() {
        let mut m = Match::default();
        let mut ids = IdGen::new(0);
        for c in [
            "*08SH-", "a16RH#", "a18AH#", "*$$&H=", "ap00:01", "a11SH=", "*p01:01", "*12SH#",
            "a$$RH=", "*p02:01", "**1set", "a11SH+", "*16RH+", "*09AH=", "ap00:01",
        ] {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        let s = stats(&m);
        assert_eq!(
            s.sets,
            [
                SetScore {
                    set: 1,
                    home: 2,
                    visiting: 1
                },
                SetScore {
                    set: 2,
                    home: 0,
                    visiting: 1
                }
            ]
        );
        // The green code is not a touch; the unknown receiver counts for
        // the team only.
        assert_eq!(s.home[&'S'].total, 2);
        assert!(!s.home.contains_key(&'&'));
        assert_eq!(s.visiting[&'R'].total, 2);
        assert_eq!(s.players.len(), 7);
        let serve = &s.visiting[&'S'];
        assert_eq!((serve.total, serve.with("="), serve.with("+")), (2, 1, 1));
        // DataVolley: serve positive is `#/`, so `+` is neutral: (0 − 1) / 2.
        assert_eq!(
            serve.efficiency(Skill::Serve, &SymbolSets::DATAVOLLEY),
            Some(-0.5)
        );
        // VolleyStation counts `+` as positive: (1 − 1) / 2.
        assert_eq!(
            serve.efficiency(Skill::Serve, &SymbolSets::VOLLEYSTATION),
            Some(0.0)
        );
    }

    /// A dig error under a kill counts against the digger, as DataVolley
    /// counts it (the user's choice, #22); VolleyStation leaves it neutral.
    #[test]
    fn a_dig_error_counts_against_the_digger() {
        let mut m = Match::default();
        let mut ids = IdGen::new(0);
        for c in ["*11SH+", "a07RH+", "a04EH+", "a12AH#", "*06DH=", "ap00:01"] {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        let s = stats(&m);
        let dig = &s.home[&'D'];
        assert_eq!((dig.total, dig.with("=")), (1, 1));
        assert_eq!(
            dig.efficiency(Skill::Dig, &SymbolSets::DATAVOLLEY),
            Some(-1.0)
        );
        assert_eq!(
            dig.efficiency(Skill::Dig, &SymbolSets::VOLLEYSTATION),
            Some(0.0)
        );
        assert_eq!(s.visiting[&'A'].with("#"), 1);
    }
}
