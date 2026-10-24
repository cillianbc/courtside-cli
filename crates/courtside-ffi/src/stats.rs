//! Statistics (`courtside stats`): set scores and per-team and per-player skill counts
//! from vb-engine, as text or JSON. Rates name the symbol sets and the
//! formula they use, because DataVolley and VolleyStation define
//! "efficiency" differently.

use std::fmt::Write as _;

use dvw_codec::Dvw;
use serde_json::{Map, Value, json};
use vb_engine::stats::{MatchStats, SKILLS, SkillCounts, SymbolSets, stats};
use vb_engine::{IdGen, Match};
use vb_grammar::{Skill, Team};

const POSITIVE_RATE: &str = "positive / total";
const EFFICIENCY: &str = "(positive - negative) / total";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbols {
    /// DataVolley 4's default efficiency table
    Datavolley,
    /// VolleyStation Pro's default winning/losing symbols
    Volleystation,
}

impl Symbols {
    /// `"datavolley"` or `"volleystation"`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "datavolley" => Some(Symbols::Datavolley),
            "volleystation" => Some(Symbols::Volleystation),
            _ => None,
        }
    }

    fn sets(self) -> &'static SymbolSets {
        match self {
            Symbols::Datavolley => &SymbolSets::DATAVOLLEY,
            Symbols::Volleystation => &SymbolSets::VOLLEYSTATION,
        }
    }
}

pub struct Report {
    names: [String; 2],
    player_names: Vec<(Team, u8, String)>,
    stats: MatchStats,
    sets: &'static SymbolSets,
}

pub fn report(dvw: &Dvw, symbols: Symbols) -> Report {
    let m = Match::from_dvw(dvw, &mut IdGen::new(0));
    let names = [0, 1].map(|i| dvw.teams.get(i).map(|t| t.name()).unwrap_or_default());
    let mut player_names = Vec::new();
    for (team, roster) in [
        (Team::Home, &dvw.players_home),
        (Team::Visiting, &dvw.players_visiting),
    ] {
        for p in roster {
            if let Some(n) = p.number() {
                let name = format!("{} {}", p.first_name().trim(), p.last_name().trim());
                player_names.push((team, n, name.trim().to_owned()));
            }
        }
    }
    Report {
        names,
        player_names,
        stats: stats(&m),
        sets: symbols.sets(),
    }
}

fn side(team: Team) -> &'static str {
    match team {
        Team::Home => "home",
        Team::Visiting => "visiting",
    }
}

fn skill_name(skill: Skill) -> &'static str {
    match skill {
        Skill::Serve => "serve",
        Skill::Reception => "reception",
        Skill::Attack => "attack",
        Skill::Block => "block",
        Skill::Dig => "dig",
        Skill::Set => "set",
        Skill::Freeball => "freeball",
        Skill::Green => "green",
    }
}

/// Rounded to 3 decimals so the JSON stays readable.
fn rate(r: Option<f64>) -> Value {
    r.map_or(Value::Null, |r| json!((r * 1000.0).round() / 1000.0))
}

impl Report {
    fn skills_json(&self, counts: &SkillCounts) -> Value {
        let mut out = Map::new();
        for skill in SKILLS {
            let Some(c) = counts.get(&skill.as_char()) else {
                continue;
            };
            let (positive, negative) = self.sets.for_skill(skill);
            let evaluations: Map<String, Value> = c
                .by_evaluation
                .iter()
                .map(|(e, n)| (e.to_string(), json!(n)))
                .collect();
            out.insert(
                skill_name(skill).to_owned(),
                json!({
                    "total": c.total,
                    "evaluations": evaluations,
                    "positive": c.with(positive),
                    "negative": c.with(negative),
                    "positive_rate": rate(c.positive_rate(skill, self.sets)),
                    "efficiency": rate(c.efficiency(skill, self.sets)),
                }),
            );
        }
        Value::Object(out)
    }

    fn name(&self, team: Team, number: u8) -> Option<&str> {
        self.player_names
            .iter()
            .find(|(t, n, _)| *t == team && *n == number)
            .map(|(_, _, name)| name.as_str())
    }

    pub fn to_json(&self) -> Value {
        let definitions: Map<String, Value> = SKILLS
            .iter()
            .map(|s| {
                let (positive, negative) = self.sets.for_skill(*s);
                (
                    skill_name(*s).to_owned(),
                    json!({ "positive": positive, "negative": negative }),
                )
            })
            .collect();
        json!({
            "teams": { "home": self.names[0], "visiting": self.names[1] },
            "sets": self.stats.sets.iter().map(|s| json!({
                "set": s.set, "home": s.home, "visiting": s.visiting,
            })).collect::<Vec<_>>(),
            "rates": {
                "symbols": self.sets.name,
                "positive_rate": POSITIVE_RATE,
                "efficiency": EFFICIENCY,
                "skills": definitions,
            },
            "team_totals": {
                "home": self.skills_json(&self.stats.home),
                "visiting": self.skills_json(&self.stats.visiting),
            },
            "players": self.stats.players.iter().map(|p| json!({
                "team": side(p.team),
                "number": p.number,
                "name": self.name(p.team, p.number),
                "skills": self.skills_json(&p.skills),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let sets: Vec<String> = self
            .stats
            .sets
            .iter()
            .map(|s| format!("{}-{}", s.home, s.visiting))
            .collect();
        let _ = writeln!(
            out,
            "{} vs {}: {}",
            self.names[0],
            self.names[1],
            sets.join(", ")
        );
        let _ = writeln!(
            out,
            "Rates use {}: positive % = {POSITIVE_RATE}, efficiency = {EFFICIENCY}.",
            self.sets.name
        );
        for (name, counts) in [
            (&self.names[0], &self.stats.home),
            (&self.names[1], &self.stats.visiting),
        ] {
            let _ = writeln!(out, "\n{name}");
            let _ = writeln!(
                out,
                "  {:<10}{:>6}{:>5}{:>5}{:>5}{:>5}{:>5}{:>5}{:>8}{:>8}",
                "skill", "total", "#", "+", "!", "-", "/", "=", "pos %", "eff %"
            );
            for skill in SKILLS {
                let Some(c) = counts.get(&skill.as_char()) else {
                    continue;
                };
                let pct = |r: Option<f64>| r.map_or("-".into(), |r| format!("{:.0}", r * 100.0));
                let _ = writeln!(
                    out,
                    "  {:<10}{:>6}{:>5}{:>5}{:>5}{:>5}{:>5}{:>5}{:>8}{:>8}",
                    skill_name(skill),
                    c.total,
                    c.with("#"),
                    c.with("+"),
                    c.with("!"),
                    c.with("-"),
                    c.with("/"),
                    c.with("="),
                    pct(c.positive_rate(skill, self.sets)),
                    pct(c.efficiency(skill, self.sets)),
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &[u8] = b"[3DATAVOLLEYSCOUT]\nGENERATOR-PRG: Data Volley\n[3MATCH]\n03/04/2026;;;;;;;;1252;1;Z;\n[3TEAMS]\nT1;Home Club;0;;;;\nT2;Away Club;0;;;;\n[3PLAYERS-H]\n0;8;1;1;;;;;H8;Placeholder;Alex;;;2;;\n[3PLAYERS-V]\n1;16;1;1;;;;;V16;Example;Sam;;;2;;\n[3SCOUT]\n*08SH-;;;;;;;;;;;;;;;;;;;;;;;;;;\na16RH#;;;;;;;;;;;;;;;;;;;;;;;;;;\na16AH#;;;;;;;;;;;;;;;;;;;;;;;;;;\n*$$&H=;;;;;;;;;;;;;;;;;;;;;;;;;;\nap00:01;;;;;;;;;;;;;;;;;;;;;;;;;;\n";

    #[test]
    fn json_names_the_symbol_sets() {
        let dvw = Dvw::parse(FILE).expect("parse");
        let j = report(&dvw, Symbols::Datavolley).to_json();
        assert_eq!(j["sets"], json!([{ "set": 1, "home": 0, "visiting": 1 }]));
        assert_eq!(j["rates"]["symbols"], "DataVolley 4 defaults");
        assert_eq!(j["rates"]["efficiency"], EFFICIENCY);
        assert_eq!(j["rates"]["skills"]["serve"]["positive"], "#/");
        let attack = &j["players"][1]["skills"]["attack"];
        assert_eq!(j["players"][1]["name"], "Sam Example");
        assert_eq!(
            (attack["total"].clone(), attack["efficiency"].clone()),
            (json!(1), json!(1.0))
        );
        // The green code is no touch.
        assert!(j["team_totals"]["home"].get("green").is_none());
        let vs = report(&dvw, Symbols::Volleystation).to_json();
        assert_eq!(vs["rates"]["skills"]["serve"]["positive"], "#+/");
    }

    #[test]
    fn text_has_scores_and_formula() {
        let dvw = Dvw::parse(FILE).expect("parse");
        let text = report(&dvw, Symbols::Datavolley).to_text();
        assert!(text.starts_with("Home Club vs Away Club: 0-1\n"));
        assert!(text.contains("DataVolley 4 defaults"));
        assert!(text.contains("  attack         1    1"));
    }
}
