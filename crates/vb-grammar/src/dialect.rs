//! What each evaluation symbol means, per skill and dialect (PRD I-06).
//!
//! The standard table gives each symbol's usual meaning in our own words;
//! the volleymetrics and german overrides follow openvolley's
//! `skill_evaluation_decoder`. A season
//! can start from any dialect and override individual entries.

use std::collections::HashMap;

use crate::code::Skill;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    Standard,
    Volleymetrics,
    German,
}

const STANDARD: &[(char, char, &str)] = &[
    ('S', '=', "Error"),
    ('S', '/', "Good, the pass can't be attacked"),
    ('S', '-', "Weak, an easy pass"),
    ('S', '+', "Good, the pass is under pressure"),
    ('S', '#', "Ace"),
    ('S', '!', "Fair, the quick attack is off"),
    ('R', '=', "Error"),
    ('R', '/', "Poor, no attack from it"),
    ('R', '-', "Weak, few attack options"),
    ('R', '+', "Good, an attack from it"),
    ('R', '#', "Perfect"),
    ('R', '!', "Fair, the quick attack is off"),
    ('A', '=', "Error"),
    ('A', '/', "Blocked for the point"),
    ('A', '-', "Weak, dug easily"),
    ('A', '!', "Blocked back, attacked again"),
    ('A', '+', "Good, kept in play"),
    ('A', '#', "Kill"),
    ('B', '=', "Error"),
    ('B', '/', "Over the net"),
    ('B', '-', "Weak, the attackers play again"),
    ('B', '+', "Good, a touch"),
    ('B', '#', "Stuff block"),
    ('B', '!', "Weak, the attackers play again"),
    ('D', '=', "Error"),
    ('D', '/', "Straight back over the net"),
    ('D', '-', "Kept up, no set attack"),
    ('D', '#', "Perfect"),
    ('D', '+', "Good"),
    ('D', '!', "Fair, the quick attack is off"),
    ('E', '=', "Error"),
    ('E', '-', "Poor"),
    ('E', '/', "Poor"),
    ('E', '+', "Good"),
    ('E', '#', "Perfect"),
    ('E', '!', "Fair"),
    ('F', '=', "Error"),
    ('F', '/', "Poor"),
    ('F', '!', "Fair, the quick attack is off"),
    ('F', '-', "Fair, only a high set"),
    ('F', '+', "Good"),
    ('F', '#', "Perfect"),
];

const VOLLEYMETRICS: &[(char, char, &str)] = &[
    ('B', '/', "Poor, opposition to replay"),
    ('B', '!', "Poor, blocking team cannot recover"),
    ('B', '-', "Poor block"),
    ('B', '+', "Positive block"),
    ('D', '/', "Positive block cover"),
    ('D', '!', "Poor block cover"),
    ('E', '/', "Error"),
];

const GERMAN: &[(char, char, &str)] = &[
    ('B', '/', "Error"),
    ('B', '=', "Invasion"),
    ('B', '-', "Poor block"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationTable {
    meanings: HashMap<(Skill, char), String>,
}

impl EvaluationTable {
    pub fn new(dialect: Dialect) -> Self {
        let overrides = match dialect {
            Dialect::Standard => &[][..],
            Dialect::Volleymetrics => VOLLEYMETRICS,
            Dialect::German => GERMAN,
        };
        let mut table = EvaluationTable {
            meanings: HashMap::new(),
        };
        for &(skill, symbol, meaning) in STANDARD.iter().chain(overrides) {
            if let Some(skill) = Skill::from_char(skill) {
                table.set(skill, symbol, meaning);
            }
        }
        table
    }

    /// Meaning of `symbol` for `skill`, or `None` if the table has no entry.
    pub fn meaning(&self, skill: Skill, symbol: char) -> Option<&str> {
        self.meanings.get(&(skill, symbol)).map(String::as_str)
    }

    /// Adds or replaces one entry, for a season's custom table.
    pub fn set(&mut self, skill: Skill, symbol: char, meaning: &str) {
        self.meanings.insert((skill, symbol), meaning.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_meanings() {
        let t = EvaluationTable::new(Dialect::Standard);
        assert_eq!(t.meaning(Skill::Serve, '#'), Some("Ace"));
        assert_eq!(t.meaning(Skill::Block, '/'), Some("Over the net"));
        assert_eq!(t.meaning(Skill::Attack, '?'), None);
        assert_eq!(t.meaning(Skill::Green, '#'), None);
    }

    #[test]
    fn dialects_override_standard() {
        let g = EvaluationTable::new(Dialect::German);
        assert_eq!(g.meaning(Skill::Block, '/'), Some("Error"));
        assert_eq!(g.meaning(Skill::Block, '='), Some("Invasion"));
        assert_eq!(g.meaning(Skill::Serve, '#'), Some("Ace"));
        let v = EvaluationTable::new(Dialect::Volleymetrics);
        assert_eq!(v.meaning(Skill::Set, '/'), Some("Error"));
    }

    #[test]
    fn custom_entries() {
        let mut t = EvaluationTable::new(Dialect::Standard);
        t.set(Skill::Reception, '!', "Overpass");
        assert_eq!(t.meaning(Skill::Reception, '!'), Some("Overpass"));
    }

    #[test]
    fn every_skill_has_all_six_symbols() {
        let t = EvaluationTable::new(Dialect::Standard);
        for skill in "SRABDEF".chars().filter_map(Skill::from_char) {
            for symbol in "=/-!+#".chars() {
                assert!(t.meaning(skill, symbol).is_some(), "{skill:?} {symbol}");
            }
        }
    }
}
