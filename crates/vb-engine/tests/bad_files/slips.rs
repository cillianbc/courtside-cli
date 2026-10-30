//! Plants one slip in a copy of a corpus match.
//!
//! Each slip class finds the places in the match where it can be planted
//! (its sites), picks one by seed or line, and edits the event log the way
//! a scout's mistake would have: the derived columns, `[3SET]`, `[3TEAMS]`
//! and the player tables are then written by the engine, as DataVolley
//! would have.
//! A match with no site for a class is an error, never a silent skip.

use dvw_codec::Dvw;
use serde::Deserialize;
use vb_engine::validate::Rule;
use vb_engine::{EventId, EventKind, Folded, IdGen, Match, MatchEvent, NewEvent, Producer, fold};
use vb_grammar::{Code, Player, Skill, Team};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlipClass {
    /// A point code left out: the next serve lands in the same rally.
    MissingPointCode,
    /// A point code one point too high for the winner.
    SkippedScore,
    /// The side-out team's rotation (`z`) code left out.
    MissedRotation,
    /// A point given to the wrong team; the rest of the set renumbered.
    WrongTeamPoint,
    /// Legal-looking substitutions added until the team has made seven.
    SeventhSubstitution,
    /// A player who was substituted off comes back for someone other than
    /// their replacement.
    IllegalReEntry,
    /// A set closed at 25:24 (15:14 in a deciding set).
    SetEndsWithoutMargin,
    /// An attack credited to the team's libero.
    LiberoAttack,
    /// A block credited to a back-row player.
    BackRowBlock,
    /// A blocked attack (`A/`) written `A-`, leaving the opponent's `B#`
    /// unpaired.
    BlockedAttackUnpaired,
}

impl SlipClass {
    pub fn name(self) -> &'static str {
        match self {
            SlipClass::MissingPointCode => "missing-point-code",
            SlipClass::SkippedScore => "skipped-score",
            SlipClass::MissedRotation => "missed-rotation",
            SlipClass::WrongTeamPoint => "wrong-team-point",
            SlipClass::SeventhSubstitution => "seventh-substitution",
            SlipClass::IllegalReEntry => "illegal-re-entry",
            SlipClass::SetEndsWithoutMargin => "set-ends-without-margin",
            SlipClass::LiberoAttack => "libero-attack",
            SlipClass::BackRowBlock => "back-row-block",
            SlipClass::BlockedAttackUnpaired => "blocked-attack-unpaired",
        }
    }

    /// The rule a validator that understands the slip should report it
    /// under. Only for the report: a hit doesn't need the right rule.
    pub fn expected_rule(self) -> Rule {
        match self {
            // A point against the touches is a rally-flow issue.
            SlipClass::MissingPointCode
            | SlipClass::WrongTeamPoint
            | SlipClass::BlockedAttackUnpaired => Rule::ServeReception,
            SlipClass::SkippedScore | SlipClass::SetEndsWithoutMargin => Rule::Score,
            SlipClass::MissedRotation => Rule::Rotation,
            SlipClass::SeventhSubstitution
            | SlipClass::IllegalReEntry
            | SlipClass::LiberoAttack => Rule::Substitution,
            SlipClass::BackRowBlock => Rule::BackRow,
        }
    }
}

/// A bad file and where its slip is.
pub struct Planted {
    pub dvw: Dvw,
    /// `[3SCOUT]` index of the slip in the bad file.
    pub slip: usize,
    /// For each `[3SCOUT]` line of the bad file, the source line it came
    /// from (`None` for inserted lines).
    pub source_line: Vec<Option<usize>>,
}

/// One place a slip can go: the `[3SCOUT]` index it is reported at in
/// the source (for `line` overrides) and the edit.
struct Site {
    index: usize,
    edit: Edit,
}

enum Edit {
    /// Replace events with new codes.
    Correct(Vec<(EventId, String)>),
    /// Remove events; the slip is at `slip`, the event after them.
    Remove { events: Vec<EventId>, slip: EventId },
    /// Insert codes before events; the slip is the insertion `slip_at`.
    Insert {
        codes: Vec<(EventId, String)>,
        slip_at: usize,
    },
}

pub fn plant(
    source: &Dvw,
    class: SlipClass,
    seed: u64,
    line: Option<usize>,
) -> Result<Planted, String> {
    let mut m = Match::from_dvw(source, &mut IdGen::new(0));
    let sites = {
        let events = m.log.visible();
        let folded = fold(&m);
        let cx = Cx {
            m: &m,
            ev: &events,
            f: &folded,
        };
        match class {
            SlipClass::MissingPointCode => cx.missing_point_code(),
            SlipClass::SkippedScore => cx.skipped_score(),
            SlipClass::MissedRotation => cx.missed_rotation(),
            SlipClass::WrongTeamPoint => cx.wrong_team_point(),
            SlipClass::SeventhSubstitution => cx.seventh_substitution(),
            SlipClass::IllegalReEntry => cx.illegal_re_entry(),
            SlipClass::SetEndsWithoutMargin => cx.set_ends_without_margin(),
            SlipClass::LiberoAttack => cx.libero_attack(),
            SlipClass::BackRowBlock => cx.back_row_block(),
            SlipClass::BlockedAttackUnpaired => cx.blocked_attack_unpaired(),
        }
    };
    if sites.is_empty() {
        return Err(format!("no site for {}", class.name()));
    }
    let site = match line {
        Some(l) => sites
            .into_iter()
            .find(|s| s.index == l)
            .ok_or_else(|| format!("no {} site at [3SCOUT] index {l}", class.name()))?,
        None => {
            let n = sites.len() as u64;
            let i = (seed + 1).wrapping_mul(2_654_435_761) % n;
            sites
                .into_iter()
                .nth(usize::try_from(i).map_err(|e| e.to_string())?)
                .ok_or("site index")?
        }
    };

    let mut ids = IdGen::new(1);
    let new = |code: &str| NewEvent::code(Code::parse(code), Producer::Keyboard);
    let slip_id = match site.edit {
        Edit::Correct(changes) => {
            let mut first = None;
            for (id, code) in changes {
                let c = m
                    .log
                    .correct(&mut ids, id, new(&code))
                    .map_err(|e| e.to_string())?;
                first.get_or_insert(c);
            }
            first.ok_or("empty correction")?
        }
        Edit::Remove { events, slip } => {
            for id in events {
                m.log
                    .remove(&mut ids, id, Producer::Keyboard)
                    .map_err(|e| e.to_string())?;
            }
            slip
        }
        Edit::Insert { codes, slip_at } => {
            let mut slip = None;
            for (i, (before, code)) in codes.into_iter().enumerate() {
                let target = m.log.get(before).ok_or("insertion anchor")?;
                let event = NewEvent {
                    clock: target.clock,
                    video_ms: target.video_ms,
                    ..new(&code)
                };
                let id = m
                    .log
                    .insert_before(&mut ids, before, event)
                    .map_err(|e| e.to_string())?;
                if i == slip_at {
                    slip = Some(id);
                }
            }
            slip.ok_or("slip insertion")?
        }
    };

    // The engine marks inserted substitutes `*` in the player table.
    let dvw = m.to_dvw(source);
    let lines: Vec<&MatchEvent> = m
        .log
        .visible()
        .into_iter()
        .filter(|e| e.code().is_some())
        .collect();
    let slip = lines
        .iter()
        .position(|e| e.id == slip_id)
        .ok_or("slip not in the bad file")?;
    Ok(Planted {
        source_line: lines.iter().map(|e| e.source_line).collect(),
        dvw,
        slip,
    })
}

struct Cx<'a> {
    m: &'a Match,
    ev: &'a [&'a MatchEvent],
    f: &'a Folded,
}

fn other(t: Team) -> Team {
    match t {
        Team::Home => Team::Visiting,
        Team::Visiting => Team::Home,
    }
}

/// `code` with the player number replaced.
fn with_player(code: &Code, number: u8) -> Option<String> {
    let Code::Touch(t) = code else { return None };
    let text = code.to_string();
    let rest = text.get(1..)?;
    let rest = match &t.player {
        Player::Number(d) => rest.get(d.as_str().len()..)?,
        Player::Unknown => rest.get(2..)?,
    };
    Some(format!("{}{number:02}{rest}", t.team.as_char()))
}

fn point(team: Team, home: u8, visiting: u8) -> String {
    format!("{}p{home:02}:{visiting:02}", team.as_char())
}

fn number(player: &Player) -> Option<u8> {
    match player {
        Player::Number(d) => d.value().and_then(|v| u8::try_from(v).ok()),
        Player::Unknown => None,
    }
}

impl Cx<'_> {
    fn kind(&self, i: usize) -> EventKind {
        self.ev[i].kind()
    }

    fn rally_set(&self, i: usize) -> Option<u8> {
        self.f.states[i].rally.map(|r| self.f.rallies[r].set)
    }

    fn points(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.ev.len()).filter(|&i| self.kind(i) == EventKind::Point)
    }

    /// The point is followed by another in the same set.
    fn set_continues(&self, i: usize) -> bool {
        let set = self.rally_set(i);
        (i + 1..self.ev.len())
            .find(|&j| self.kind(j) == EventKind::Point)
            .is_some_and(|j| self.rally_set(j) == set)
    }

    fn touch(&self, i: usize) -> Option<&vb_grammar::Touch> {
        match self.ev[i].code()? {
            Code::Touch(t) => Some(t),
            _ => None,
        }
    }

    /// First serve of rally `r`.
    fn serve(&self, r: usize) -> Option<usize> {
        let rally = &self.f.rallies[r];
        (rally.first..=rally.last).find(|&i| self.touch(i).is_some_and(|t| t.skill == Skill::Serve))
    }

    fn court_before(&self, i: usize, team: Team) -> [Option<u8>; 6] {
        *self.f.states[i.saturating_sub(1)].court.get(team)
    }

    fn missing_point_code(&self) -> Vec<Site> {
        self.points()
            .filter(|&i| self.set_continues(i) && i + 1 < self.ev.len())
            .map(|i| Site {
                index: i,
                edit: Edit::Remove {
                    events: vec![self.ev[i].id],
                    slip: self.ev[i + 1].id,
                },
            })
            .collect()
    }

    fn skipped_score(&self) -> Vec<Site> {
        self.points()
            .filter(|&i| self.set_continues(i))
            .filter_map(|i| {
                let Some(Code::Point { team, .. }) = self.ev[i].code() else {
                    return None;
                };
                let s = self.f.states[i].score;
                let (h, v) = match team {
                    Team::Home => (s.home + 1, s.visiting),
                    Team::Visiting => (s.home, s.visiting + 1),
                };
                Some(Site {
                    index: i,
                    edit: Edit::Correct(vec![(self.ev[i].id, point(*team, h, v))]),
                })
            })
            .collect()
    }

    fn missed_rotation(&self) -> Vec<Site> {
        let mut sites = Vec::new();
        for p in self.points() {
            let Some(r) = self.f.states[p].rally else {
                continue;
            };
            let rally = &self.f.rallies[r];
            let (Some(won), Some(served)) = (rally.won_by, rally.serving) else {
                continue;
            };
            if won == served {
                continue;
            }
            // The side-out step: z codes straight after the point.
            let zs: Vec<usize> = (p + 1..self.ev.len())
                .take_while(|&j| self.kind(j) == EventKind::SetterPosition)
                .filter(|&j| self.ev[j].code().and_then(Code::team) == Some(won))
                .collect();
            let end = (p + 1..self.ev.len()).find(|&j| self.kind(j) != EventKind::SetterPosition);
            if let (Some(&z), Some(next)) = (zs.first(), end) {
                sites.push(Site {
                    index: z,
                    edit: Edit::Remove {
                        events: zs.iter().map(|&j| self.ev[j].id).collect(),
                        slip: self.ev[next].id,
                    },
                });
            }
        }
        sites
    }

    fn wrong_team_point(&self) -> Vec<Site> {
        self.points()
            .filter(|&i| self.set_continues(i))
            .filter_map(|i| {
                let r = self.f.states[i].rally?;
                let set = self.f.rallies[r].set;
                let mut score = self.f.rallies[r].score_before;
                let mut changes = Vec::new();
                for j in (i..self.ev.len()).filter(|&j| self.kind(j) == EventKind::Point) {
                    if self.rally_set(j) != Some(set) {
                        break;
                    }
                    let team = self.ev[j].code().and_then(Code::team)?;
                    let team = if j == i { other(team) } else { team };
                    match team {
                        Team::Home => score.home += 1,
                        Team::Visiting => score.visiting += 1,
                    }
                    changes.push((self.ev[j].id, point(team, score.home, score.visiting)));
                }
                Some(Site {
                    index: i,
                    edit: Edit::Correct(changes),
                })
            })
            .collect()
    }

    /// Substitution events by `team` in `set`.
    fn subs(&self, team: Team, set: u8) -> Vec<usize> {
        (0..self.ev.len())
            .filter(|&i| {
                self.kind(i) == EventKind::Substitution
                    && self.f.states[i].set == set
                    && self.ev[i].code().and_then(Code::team) == Some(team)
            })
            .collect()
    }

    /// Players `team` touches the ball with in rallies `rallies`.
    fn touches_in(&self, team: Team, rallies: std::ops::Range<usize>) -> Vec<u8> {
        rallies
            .flat_map(|r| {
                let rally = &self.f.rallies[r];
                rally.first..=rally.last
            })
            .filter_map(|i| self.touch(i))
            .filter(|t| t.team == team)
            .filter_map(|t| number(&t.player))
            .collect()
    }

    fn rallies_in(&self, set: u8) -> Vec<usize> {
        (0..self.f.rallies.len())
            .filter(|&r| self.f.rallies[r].set == set)
            .collect()
    }

    fn seventh_substitution(&self) -> Vec<Site> {
        let mut sites = Vec::new();
        let sets = self.f.rallies.iter().map(|r| r.set).max().unwrap_or(0);
        for set in 1..=sets {
            for team in [Team::Home, Team::Visiting] {
                let subs = self.subs(team, set);
                // Pairs of out-and-back substitutions keep the court as it
                // was, so the 7th is a legal-looking return.
                if subs.len() % 2 == 0 || subs.len() > 5 {
                    continue;
                }
                let pairs = (7 - subs.len()) / 2;
                let rallies = self.rallies_in(set);
                let after = subs.last().copied().unwrap_or(0);
                let Some(start) = rallies
                    .iter()
                    .position(|&r| self.f.rallies[r].first > after)
                else {
                    continue;
                };
                let window = &rallies[start..];
                if window.len() < pairs * 2 {
                    continue;
                }
                let sheet = self.m.teams.get(team);
                let first_serve = self.serve(window[0]);
                let Some(first_serve) = first_serve else {
                    continue;
                };
                let court = self.court_before(first_serve, team);
                let setter = *self.f.states[first_serve].setter.get(team);
                let mut used_subs: Vec<u8> = Vec::new();
                for &i in &subs {
                    if let Some(Code::Substitution { out, r#in, .. }) = self.ev[i].code() {
                        used_subs.extend(out.value().and_then(|v| u8::try_from(v).ok()));
                        used_subs.extend(r#in.value().and_then(|v| u8::try_from(v).ok()));
                    }
                }
                let mut bench = sheet.players.iter().map(|p| p.number).filter(|&n| {
                    !sheet.is_libero(n) && !sheet.played_in(n, set) && !court.contains(&Some(n))
                });
                let mut codes = Vec::new();
                let mut starters = court.iter().flatten().copied().filter(|&n| {
                    Some(n) != setter && !sheet.is_libero(n) && !used_subs.contains(&n)
                });
                let mut ok = true;
                for p in 0..pairs {
                    let off = window[2 * p];
                    let back = window[2 * p + 1];
                    let touched = self.touches_in(team, off..off + 1);
                    let (Some(x), Some(y)) =
                        (starters.find(|n| !touched.contains(n)), bench.next())
                    else {
                        ok = false;
                        break;
                    };
                    let (Some(s1), Some(s2)) = (self.serve(off), self.serve(back)) else {
                        ok = false;
                        break;
                    };
                    let c = team.as_char();
                    codes.push((self.ev[s1].id, format!("{c}c{x:02}:{y:02}")));
                    codes.push((self.ev[s2].id, format!("{c}c{y:02}:{x:02}")));
                }
                if ok {
                    sites.push(Site {
                        index: subs.last().copied().unwrap_or(0),
                        edit: Edit::Insert {
                            // The team's seventh substitution in the set.
                            slip_at: 6 - subs.len(),
                            codes,
                        },
                    });
                }
            }
        }
        sites
    }

    fn illegal_re_entry(&self) -> Vec<Site> {
        let mut sites = Vec::new();
        for i in 0..self.ev.len() {
            let Some(Code::Substitution {
                team, out, r#in, ..
            }) = self.ev[i].code()
            else {
                continue;
            };
            let team = *team;
            let set = self.f.states[i].set;
            let (Some(a), Some(b)) = (
                out.value().and_then(|v| u8::try_from(v).ok()),
                r#in.value().and_then(|v| u8::try_from(v).ok()),
            ) else {
                continue;
            };
            if self.subs(team, set).len() > 5 {
                continue;
            }
            let sheet = self.m.teams.get(team);
            let later_subs: Vec<u8> = self
                .subs(team, set)
                .into_iter()
                .filter(|&j| j > i)
                .filter_map(|j| match self.ev[j].code() {
                    Some(Code::Substitution { out, r#in, .. }) => Some([out, r#in]),
                    _ => None,
                })
                .flatten()
                .filter_map(|d| d.value().and_then(|v| u8::try_from(v).ok()))
                .collect();
            if later_subs.contains(&a) {
                continue;
            }
            let rallies = self.rallies_in(set);
            let last = rallies.last().copied().unwrap_or(0);
            let site = rallies
                .iter()
                .filter(|&&r| self.f.rallies[r].first > i)
                .find_map(|&r| {
                    let serve = self.serve(r)?;
                    let court = self.court_before(serve, team);
                    if court.contains(&Some(a)) || !court.contains(&Some(b)) {
                        return None;
                    }
                    let setter = *self.f.states[serve].setter.get(team);
                    let touched = self.touches_in(team, r..last + 1);
                    let c = court.iter().flatten().copied().find(|&n| {
                        n != b
                            && Some(n) != setter
                            && !sheet.is_libero(n)
                            && !touched.contains(&n)
                            && !later_subs.contains(&n)
                    })?;
                    Some(Site {
                        index: i,
                        edit: Edit::Insert {
                            codes: vec![(
                                self.ev[serve].id,
                                format!("{}c{c:02}:{a:02}", team.as_char()),
                            )],
                            slip_at: 0,
                        },
                    })
                });
            sites.extend(site);
        }
        sites
    }

    fn set_ends_without_margin(&self) -> Vec<Site> {
        self.points()
            .filter(|&i| self.set_continues(i))
            .filter_map(|i| {
                let set = self.rally_set(i)?;
                let target = self.m.regulation.set_target(set);
                let s = self.f.states[i].score;
                let (hi, lo) = (s.home.max(s.visiting), s.home.min(s.visiting));
                if hi != target || lo != target - 1 {
                    return None;
                }
                let end = (i + 1..self.ev.len()).find(|&j| self.kind(j) == EventKind::SetEnd)?;
                Some(Site {
                    index: i,
                    edit: Edit::Remove {
                        events: (i + 1..end).map(|j| self.ev[j].id).collect(),
                        slip: self.ev[end].id,
                    },
                })
            })
            .collect()
    }

    fn libero_attack(&self) -> Vec<Site> {
        (0..self.ev.len())
            .filter_map(|i| {
                let t = self.touch(i).filter(|t| t.skill == Skill::Attack)?;
                let sheet = self.m.teams.get(t.team);
                let libero = sheet.players.iter().find(|p| p.libero)?.number;
                if number(&t.player) == Some(libero) {
                    return None;
                }
                let code = with_player(self.ev[i].code()?, libero)?;
                Some(Site {
                    index: i,
                    edit: Edit::Correct(vec![(self.ev[i].id, code)]),
                })
            })
            .collect()
    }

    fn back_row_block(&self) -> Vec<Site> {
        (0..self.ev.len())
            .filter_map(|i| {
                let t = self.touch(i).filter(|t| t.skill == Skill::Block)?;
                let sheet = self.m.teams.get(t.team);
                let court = self.court_before(i, t.team);
                // Zones 6, 5, 1.
                let player = [5, 4, 0].iter().find_map(|&z| {
                    court[z].filter(|&n| !sheet.is_libero(n) && Some(n) != number(&t.player))
                })?;
                let code = with_player(self.ev[i].code()?, player)?;
                Some(Site {
                    index: i,
                    edit: Edit::Correct(vec![(self.ev[i].id, code)]),
                })
            })
            .collect()
    }

    fn blocked_attack_unpaired(&self) -> Vec<Site> {
        (0..self.ev.len())
            .filter_map(|i| {
                let t = self
                    .touch(i)
                    .filter(|t| t.skill == Skill::Attack && t.evaluation() == Some('/'))?;
                let b = self.touch(i + 1)?;
                if b.team == t.team || b.skill != Skill::Block || b.evaluation() != Some('#') {
                    return None;
                }
                let code = Code::Touch(t.clone().with_evaluation('-')).to_string();
                Some(Site {
                    index: i,
                    edit: Edit::Correct(vec![(self.ev[i].id, code)]),
                })
            })
            .collect()
    }
}
