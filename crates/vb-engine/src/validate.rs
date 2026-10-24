//! The six validation rules (PRD domain model), run over the visible events
//! and the fold. Nothing here changes the match; it only reports.

use std::collections::{HashMap, HashSet};

use vb_grammar::{Code, Player, Skill, Team};

use crate::event::{EventId, EventKind, MatchEvent};
use crate::fold::{Folded, Score, State, fold_events};
use crate::model::{Match, Outcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Worth knowing, not a mistake (e.g. a rotation the scout corrected).
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// 1. Every point advances one side by one; sets end at a won score.
    Score,
    /// 2. Side-outs rotate, the rally winner serves, players are on court.
    Rotation,
    /// 3. Back-row players don't attack from the front zone or block.
    BackRow,
    /// 4. Substitutions: on and off court, six per set, re-entry; liberos
    ///    don't attack, block or (DataVolley) serve.
    Substitution,
    /// 5. A serve is followed by the other team's reception, or ends the
    ///    rally; the point goes to the team the touches say won it.
    ServeReception,
    /// 6. Unknown codes, players not on the team sheet, bad zones.
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub rule: Rule,
    pub event: EventId,
    /// Index into the visible events.
    pub index: usize,
    pub message: String,
    /// What the issue is, for code; the message is for people.
    pub kind: IssueKind,
    /// For a knock-on: the event (index into the visible events) whose
    /// issue this one follows from. Knock-ons are downgraded to Info, so
    /// one slip is reported once (CONTEXT.md "Knock-on").
    pub cause: Option<usize>,
}

/// What an issue is about, with the team it concerns where the check
/// knows it. One per kind of message the validator writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueKind {
    // Rule::Code
    UnrecognisedCode,
    NotOnTeamSheet(Team),
    UnknownEvaluation,
    UnknownSkillType,
    ZoneOutOfRange,
    CustomTextTooLong,
    SetterPositionOutOfRange,
    UnreadableScore,
    WrongSetEnd,
    // Rule::Score
    PointAfterSetWon,
    WrongScore,
    SetEndNotWon,
    /// Much more video than usual between a point and the next serve.
    LongGapBeforeServe,
    // Rule::Rotation
    /// The team that served out of turn.
    WrongServer(Team),
    SetterCorrected(Team),
    MissedRotation(Team),
    SetterMoved(Team),
    SetterSetAway(Team),
    NotOnCourt(Team),
    ServerOutOfZone(Team),
    // Rule::BackRow
    BackRowBlock,
    BackRowAttack,
    // Rule::Substitution
    LiberoOnCourt(Team),
    LiberoPlayed,
    UnreadableSubstitution,
    SubstitutedBySelf,
    LiberoSubstitution,
    OffNotOnCourt,
    OnAlreadyOnCourt,
    TooManySubstitutions(Team),
    ReEntry,
    WrongPartner,
    // Rule::ServeReception
    NoPointCode,
    SecondServe,
    ServeWithoutReception,
    ReceptionWithoutServe,
    /// A pass rated against its serve (`S+` then `R+`).
    PassDoesNotFitServe,
    Overpass,
    BlockedWithoutBlockPoint,
    BlockPointWithoutBlockedAttack,
    RallyContinues,
    PointToWrongTeam,
}

impl IssueKind {
    /// The team the issue is about, when the check knows it.
    pub fn team(self) -> Option<Team> {
        use IssueKind::*;
        match self {
            NotOnTeamSheet(t)
            | WrongServer(t)
            | SetterCorrected(t)
            | MissedRotation(t)
            | SetterMoved(t)
            | SetterSetAway(t)
            | NotOnCourt(t)
            | ServerOutOfZone(t)
            | LiberoOnCourt(t)
            | TooManySubstitutions(t) => Some(t),
            _ => None,
        }
    }

    /// A setter-zone issue: the scout set the zone by hand.
    pub fn is_setter_zone(self) -> bool {
        matches!(
            self,
            IssueKind::SetterCorrected(_) | IssueKind::SetterMoved(_) | IssueKind::SetterSetAway(_)
        )
    }
}

fn side(team: Team) -> &'static str {
    match team {
        Team::Home => "home",
        Team::Visiting => "visiting",
    }
}

fn other(team: Team) -> Team {
    match team {
        Team::Home => Team::Visiting,
        Team::Visiting => Team::Home,
    }
}

fn num(d: &vb_grammar::code::Digits) -> Option<u8> {
    d.value().and_then(|v| u8::try_from(v).ok())
}

fn touch_player(code: &Code) -> Option<(Team, u8)> {
    match code {
        Code::Touch(t) if t.skill != Skill::Green => match &t.player {
            Player::Number(d) => num(d).map(|n| (t.team, n)),
            Player::Unknown => None,
        },
        _ => None,
    }
}

/// Zone 1–6 of `n` on `team`'s court, if on court.
fn zone_of(st: &State, team: Team, n: u8) -> Option<usize> {
    st.court
        .get(team)
        .iter()
        .position(|p| *p == Some(n))
        .map(|i| i + 1)
}

/// The setter position one rotation on from `p` (1 → 6 → 5 …).
/// Whether a pass rated `r` can follow a serve rated `s`: DataVolley's and
/// VolleyStation's default tables agree (`S#`–`R=`, `S/`–`R/`, `S-`–`R#`/`R+`,
/// `S+`–`R-`, `S!`–`R!`), and many DataVolley 4 files in the corpus pair
/// `S+`–`R!` and `S!`–`R+` instead (FORMAT-NOTES), so either goes. A rating
/// outside the usual symbols isn't judged here.
fn pass_fits_serve(s: char, r: char) -> bool {
    let fits: &[char] = match s {
        '#' => &['='],
        '/' => &['/'],
        '-' => &['#', '+'],
        '+' => &['-', '!'],
        '!' => &['!', '+'],
        _ => return true,
    };
    fits.contains(&r) || !"#/-+!=".contains(r)
}

fn rotated(p: u8) -> u8 {
    if p == 1 { 6 } else { p - 1 }
}

struct Validator<'a> {
    m: &'a Match,
    events: &'a [&'a MatchEvent],
    f: &'a Folded,
    issues: Vec<Issue>,
    /// "Won a side-out but didn't rotate" errors: (event, team, position
    /// in `issues`).
    missed_rotations: Vec<(usize, Team, usize)>,
}

impl Validator<'_> {
    fn push(&mut self, i: usize, severity: Severity, rule: Rule, kind: IssueKind, message: String) {
        self.issues.push(Issue {
            severity,
            rule,
            event: self.events[i].id,
            index: i,
            message,
            kind,
            cause: None,
        });
    }

    /// Whether event `i` is the first of a set (its lineup loads there).
    fn starts_set(&self, i: usize) -> bool {
        self.events[..i]
            .iter()
            .rev()
            .find(|e| e.kind() != EventKind::Comment)
            .is_none_or(|e| e.kind() == EventKind::SetEnd)
    }

    /// Court before event `i`.
    fn court_before(&self, i: usize, team: Team) -> [Option<u8>; 6] {
        if self.starts_set(i) {
            let set = self.f.states[i].set;
            match team {
                Team::Home => self.m.teams.home.starting_lineup(set),
                Team::Visiting => self.m.teams.visiting.starting_lineup(set),
            }
        } else {
            *self.f.states[i - 1].court.get(team)
        }
    }

    fn score_before(&self, i: usize) -> Score {
        if self.starts_set(i) {
            Score::default()
        } else {
            self.f.states[i - 1].score
        }
    }

    fn won(&self, set: u8, s: Score) -> bool {
        let target = self.f.set_target(self.m, set);
        let (hi, lo) = (s.home.max(s.visiting), s.home.min(s.visiting));
        hi >= target && hi - lo >= 2 && (hi == target || hi - lo == 2)
    }

    fn codes(&mut self) {
        for i in 0..self.events.len() {
            let Some(code) = self.events[i].code() else {
                continue;
            };
            let mut found: Vec<(Severity, IssueKind, String)> = Vec::new();
            match code {
                Code::Unrecognised(s) if !s.is_empty() => {
                    found.push((
                        Severity::Warning,
                        IssueKind::UnrecognisedCode,
                        format!("unrecognised code {s:?}"),
                    ));
                }
                Code::Touch(t) => {
                    if let Some((team, n)) = touch_player(code)
                        && self.m.teams.get(team).player(n).is_none()
                    {
                        found.push((
                            Severity::Error,
                            IssueKind::NotOnTeamSheet(team),
                            format!("player {n} is not on the {} team sheet", side(team)),
                        ));
                    }
                    if t.skill != Skill::Green {
                        if let Some(e) = t.evaluation().filter(|e| !"#+!-/=".contains(*e)) {
                            found.push((
                                Severity::Error,
                                IssueKind::UnknownEvaluation,
                                format!("unknown evaluation {e:?}"),
                            ));
                        }
                        if let Some(k) = t.skill_type().filter(|k| !"HMQTUNO".contains(*k)) {
                            found.push((
                                Severity::Warning,
                                IssueKind::UnknownSkillType,
                                format!("unknown skill type {k:?}"),
                            ));
                        }
                    }
                    for (what, z) in [("start zone", t.start_zone()), ("end zone", t.end_zone())] {
                        if let Some(z) = z.filter(|z| !('1'..='9').contains(z)) {
                            found.push((
                                Severity::Error,
                                IssueKind::ZoneOutOfRange,
                                format!("{what} {z:?} is not 1–9"),
                            ));
                        }
                    }
                    if let Some(z) = t.end_subzone().filter(|z| !('A'..='D').contains(z)) {
                        found.push((
                            Severity::Error,
                            IssueKind::ZoneOutOfRange,
                            format!("end subzone {z:?} is not A–D"),
                        ));
                    }
                    // DataVolley stores codes in 20 characters, leaving 5
                    // for custom text, and cuts anything longer on load.
                    if t.custom().chars().count() > 5 {
                        found.push((
                            Severity::Warning,
                            IssueKind::CustomTextTooLong,
                            format!(
                                "custom text {:?} is over 5 characters; DataVolley cuts it to {:?}",
                                t.custom(),
                                t.custom().chars().take(5).collect::<String>()
                            ),
                        ));
                    }
                }
                Code::SetterPosition { position, .. }
                    if !num(position).is_some_and(|p| (1..=6).contains(&p)) =>
                {
                    found.push((
                        Severity::Error,
                        IssueKind::SetterPositionOutOfRange,
                        format!("setter position {position} is not 1–6"),
                    ));
                }
                Code::Point { home, visiting, .. }
                    if num(home).is_none() || num(visiting).is_none() =>
                {
                    found.push((
                        Severity::Error,
                        IssueKind::UnreadableScore,
                        "point code without a readable score".into(),
                    ));
                }
                Code::SetEnd { set, .. } => {
                    let ending = self.f.states[i].set.saturating_sub(1);
                    if num(set) != Some(ending) {
                        found.push((
                            Severity::Error,
                            IssueKind::WrongSetEnd,
                            format!("set end {set} where set {ending} is being played"),
                        ));
                    }
                }
                _ => {}
            }
            for (severity, kind, message) in found {
                self.push(i, severity, Rule::Code, kind, message);
            }
        }
    }

    /// A rally the scout missed leaves no code when the serving team won
    /// it: no rotation, no change of server, only the score one short.
    /// With video times on the codes it shows as a long stretch of video
    /// between a point and the next serve. Timeouts, substitutions,
    /// comments and set ends explain a long stretch, so they clear it.
    fn gaps(&mut self) {
        // Fewer gaps than this and the usual one isn't known.
        const MIN_GAPS: usize = 8;
        const MIN_FLAG_MS: u64 = 30_000;
        let mut gaps = Vec::new();
        let mut since_point: Option<u64> = None;
        for (i, e) in self.events.iter().enumerate() {
            match e.kind() {
                EventKind::Point => since_point = e.video_ms,
                EventKind::Touch => {
                    let serve = matches!(e.code(), Some(Code::Touch(t)) if t.skill == Skill::Serve);
                    if serve {
                        if let (Some(p), Some(v)) = (since_point, e.video_ms)
                            && v > p
                        {
                            gaps.push((i, v - p));
                        }
                        since_point = None;
                    }
                }
                EventKind::SetterPosition | EventKind::SetterChange => {}
                _ => since_point = None,
            }
        }
        if gaps.len() < MIN_GAPS {
            return;
        }
        let mut sorted: Vec<u64> = gaps.iter().map(|g| g.1).collect();
        sorted.sort_unstable();
        let usual = sorted[sorted.len() / 2];
        let flag = (usual * 8 / 5).max(MIN_FLAG_MS);
        for (i, gap) in gaps {
            if gap >= flag {
                self.push(
                    i,
                    Severity::Info,
                    Rule::Score,
                    IssueKind::LongGapBeforeServe,
                    format!(
                        "{} s of video since the last point, usually {} s; a missed rally?",
                        gap / 1000,
                        usual / 1000
                    ),
                );
            }
        }
    }

    fn score(&mut self) {
        for i in 0..self.events.len() {
            let st = &self.f.states[i];
            match self.events[i].code() {
                Some(Code::Point { team, .. }) => {
                    let before = self.score_before(i);
                    if self.won(st.set, before) {
                        self.push(
                            i,
                            Severity::Error,
                            Rule::Score,
                            IssueKind::PointAfterSetWon,
                            format!(
                                "point after the set was won at {}:{}; missing set end?",
                                before.home, before.visiting
                            ),
                        );
                        continue;
                    }
                    let mut expected = before;
                    match team {
                        Team::Home => expected.home += 1,
                        Team::Visiting => expected.visiting += 1,
                    }
                    if st.score != expected {
                        self.push(
                            i,
                            Severity::Error,
                            Rule::Score,
                            IssueKind::WrongScore,
                            format!(
                                "point to {}: score {}:{} → {}:{}, expected {}:{}",
                                side(*team),
                                before.home,
                                before.visiting,
                                st.score.home,
                                st.score.visiting,
                                expected.home,
                                expected.visiting
                            ),
                        );
                    }
                }
                Some(Code::SetEnd { .. }) => {
                    let set = st.set.saturating_sub(1);
                    if !self.won(set, st.score) {
                        self.push(
                            i,
                            Severity::Error,
                            Rule::Score,
                            IssueKind::SetEndNotWon,
                            format!(
                                "set {set} ends at {}:{}, which doesn't win it",
                                st.score.home, st.score.visiting
                            ),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn first_serve(&self, r: usize) -> Option<usize> {
        let rally = &self.f.rallies[r];
        (rally.first..=rally.last).find(
            |&i| matches!(self.events[i].code(), Some(Code::Touch(t)) if t.skill == Skill::Serve),
        )
    }

    fn rotation(&mut self) {
        let rallies = self.f.rallies.clone();
        for (k, pair) in rallies.windows(2).enumerate() {
            let (r, next) = (&pair[0], &pair[1]);
            let (Some(winner), Some(server)) = (r.won_by, r.serving) else {
                continue;
            };
            if next.set != r.set {
                continue;
            }
            let at = self.first_serve(k + 1);
            if let Some(s) = at
                && next.serving != Some(winner)
            {
                self.push(
                    s,
                    Severity::Error,
                    Rule::Rotation,
                    IssueKind::WrongServer(other(winner)),
                    format!(
                        "{} serves but {} won the last rally",
                        side(other(winner)),
                        side(winner)
                    ),
                );
            }
            let check_at = at.unwrap_or(next.last);
            let end = &self.f.states[r.last];
            let then = &self.f.states[check_at];
            // The `z` codes straight after the point code are the side-out
            // step. Both DataVolley (renumbering after a setter change, or
            // the scout re-entering `z`) and VolleyStation (a `z` for every
            // setter-zone change) also write `z` codes elsewhere; those are
            // corrections, not rotation mistakes.
            let step_end = (r.last + 1..check_at)
                .take_while(|&i| {
                    matches!(
                        self.events[i].kind(),
                        EventKind::SetterPosition | EventKind::Comment
                    )
                })
                .last();
            for team in [Team::Home, Team::Visiting] {
                let (Some(p), Some(q)) = (
                    *end.setter_position.get(team),
                    *then.setter_position.get(team),
                ) else {
                    continue;
                };
                let rotates = team == winner && winner != server;
                let expected = if rotates { rotated(p) } else { p };
                if q == expected {
                    continue;
                }
                let stepped = step_end.and_then(|i| {
                    let z_for_team = (r.last + 1..=i).any(|j| {
                        matches!(self.events[j].code(), Some(Code::SetterPosition { team: t, .. }) if *t == team)
                    });
                    z_for_team.then(|| *self.f.states[i].setter_position.get(team))
                });
                let (severity, kind, message) = if stepped == Some(Some(expected)) {
                    (
                        Severity::Info,
                        IssueKind::SetterCorrected(team),
                        format!(
                            "{} setter position corrected from zone {expected} to {q} after the side-out step",
                            side(team)
                        ),
                    )
                } else if rotates && q == p {
                    self.missed_rotations
                        .push((check_at, team, self.issues.len()));
                    (
                        Severity::Error,
                        IssueKind::MissedRotation(team),
                        format!("{} won a side-out but didn't rotate", side(team)),
                    )
                } else if stepped.is_some() {
                    (
                        Severity::Warning,
                        IssueKind::SetterMoved(team),
                        format!(
                            "{} setter moved from zone {p} to {q}; expected {expected}",
                            side(team)
                        ),
                    )
                } else {
                    (
                        Severity::Info,
                        IssueKind::SetterSetAway(team),
                        format!(
                            "{} setter position set from zone {p} to {q} away from a side-out step (expected {expected})",
                            side(team)
                        ),
                    )
                };
                self.push(check_at, severity, Rule::Rotation, kind, message);
            }
        }
        // Who has been on court so far in each set, by team: a live match's
        // team sheet only gets its starting zones when it is exported.
        let mut been_on = std::collections::HashSet::new();
        for i in 0..self.events.len() {
            let st = &self.f.states[i];
            for team in [Team::Home, Team::Visiting] {
                for n in st.court.get(team).iter().flatten() {
                    been_on.insert((team, st.set, *n));
                }
            }
            let Some(code) = self.events[i].code() else {
                continue;
            };
            let Some((team, n)) = touch_player(code) else {
                continue;
            };
            let sheet = self.m.teams.get(team);
            // Liberos are never among the six on court (both products keep
            // them out). Players missing from the team sheet are a Code
            // error already.
            if sheet.is_libero(n)
                || sheet.player(n).is_none()
                || st.court.get(team).iter().all(Option::is_none)
            {
                continue;
            }
            let is_serve = matches!(code, Code::Touch(t) if t.skill == Skill::Serve);
            match zone_of(st, team, n) {
                None => self.push(
                    i,
                    Severity::Warning,
                    Rule::Rotation,
                    IssueKind::NotOnCourt(team),
                    if been_on.contains(&(team, st.set, n)) {
                        format!(
                            "{} player {n} went off earlier in set {}; missing substitution?",
                            side(team),
                            st.set
                        )
                    } else if sheet.played_in(n, st.set) {
                        format!(
                            "{} player {n} is not on court; missing substitution?",
                            side(team)
                        )
                    } else {
                        format!(
                            "{} player {n} hasn't been on court in set {}; wrong shirt number?",
                            side(team),
                            st.set
                        )
                    },
                ),
                Some(z) if is_serve && z != 1 => self.push(
                    i,
                    Severity::Warning,
                    Rule::Rotation,
                    IssueKind::ServerOutOfZone(team),
                    format!("{} server {n} is in zone {z}, not zone 1", side(team)),
                ),
                _ => {}
            }
        }
    }

    fn back_row(&mut self) {
        for i in 0..self.events.len() {
            let Some(Code::Touch(t)) = self.events[i].code() else {
                continue;
            };
            let Some((team, n)) = touch_player(&Code::Touch(t.clone())) else {
                continue;
            };
            let zone = zone_of(&self.f.states[i], team, n);
            let back = zone.is_some_and(|z| matches!(z, 1 | 5 | 6));
            match t.skill {
                Skill::Block if back => self.push(
                    i,
                    Severity::Error,
                    Rule::BackRow,
                    IssueKind::BackRowBlock,
                    format!("back-row player {n} (zone {}) blocked", zone.unwrap_or(0)),
                ),
                Skill::Attack if back && matches!(t.start_zone(), Some('2' | '3' | '4')) => self
                    .push(
                        i,
                        Severity::Warning,
                        Rule::BackRow,
                        IssueKind::BackRowAttack,
                        format!(
                            "back-row player {n} (zone {}) attacked from zone {}",
                            zone.unwrap_or(0),
                            t.start_zone().unwrap_or(' ')
                        ),
                    ),
                _ => {}
            }
        }
    }

    /// Liberos may not attack or block (both products). DataVolley also
    /// forbids a libero serve unless an option allows it; VolleyStation
    /// allows it, so it is only a warning, and none where the match's
    /// rules allow it.
    ///
    /// Neither product puts a libero among the six on court, so a libero
    /// who is in the court columns is a team-sheet mislabel: reported once
    /// per set instead of on every touch.
    fn liberos(&mut self) {
        let mut mislabelled: HashSet<(u8, Team, u8)> = HashSet::new();
        for i in 0..self.events.len() {
            let Some(code @ Code::Touch(t)) = self.events[i].code() else {
                continue;
            };
            let Some((team, n)) = touch_player(code) else {
                continue;
            };
            if !self.m.teams.get(team).is_libero(n) {
                continue;
            }
            let set = self.f.states[i].set;
            if zone_of(&self.f.states[i], team, n).is_some() {
                if mislabelled.insert((set, team, n)) {
                    self.push(
                        i,
                        Severity::Warning,
                        Rule::Substitution,
                        IssueKind::LiberoOnCourt(team),
                        format!(
                            "{} player {n} is a libero on the team sheet but is on court in set {set}; team sheet wrong?",
                            side(team)
                        ),
                    );
                }
                continue;
            }
            let (severity, what) = match t.skill {
                Skill::Attack => (Severity::Error, "attacked"),
                Skill::Block => (Severity::Error, "blocked"),
                Skill::Serve if self.m.rules.libero_may_serve => continue,
                Skill::Serve => (Severity::Warning, "served"),
                _ => continue,
            };
            self.push(
                i,
                severity,
                Rule::Substitution,
                IssueKind::LiberoPlayed,
                format!("libero {n} {what}"),
            );
        }
    }

    fn substitutions(&mut self) {
        #[derive(Default)]
        struct Book {
            count: u8,
            pairs: HashMap<u8, u8>,
            closed: HashSet<u8>,
        }
        let mut books: HashMap<(u8, Team), Book> = HashMap::new();
        for i in 0..self.events.len() {
            let Some(Code::Substitution {
                team, out, r#in, ..
            }) = self.events[i].code()
            else {
                continue;
            };
            let team = *team;
            let (Some(out), Some(inn)) = (num(out), num(r#in)) else {
                self.push(
                    i,
                    Severity::Error,
                    Rule::Substitution,
                    IssueKind::UnreadableSubstitution,
                    "substitution without readable shirt numbers".into(),
                );
                continue;
            };
            let sheet = self.m.teams.get(team);
            let court = self.court_before(i, team);
            let mut found: Vec<(Severity, IssueKind, String)> = Vec::new();
            if out == inn {
                found.push((
                    Severity::Error,
                    IssueKind::SubstitutedBySelf,
                    format!("player {out} replaced by themselves"),
                ));
            }
            if sheet.is_libero(out) || sheet.is_libero(inn) {
                found.push((
                    Severity::Warning,
                    IssueKind::LiberoSubstitution,
                    "libero replacements are not substitutions".into(),
                ));
            }
            if sheet.player(inn).is_none() {
                found.push((
                    Severity::Error,
                    IssueKind::NotOnTeamSheet(team),
                    format!("player {inn} is not on the {} team sheet", side(team)),
                ));
            }
            if !court.contains(&Some(out)) && court.iter().any(Option::is_some) {
                found.push((
                    Severity::Error,
                    IssueKind::OffNotOnCourt,
                    format!("player {out} is not on court"),
                ));
            }
            if court.contains(&Some(inn)) {
                found.push((
                    Severity::Error,
                    IssueKind::OnAlreadyOnCourt,
                    format!("player {inn} is already on court"),
                ));
            }
            let book = books.entry((self.f.states[i].set, team)).or_default();
            book.count = book.count.saturating_add(1);
            let limit = self.m.rules.substitutions;
            if book.count == limit.saturating_add(1) {
                found.push((
                    Severity::Error,
                    IssueKind::TooManySubstitutions(team),
                    format!(
                        "{} substitution {} in the set; the limit is {limit}",
                        side(team),
                        book.count
                    ),
                ));
            }
            if let Some(p) = [out, inn].into_iter().find(|p| book.closed.contains(p)) {
                found.push((
                    Severity::Error,
                    IssueKind::ReEntry,
                    format!("player {p} has already gone off and come back on this set"),
                ));
            } else if let Some(&partner) = book.pairs.get(&out) {
                if partner == inn {
                    book.closed.insert(out);
                    book.closed.insert(inn);
                } else {
                    found.push((
                        Severity::Error,
                        IssueKind::WrongPartner,
                        format!("player {out} can only be replaced by {partner}"),
                    ));
                }
            } else if let Some(&partner) = book.pairs.get(&inn) {
                found.push((
                    Severity::Error,
                    IssueKind::WrongPartner,
                    format!("player {inn} can only come on for {partner}"),
                ));
            } else {
                book.pairs.insert(out, inn);
                book.pairs.insert(inn, out);
            }
            for (severity, kind, message) in found {
                self.push(i, severity, Rule::Substitution, kind, message);
            }
        }
    }

    fn serve_reception(&mut self) {
        let last_rally = self.f.rallies.len().saturating_sub(1);
        for (k, r) in self.f.rallies.clone().iter().enumerate() {
            let touches: Vec<(usize, vb_grammar::Touch)> = (r.first..=r.last)
                .filter_map(|i| match self.events[i].code() {
                    Some(Code::Touch(t)) if t.skill != Skill::Green => Some((i, t.clone())),
                    _ => None,
                })
                .collect();
            if touches.is_empty() {
                continue;
            }
            if r.won_by.is_none() && k != last_rally {
                self.push(
                    r.last,
                    Severity::Error,
                    Rule::ServeReception,
                    IssueKind::NoPointCode,
                    "rally has no point code".into(),
                );
            }
            let mut serves = 0;
            for (j, (i, t)) in touches.iter().enumerate() {
                let next = touches.get(j + 1);
                let outcome = t
                    .evaluation()
                    .and_then(|e| self.m.winning_symbols.outcome(t.skill, e));
                match t.skill {
                    Skill::Serve => {
                        serves += 1;
                        if serves == 2 {
                            self.push(
                                *i,
                                Severity::Error,
                                Rule::ServeReception,
                                IssueKind::SecondServe,
                                "second serve in one rally; missing point code before it?".into(),
                            );
                        }
                        if outcome.is_none()
                            && let Some((ni, n)) = next
                            && !(n.skill == Skill::Reception && n.team != t.team)
                        {
                            self.push(
                                *ni,
                                Severity::Warning,
                                Rule::ServeReception,
                                IssueKind::ServeWithoutReception,
                                "a serve should be followed by the other team's reception".into(),
                            );
                        }
                    }
                    Skill::Reception => {
                        let prev = j.checked_sub(1).and_then(|p| touches.get(p));
                        if !prev.is_some_and(|(_, p)| p.skill == Skill::Serve && p.team != t.team) {
                            self.push(
                                *i,
                                Severity::Warning,
                                Rule::ServeReception,
                                IssueKind::ReceptionWithoutServe,
                                "reception without a serve from the other team".into(),
                            );
                        }
                        if let Some((_, p)) = prev
                            && p.skill == Skill::Serve
                            && p.team != t.team
                            && let (Some(s), Some(r)) = (p.evaluation(), t.evaluation())
                            && !pass_fits_serve(s, r)
                        {
                            self.push(
                                *i,
                                Severity::Warning,
                                Rule::ServeReception,
                                IssueKind::PassDoesNotFitServe,
                                format!(
                                    "{} doesn't fit the serve {} before it; check one of the two",
                                    Code::Touch(t.clone()),
                                    Code::Touch(p.clone())
                                ),
                            );
                        }
                    }
                    _ => {}
                }
                // A blocked attack and a block point come together (both
                // products check it, docs/research). Their other pairing
                // check, "R/ means the opponent won", is only a hint: an
                // overpass rally is often won by the receiving team (22 in
                // the corpus, e.g. R/ then the server's attack error).
                let prev = j.checked_sub(1).and_then(|p| touches.get(p));
                let is = |t: &vb_grammar::Touch, skill, eval| {
                    t.skill == skill && t.evaluation() == Some(eval)
                };
                if is(t, Skill::Reception, '/') && r.won_by == Some(t.team) {
                    self.push(
                        *i,
                        Severity::Info,
                        Rule::ServeReception,
                        IssueKind::Overpass,
                        format!(
                            "{} is an overpass but {} won the rally; check the evaluation",
                            Code::Touch(t.clone()),
                            side(t.team)
                        ),
                    );
                }
                if is(t, Skill::Attack, '/')
                    && !next.is_some_and(|(_, n)| n.team != t.team && is(n, Skill::Block, '#'))
                {
                    self.push(
                        *i,
                        Severity::Warning,
                        Rule::ServeReception,
                        IssueKind::BlockedWithoutBlockPoint,
                        format!(
                            "{} was blocked but the other team has no block point (B#) after it",
                            Code::Touch(t.clone())
                        ),
                    );
                }
                if is(t, Skill::Block, '#')
                    && !prev.is_some_and(|(_, p)| p.team != t.team && is(p, Skill::Attack, '/'))
                {
                    self.push(
                        *i,
                        Severity::Warning,
                        Rule::ServeReception,
                        IssueKind::BlockPointWithoutBlockedAttack,
                        format!(
                            "block point {} without a blocked attack (A/) from the other team before it",
                            Code::Touch(t.clone())
                        ),
                    );
                }
                // DataVolley records a decided rally as a pair: the touch
                // and the other team's matching one (ace then reception
                // error, blocked attack then block point).
                let winner = |t: &vb_grammar::Touch| {
                    let o = self.m.winning_symbols.outcome(t.skill, t.evaluation()?)?;
                    Some(if o == Outcome::Win {
                        t.team
                    } else {
                        other(t.team)
                    })
                };
                let paired = next.is_some_and(|(_, n)| {
                    n.team != t.team && winner(n).is_some() && winner(n) == winner(t)
                });
                if outcome.is_some()
                    && !paired
                    && let Some((ni, _)) = next
                {
                    self.push(
                        *ni,
                        Severity::Warning,
                        Rule::ServeReception,
                        IssueKind::RallyContinues,
                        format!(
                            "the rally continues after {} ended it",
                            Code::Touch(t.clone())
                        ),
                    );
                }
            }
            // The last touch that decides a rally says who won it.
            let decided = touches.iter().rev().find_map(|(i, t)| {
                let o = self.m.winning_symbols.outcome(t.skill, t.evaluation()?)?;
                Some((
                    *i,
                    t,
                    if o == Outcome::Win {
                        t.team
                    } else {
                        other(t.team)
                    },
                ))
            });
            if let (Some(won), Some((_, t, expected))) = (r.won_by, decided)
                && won != expected
            {
                self.push(
                    r.last,
                    Severity::Error,
                    Rule::ServeReception,
                    IssueKind::PointToWrongTeam,
                    format!(
                        "point to {} but {} means {} won the rally",
                        side(won),
                        Code::Touch(t.clone()),
                        side(expected)
                    ),
                );
            }
        }
    }
}

impl Validator<'_> {
    /// Downgrades issues that follow from an earlier one to Info:
    /// - after a point code with a Score or ServeReception error (wrong
    ///   score, wrong team), a score issue on the next point code;
    /// - after a point given to the wrong team, the rotation issues of the
    ///   next rally, and set-end issues in that set (a point after the set
    ///   was "won", a set end at a losing score) that giving the point to
    ///   the other team would fix;
    /// - after "won a side-out but didn't rotate", that team's rotation
    ///   issues in the set up to the scout's next `z` correction.
    fn knock_ons(&mut self) {
        let mut downgrade: Vec<(usize, usize)> = Vec::new();
        // Point codes with an error, and whether it says the wrong team won.
        let mut seeds: Vec<(usize, bool)> = Vec::new();
        for i in &self.issues {
            if i.severity == Severity::Error
                && matches!(i.rule, Rule::Score | Rule::ServeReception)
                && self.events[i.index].kind() == EventKind::Point
            {
                let wrong_team = i.kind == IssueKind::PointToWrongTeam;
                match seeds.iter_mut().find(|(p, _)| *p == i.index) {
                    Some(seed) => seed.1 |= wrong_team,
                    None => seeds.push((i.index, wrong_team)),
                }
            }
        }
        for (p, wrong_team) in seeds {
            let Some(k) = self.f.states[p].rally else {
                continue;
            };
            let set = self.f.rallies[k].set;
            // The score had the seed point gone to the other team.
            let won_by_home = self.f.states[p].score.home > self.score_before(p).home;
            let flip = |s: Score| {
                if won_by_home {
                    Score {
                        home: s.home.saturating_sub(1),
                        visiting: s.visiting + 1,
                    }
                } else {
                    Score {
                        home: s.home + 1,
                        visiting: s.visiting.saturating_sub(1),
                    }
                }
            };
            let next_rally = self.f.rallies.get(k + 1).filter(|n| n.set == set);
            let next_point = (p + 1..self.events.len())
                .find(|&j| self.events[j].kind() == EventKind::Point)
                .filter(|&j| {
                    self.f.states[j]
                        .rally
                        .is_some_and(|r| self.f.rallies[r].set == set)
                });
            for (n, issue) in self.issues.iter().enumerate() {
                let j = issue.index;
                if issue.severity < Severity::Warning || j <= p {
                    continue;
                }
                let follows = match issue.rule {
                    Rule::Rotation => {
                        wrong_team && next_rally.is_some_and(|r| (r.first..=r.last).contains(&j))
                    }
                    Rule::Score => match self.events[j].kind() {
                        EventKind::SetEnd => {
                            wrong_team
                                && self.f.states[j].set.saturating_sub(1) == set
                                && self.won(set, flip(self.f.states[j].score))
                        }
                        EventKind::Point => {
                            let in_set = self.f.states[j]
                                .rally
                                .is_some_and(|r| self.f.rallies[r].set == set);
                            let before = self.score_before(j);
                            in_set
                                && (Some(j) == next_point
                                    || wrong_team
                                        && self.won(set, before)
                                        && !self.won(set, flip(before)))
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if follows {
                    downgrade.push((n, p));
                }
            }
        }
        for (e, team, seed) in self.missed_rotations.clone() {
            let set = self.f.states[e].set;
            // The setter issues sit on the next serve, which may be the
            // other team's, so go by the team each issue is about.
            let mut later: Vec<(usize, usize)> = self
                .issues
                .iter()
                .enumerate()
                .filter(|&(n, i)| {
                    n != seed
                        && i.rule == Rule::Rotation
                        && i.severity >= Severity::Warning
                        && i.index >= e
                        && self.f.states[i.index].set == set
                        && i.kind.team() == Some(team)
                })
                .map(|(n, i)| (i.index, n))
                .collect();
            later.sort_unstable();
            for (_, n) in later {
                downgrade.push((n, e));
                // The scout's `z` correction ends the chain.
                if self.issues[n].kind.is_setter_zone() {
                    break;
                }
            }
        }
        for (n, cause) in downgrade {
            let issue = &mut self.issues[n];
            if issue.cause.is_none() {
                issue.severity = Severity::Info;
                issue.cause = Some(cause);
            }
        }
    }
}

/// Every issue in the match, in event order.
pub fn validate(m: &Match) -> Vec<Issue> {
    let events = m.log.visible();
    let folded = fold_events(m, &events);
    validate_folded(m, &events, &folded)
}

/// As [`validate`], for events already folded.
pub fn validate_folded(m: &Match, events: &[&MatchEvent], folded: &Folded) -> Vec<Issue> {
    let mut v = Validator {
        m,
        events,
        f: folded,
        issues: Vec::new(),
        missed_rotations: Vec::new(),
    };
    v.codes();
    v.score();
    v.gaps();
    v.rotation();
    v.back_row();
    v.substitutions();
    v.liberos();
    v.serve_reception();
    v.knock_ons();
    let mut issues = v.issues;
    issues.sort_by_key(|i| (i.index, std::cmp::Reverse(i.severity), i.rule));
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{IdGen, NewEvent, Producer};
    use crate::model::{RosterPlayer, Rules, Sides, StartingZone, TeamSheet};

    fn sheet(lineup: [u8; 6], bench: &[u8], libero: Option<u8>) -> TeamSheet {
        let mut players: Vec<RosterPlayer> = lineup
            .iter()
            .enumerate()
            .map(|(i, n)| RosterPlayer {
                number: *n,
                starting_zones: [StartingZone::Zone(i as u8 + 1); 5],
                libero: false,
            })
            .collect();
        for n in bench.iter().copied().chain(libero) {
            players.push(RosterPlayer {
                number: n,
                starting_zones: [StartingZone::None; 5],
                libero: Some(n) == libero,
            });
        }
        TeamSheet { players }
    }

    fn check(codes: &[&str]) -> Vec<String> {
        check_with(codes, Rules::default())
    }

    fn check_with(codes: &[&str], rules: Rules) -> Vec<String> {
        let mut m = Match {
            teams: Sides {
                home: sheet([8, 12, 13, 11, 3, 14], &[5, 6, 7, 9, 10, 15, 16], Some(1)),
                visiting: sheet([16, 11, 18, 6, 10, 13], &[2, 3], None),
            },
            rules,
            ..Match::default()
        };
        let mut ids = IdGen::new(0);
        for c in codes {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        validate(&m)
            .iter()
            .map(|i| match i.cause {
                Some(c) => format!("{} {:?}: {} (knock-on of {c})", i.index, i.rule, i.message),
                None => format!("{} {:?}: {}", i.index, i.rule, i.message),
            })
            .collect()
    }

    const LINEUP: [&str; 4] = ["*P08>LUp", "*z1>LUp", "aP11>LUp", "az2>LUp"];

    fn with_lineup(codes: &[&str]) -> Vec<String> {
        let all: Vec<&str> = LINEUP.iter().chain(codes).copied().collect();
        check(&all)
    }

    #[test]
    fn clean_rallies() {
        assert_eq!(
            with_lineup(&[
                "*08SH-", "a16RH#", "a18AH#", "ap00:01", // side-out
                "az1", "a11SH=", "*p01:01", // serve error
                "*z6", "*12SH+", "a18RH-", "a11EH+", "a10AH/", "*13BH#", "a$$&H=", "*p02:01",
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn score_and_rotation() {
        assert_eq!(
            with_lineup(&["*08SH-", "a16RH#", "a18AH#", "ap00:02", "*12SH-"]),
            [
                "7 Score: point to visiting: score 0:0 → 0:2, expected 0:1",
                "8 Rotation: home serves but visiting won the last rally",
                "8 Rotation: visiting won a side-out but didn't rotate",
                "8 Rotation: home server 12 is in zone 2, not zone 1",
            ]
        );
    }

    #[test]
    fn point_against_the_touches() {
        assert_eq!(
            with_lineup(&["*08SH#", "ap00:01"]),
            ["5 ServeReception: point to visiting but *08SH# means home won the rally"]
        );
    }

    #[test]
    fn substitutions_and_back_row() {
        assert_eq!(
            with_lineup(&[
                "*c13:05", "*c05:07", "*c11:13", "*c01:12", "*08SH-", "a16RH+", "a18AH+", "*01BH-",
                "*08DH+",
            ]),
            [
                "5 Substitution: player 5 can only be replaced by 13",
                "6 Substitution: player 13 can only come on for 5",
                "7 Substitution: player 1 is not on court",
                "7 Substitution: player 12 is already on court",
                "7 Substitution: libero replacements are not substitutions",
                "11 Substitution: libero 1 blocked",
            ]
        );
    }

    #[test]
    fn long_custom_text() {
        let code = |c: &str| {
            check(&[c])
                .into_iter()
                .filter(|m| m.contains("custom"))
                .collect::<Vec<_>>()
        };
        assert_eq!(code("*08SH-~~~~~~~~~ABCDE"), Vec::<String>::new());
        assert_eq!(
            code("*08SH-~~~~~~~~~ABCDEFG"),
            [
                "0 Code: custom text \"ABCDEFG\" is over 5 characters; DataVolley cuts it to \"ABCDE\""
            ]
        );
    }

    #[test]
    fn serve_reception_and_codes() {
        assert_eq!(
            with_lineup(&[
                "*08SH-", "*12AH+", "a16RH#", "*44SH-", "a18AQ#", "ap00:01", "%%"
            ]),
            [
                "5 ServeReception: a serve should be followed by the other team's reception",
                "6 ServeReception: reception without a serve from the other team",
                "7 ServeReception: second serve in one rally; missing point code before it?",
                "7 Code: player 44 is not on the home team sheet",
                "8 ServeReception: a serve should be followed by the other team's reception",
                "10 Rotation: visiting won a side-out but didn't rotate",
                "10 Code: unrecognised code \"%%\"",
            ]
        );
    }

    #[test]
    fn a_pass_rated_against_its_serve_is_a_warning() {
        assert_eq!(
            with_lineup(&[
                "*08SH+", "a16RH+", "a18AH=", "*p01:00", // VolleyStation flags this
                "*08SH+", "a16RH!", "a18AH=", "*p02:00", // a DataVolley convention
                "*08SH-", "a16RH#", "a18AH=", "*p03:00",
            ]),
            [
                "5 ServeReception: a16RH+ doesn't fit the serve *08SH+ before it; check one of the two"
            ]
        );
    }

    #[test]
    fn blocked_attack_and_block_point_pair_up() {
        assert_eq!(
            with_lineup(&[
                "*08SH-", "a16RH#", "a18AH/", "*13EH+", "*p01:00", // A/ without B#
                "*08SH-", "a16RH#", "a18AH-", "*13BH#", "*p02:00", // B# without A/
            ]),
            [
                "6 ServeReception: a18AH/ was blocked but the other team has no block point (B#) after it",
                "7 ServeReception: the rally continues after a18AH/ ended it",
                "12 ServeReception: block point *13BH# without a blocked attack (A/) from the other team before it",
            ]
        );
    }

    #[test]
    fn overpass_won_by_the_receiver_is_a_hint() {
        let issues = with_lineup(&["*08SH/", "a16RH/", "*13AH=", "ap00:01"]);
        assert_eq!(
            issues,
            [
                "5 ServeReception: a16RH/ is an overpass but visiting won the rally; check the evaluation"
            ]
        );
        let m = {
            let mut m = Match::default();
            let mut ids = IdGen::new(0);
            for c in LINEUP
                .iter()
                .chain(&["*08SH/", "a16RH/", "*13AH=", "ap00:01"])
            {
                m.log
                    .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
            }
            m
        };
        assert!(
            validate(&m)
                .iter()
                .filter(|i| i.message.contains("overpass"))
                .all(|i| i.severity == Severity::Info)
        );
    }

    #[test]
    fn knock_ons_are_info() {
        // A point given to the wrong team: the scout carries on as the
        // touches say, so the next rally's rotation issues follow from it.
        assert_eq!(
            with_lineup(&["*08SH#", "ap00:01", "*08SH-", "a16RH#", "a18AH#", "ap00:02"]),
            [
                "5 ServeReception: point to visiting but *08SH# means home won the rally",
                "6 Rotation: home serves but visiting won the last rally (knock-on of 5)",
                "6 Rotation: visiting won a side-out but didn't rotate (knock-on of 5)",
            ]
        );
        // A missed rotation: the server is out of zone 1 until the scout's
        // `z` correction.
        assert_eq!(
            with_lineup(&[
                "*08SH-", "a16RH#", "a18AH#", "ap00:01", "a11SH-", "*12RH+", "*13AH#", "*p01:01",
                "*z6", "*12SH-", "a16RH+", "a18AH=", "*p02:01", "az1", "*12SH-", "a11RH+",
                "a18AH#", "ap02:02", "a11SH-",
            ]),
            [
                "8 Rotation: visiting won a side-out but didn't rotate",
                "8 Rotation: visiting server 11 is in zone 2, not zone 1 (knock-on of 8)",
                "18 Rotation: visiting setter moved from zone 2 to 1; expected 2 (knock-on of 8)",
                "22 Rotation: visiting won a side-out but didn't rotate",
            ]
        );
    }

    #[test]
    fn liberos() {
        assert_eq!(
            with_lineup(&[
                "a16SH-", "*01RH#", "*01AH+", "a18DH+", "a10AH-", "*01BH+", "*p01:00", "*z6",
                "*01SH-"
            ]),
            [
                "6 Substitution: libero 1 attacked",
                "9 Substitution: libero 1 blocked",
                "12 Substitution: libero 1 served",
            ]
        );
    }

    #[test]
    fn rule_options() {
        // Seven substitutions: one too many under FIVB rules, fine with 8.
        let subs = [
            "*c08:05", "*c12:06", "*c13:07", "*c11:09", "*c03:10", "*c14:15", "*c05:08",
        ];
        let all: Vec<&str> = LINEUP.iter().chain(&subs).copied().collect();
        assert_eq!(
            check(&all),
            ["10 Substitution: home substitution 7 in the set; the limit is 6"]
        );
        let eight = Rules {
            substitutions: 8,
            ..Rules::default()
        };
        assert!(check_with(&all, eight).is_empty());

        let serve: Vec<&str> = LINEUP.iter().chain(&["*01SH-"]).copied().collect();
        assert_eq!(check(&serve), ["4 Substitution: libero 1 served"]);
        let libero_serves = Rules {
            libero_may_serve: true,
            ..Rules::default()
        };
        assert!(check_with(&serve, libero_serves).is_empty());
    }

    #[test]
    fn libero_in_the_six_is_a_team_sheet_mislabel() {
        let mut m = Match {
            teams: Sides {
                home: sheet([8, 12, 13, 11, 3, 14], &[], None),
                visiting: sheet([16, 11, 18, 6, 10, 13], &[], None),
            },
            ..Match::default()
        };
        m.teams.home.players[0].libero = true;
        let mut ids = IdGen::new(0);
        for c in LINEUP
            .iter()
            .chain(&["*08SH-", "a16RH#", "a18AH+", "*08DH+", "*08AH#", "*p01:00"])
        {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        let got: Vec<String> = validate(&m).iter().map(|i| i.message.clone()).collect();
        assert_eq!(
            got,
            [
                "home player 8 is a libero on the team sheet but is on court in set 1; team sheet wrong?"
            ]
        );
    }

    #[test]
    fn players_off_court() {
        // 5 is on the bench (no starting zone); 9 too. The sheet in these
        // tests marks nobody as a substitute, so both read as wrong shirts.
        let mut m = Match {
            teams: Sides {
                home: sheet([8, 12, 13, 11, 3, 14], &[5], None),
                visiting: sheet([16, 11, 18, 6, 10, 13], &[], None),
            },
            ..Match::default()
        };
        m.teams.home.players[6].starting_zones[0] = StartingZone::Substitute;
        let mut ids = IdGen::new(0);
        for c in LINEUP
            .iter()
            .chain(&["*08SH-", "a16RH#", "a18AH-", "*05DH+", "*09AH#", "*p01:00"])
        {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        let got: Vec<String> = validate(&m)
            .iter()
            .map(|i| format!("{} {:?}", i.index, i.message))
            .collect();
        assert_eq!(
            got,
            [
                "7 \"home player 5 is not on court; missing substitution?\"",
                "8 \"player 9 is not on the home team sheet\"",
            ]
        );
    }

    /// The scout's report (2026-10-19): a set one point short with no
    /// issue to find the missed rally by.
    #[test]
    fn a_long_gap_before_a_serve_hints_at_a_missed_rally() {
        let mut m = Match {
            teams: Sides {
                home: sheet([8, 12, 13, 11, 3, 14], &[5], None),
                visiting: sheet([16, 11, 18, 6, 10, 13], &[], None),
            },
            ..Match::default()
        };
        let mut ids = IdGen::new(0);
        let mut add = |code: &str, video_s: u64| {
            m.log.append(
                &mut ids,
                NewEvent {
                    video_ms: Some(video_s * 1000),
                    ..NewEvent::code(Code::parse(code), Producer::Keyboard)
                },
            );
        };
        for c in LINEUP {
            add(c, 0);
        }
        // Home serves and wins ten rallies, 20 s from each point to the
        // next serve; 50 s before the 6th serve, and before the 9th with
        // a timeout in between.
        let mut t = 10;
        for n in 1..=10u8 {
            t += match n {
                6 => 50,
                9 => 25,
                _ => 20,
            };
            if n == 9 {
                add("aT", t);
                t += 25;
            }
            add("*08SH#", t);
            add(&format!("*p{n:02}:00"), t + 5);
            t += 5;
        }
        let got: Vec<String> = validate(&m)
            .iter()
            .filter(|i| i.kind == IssueKind::LongGapBeforeServe)
            .map(|i| {
                format!(
                    "{} {}",
                    m.log.visible()[i.index]
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                    i.message
                )
            })
            .collect();
        assert_eq!(
            got,
            ["*08SH# 50 s of video since the last point, usually 20 s; a missed rally?"]
        );
    }

    #[test]
    fn a_player_subbed_off_went_off_this_set() {
        // A live match: nobody's starting zones are on the sheet yet.
        let mut m = Match {
            teams: Sides {
                home: sheet([8, 12, 13, 11, 3, 14], &[5], None),
                visiting: sheet([16, 11, 18, 6, 10, 13], &[], None),
            },
            ..Match::default()
        };
        for p in m.teams.home.players.iter_mut() {
            p.starting_zones = [StartingZone::None; 5];
        }
        let mut ids = IdGen::new(0);
        // The console's lineup entry: a starting six for home.
        m.log.append(
            &mut ids,
            NewEvent {
                body: crate::event::Body::StartingSix {
                    team: Team::Home,
                    court: [8, 12, 13, 11, 3, 14],
                },
                video_ms: None,
                clock: None,
                producer: Producer::Keyboard,
            },
        );
        for c in LINEUP
            .iter()
            .chain(&["*c08:05", "a16SH-", "*12RH+", "*08EH+", "*13AH#", "*p01:00"])
        {
            m.log
                .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
        }
        let got: Vec<String> = validate(&m)
            .iter()
            .filter(|i| i.rule == Rule::Rotation)
            .map(|i| i.message.clone())
            .collect();
        assert_eq!(
            got,
            ["home player 8 went off earlier in set 1; missing substitution?"]
        );
    }

    #[test]
    fn setter_zone_corrections_are_info() {
        let issues = |codes: &[&str]| -> Vec<String> {
            let all: Vec<&str> = LINEUP.iter().chain(codes).copied().collect();
            let mut m = Match {
                teams: Sides {
                    home: sheet([8, 12, 13, 11, 3, 14], &[5], None),
                    visiting: sheet([16, 11, 18, 6, 10, 13], &[2], None),
                },
                ..Match::default()
            };
            let mut ids = IdGen::new(0);
            for c in all {
                m.log
                    .append(&mut ids, NewEvent::code(Code::parse(c), Producer::Keyboard));
            }
            validate(&m)
                .iter()
                .filter(|i| i.rule == Rule::Rotation)
                .map(|i| format!("{:?} {}", i.severity, i.message))
                .collect()
        };
        // Side-out step right after the point, then the scout changes the
        // setter and re-enters z: a correction.
        assert_eq!(
            issues(&[
                "*08SH-", "a16RH#", "a18AH#", "ap00:01", "az1", "ac11:02", "aP02", "az3", "a$$SH-"
            ]),
            ["Info visiting setter position corrected from zone 1 to 3 after the side-out step"]
        );
        // The z straight after the point contradicts the side-out rule.
        assert_eq!(
            issues(&["*08SH-", "a16RH#", "a18AH#", "ap00:01", "az4", "a$$SH-"]),
            ["Warning visiting setter moved from zone 2 to 4; expected 1",]
        );
        // No z after the point, one later on: info.
        assert_eq!(
            issues(&[
                "*08SH-", "a16RH-", "a18AH=", "*p01:00", "aT", "az3", "*08SH-"
            ]),
            [
                "Info visiting setter position set from zone 2 to 3 away from a side-out step (expected 2)"
            ]
        );
    }
}
