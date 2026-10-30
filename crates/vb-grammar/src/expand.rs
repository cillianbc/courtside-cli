//! What the scout types → full codes (PRD F-11).
//!
//! The command line takes shorthand and fills in the rest:
//! - no team prefix means home (`7` is `*07`); one-digit numbers are padded;
//! - a missing skill is [`Defaults::skill`] (attack unless the season
//!   says otherwise), as VolleyStation does, except two predictions: a
//!   rally's first code is the serve, and a number straight after the
//!   other team's serve is their reception. So `8` after the reception or
//!   a dig is `*08AH+` (the console adds the setter's set in front), and
//!   `8` after the other team's attack or a block is the counter-attack,
//!   with the dig left out (the user's call, #24). Scouts type `D` for a
//!   dig and `E` when someone else set or they want the set's details
//!   (`4EK1`: a setter call fills type and evaluation like a combination
//!   does);
//! - VolleyStation's order (its public help articles): a call or
//!   combination may stand in for the skill (`4K1`, `8X5#`), zones go
//!   before the evaluation (`9AQ35#`) and the extended part after it, no
//!   `~` (`8X5#T2`: a tip against two blockers); `~` or a combination or
//!   call right after the evaluation means DataVolley's columns;
//! - a missing type is copied from the touch it answers (reception from the
//!   serve, block and dig from the attack), else the per-skill default;
//! - a missing evaluation is the per-skill default;
//! - compound ("dot") codes: `serve.reception`, `attack.block`,
//!   `attack.dig`. The second half is the other team, its skill implied
//!   (an attack pairs with a block unless `D` is typed), the type copied,
//!   the zones shared (a block's zone mirrors the attack's, 4 ↔ 2), and
//!   whichever evaluation is missing derived from the other one
//!   (as both programs fill it in);
//! - commands (`T`, `c6.7`, `z3`, `P5`, `p25:23`) work without the `*`;
//!   substitution and point numbers are padded to two digits, a setter
//!   number is kept as typed (DataVolley writes `aP2`).
//!
//! Expansion only writes codes; whether they make sense is the validator's
//! job.

use crate::code::{Code, Skill, Team, Touch};

/// The season's typing defaults (PRD F-03); set in the season settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defaults {
    /// Skill when none is typed and the rally predicts none.
    pub skill: Skill,
    /// Type per skill when none is typed or inherited, in the order
    /// serve, reception, set, attack, block, dig, freeball.
    pub types: [char; 7],
    /// Evaluation per skill when none is typed, same order.
    pub evaluations: [char; 7],
    /// Read a bare number straight after the other team's serve as their
    /// reception. A rally's first code is the serve either way.
    pub predict: bool,
    /// Attack combinations. A typed combination fills a missing type and
    /// start zone.
    pub combinations: Vec<Combination>,
}

/// An attack combination from the match's table (`X5`: a fast set to
/// zone 4, aimed at the front attacker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combination {
    pub code: String,
    /// The attack's type letter.
    pub tempo: char,
    /// Start zone digit, or `~` when the table has none.
    pub start_zone: char,
    /// Who the set goes to (`F`, `B`, `C`, `P`, `S`), if the table says.
    pub target: Option<char>,
}

impl Defaults {
    pub fn combination(&self, code: &str) -> Option<&Combination> {
        self.combinations.iter().find(|c| c.code == code)
    }
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            skill: Skill::Attack,
            types: ['H'; 7],
            evaluations: ['+'; 7],
            predict: true,
            combinations: Vec::new(),
        }
    }
}

fn slot(s: Skill) -> usize {
    match s {
        Skill::Serve => 0,
        Skill::Reception => 1,
        Skill::Set => 2,
        Skill::Attack => 3,
        Skill::Block => 4,
        Skill::Dig => 5,
        Skill::Freeball | Skill::Green => 6,
    }
}

/// What a typed token became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub codes: Vec<Code>,
    /// What was filled in, in words: "skill predicted", "type from the
    /// serve", "evaluation default" …; empty when everything was typed.
    pub filled: Vec<String>,
}

const TYPES: &str = "HMQTUNO";
const EVALUATIONS: &str = "=/-!+#";

/// Expands one token of the command line. `before` is the last touch of
/// the rally so far (for predicting the skill and inheriting the type).
pub fn expand(token: &str, before: Option<&Touch>, d: &Defaults) -> Result<Expanded, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Type a code first.".into());
    }
    if token.starts_with("**") {
        return Ok(Expanded {
            codes: vec![Code::parse(token)],
            filled: Vec::new(),
        });
    }
    if let Some(cmd) = command(token) {
        return Ok(Expanded {
            codes: vec![Code::parse(&cmd)],
            filled: Vec::new(),
        });
    }
    // A dot in a full code's custom text (column 15 on, `…14F.+`) is
    // text, not the split of a dot code.
    let pair = token
        .find('.')
        .filter(|i| *i < CUSTOM_TEXT_COLUMN)
        .map(|i| (&token[..i], &token[i + 1..]));
    match pair {
        Some((first, second)) => compound(first, second, before, d),
        None => {
            let typed = typed_touch(token, false)?;
            let mut filled = Vec::new();
            let touch = complete(&typed, before, d, &mut filled)?;
            Ok(Expanded {
                codes: vec![Code::Touch(touch)],
                filled,
            })
        }
    }
}

/// Where a full code's custom text starts (0-based).
const CUSTOM_TEXT_COLUMN: usize = 15;

/// `T`, `c6.7`, `z3`, `P5`, `p25:23` with or without the team prefix.
fn command(token: &str) -> Option<String> {
    let (team, rest) = split_team(token);
    let mut chars = rest.chars();
    let letter = chars.next()?;
    let tail = chars.as_str();
    let two = |s: &str| -> Option<String> {
        let n: u8 = s.parse().ok()?;
        Some(format!("{n:02}"))
    };
    let t = team.as_char();
    match letter {
        'T' if tail.is_empty() => Some(format!("{t}T")),
        'c' | 'C' => {
            let (out, inn) = tail.split_once(['.', ':'])?;
            Some(format!("{t}c{}:{}", two(out)?, two(inn)?))
        }
        'z' | 'Z' if tail.len() == 1 && tail.chars().all(|c| ('1'..='6').contains(&c)) => {
            Some(format!("{t}z{tail}"))
        }
        // Kept as typed: DataVolley writes setter changes as `aP2`.
        'P' if !tail.is_empty() && tail.len() <= 2 && tail.chars().all(|c| c.is_ascii_digit()) => {
            Some(format!("{t}P{tail}"))
        }
        'p' => {
            let (h, v) = tail.split_once(':')?;
            Some(format!("{t}p{}:{}", two(h)?, two(v)?))
        }
        _ => None,
    }
}

fn split_team(token: &str) -> (Team, &str) {
    if let Some(rest) = token.strip_prefix('a') {
        (Team::Visiting, rest)
    } else if let Some(rest) = token.strip_prefix('*') {
        (Team::Home, rest)
    } else {
        (Team::Home, token)
    }
}

/// A touch as typed: what is there, nothing filled yet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Typed {
    /// `None` when no prefix was typed (home unless a pair says otherwise).
    team: Option<Team>,
    player: String,
    skill: Option<Skill>,
    skill_type: Option<char>,
    evaluation: Option<char>,
    /// Between the type and the evaluation, VolleyStation's order: call or
    /// combination, target, zones, subzone (`X55`, `K13C`, `35`).
    pre: String,
    /// After the evaluation: the extended part (`T2`, `H1O`), or positions
    /// 7 on by column (`~~~48`, `X5~4`).
    post: String,
}

/// `implied`: the skill may be left out though more follows (a dot
/// code's halves, whose skill the pair implies).
fn typed_touch(token: &str, implied: bool) -> Result<Typed, String> {
    let team = match token.chars().next() {
        Some('a') => Some(Team::Visiting),
        Some('*') => Some(Team::Home),
        _ => None,
    };
    let s = if team.is_some() { &token[1..] } else { token };
    let (player, rest) = if let Some(r) = s.strip_prefix("$$") {
        ("$$".to_owned(), r)
    } else {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        if end == 0 {
            return Err(if team.is_none() {
                "Start with the shirt number (a for the away team).".into()
            } else {
                "Add the shirt number, or T, c, z or p.".into()
            });
        }
        let n: u8 = s[..end]
            .parse()
            .ok()
            .filter(|n| *n <= 99)
            .ok_or("Shirt numbers go from 0 to 99.")?;
        (format!("{n:02}"), &s[end..])
    };
    // Letters after the number are codes, whatever case they are typed in.
    let rest = rest.to_ascii_uppercase();
    let mut chars = rest.chars().peekable();
    // Skill and type letters don't overlap, so the skill may be left out.
    let skill = match chars.peek() {
        Some(&c) if TYPES.contains(c) => None,
        // A setter call or a combination stands in for the skill (`4K1`,
        // `8X5#`); `complete` tells which.
        Some(&c) if c.is_ascii_alphabetic() && Skill::from_char(c).is_none() => None,
        Some(&c) if c.is_ascii_alphabetic() || c == '&' => {
            chars.next();
            Some(Skill::from_char(c).ok_or(format!("Unknown skill letter {c}."))?)
        }
        _ => None,
    };
    let skill_type = match chars.peek() {
        Some(&c) if TYPES.contains(c) || (c == '~' && skill.is_some()) => {
            chars.next();
            (c != '~').then_some(c)
        }
        _ => None,
    };
    let rest: String = chars.collect();
    // By column when `~` holds the gaps (DataVolley's layout); else the
    // evaluation may come after the zones (VolleyStation's).
    let at = rest.find(|c| EVALUATIONS.contains(c));
    let (pre, evaluation, post) = match at {
        // `~` before any evaluation: by column with the evaluation left out.
        _ if rest.find('~').is_some_and(|g| at.is_none_or(|i| g < i)) => {
            (String::new(), None, rest)
        }
        Some(i) => (
            rest[..i].to_owned(),
            rest[i..].chars().next(),
            rest[i + 1..].to_owned(),
        ),
        None => (rest, None, String::new()),
    };
    if skill.is_none()
        && !implied
        && (post.contains('~') || pre.starts_with(|c: char| c.is_ascii_digit()))
    {
        return Err("Put the skill letter (S R E A B D F) before the zones and the rest.".into());
    }
    Ok(Typed {
        team,
        player,
        skill,
        skill_type,
        evaluation,
        pre,
        post,
    })
}

/// The skill `before` settles for `team`'s next touch: the serve to
/// start a rally, and the reception after the other team's serve unless
/// the season turned that off ([`Defaults::predict`]). Anything else
/// takes [`Defaults::skill`], as VolleyStation does: a number after the
/// other team's attack or a block is the counter-attack, not a dig (the
/// user's call, #24).
fn predicted(before: Option<&Touch>, team: Team, d: &Defaults) -> Option<Skill> {
    let Some(b) = before else {
        return Some(Skill::Serve);
    };
    (d.predict && b.skill == Skill::Serve && b.team != team).then_some(Skill::Reception)
}

/// The type a touch answering `before` takes, if it inherits one.
fn inherited_type(skill: Skill, before: Option<&Touch>) -> Option<(char, &'static str)> {
    let b = before?;
    let from = match (skill, b.skill) {
        (Skill::Reception, Skill::Serve) => "the serve",
        (Skill::Block | Skill::Dig, Skill::Attack) => "the attack",
        _ => return None,
    };
    b.skill_type().map(|t| (t, from))
}

fn complete(
    t: &Typed,
    before: Option<&Touch>,
    d: &Defaults,
    filled: &mut Vec<String>,
) -> Result<Touch, String> {
    let team = t.team.unwrap_or(Team::Home);
    let skill = match t.skill {
        Some(s) => s,
        None if t.pre.starts_with('K') && t.pre.len() >= 2 => {
            filled.push("set from the call".into());
            Skill::Set
        }
        None if t.pre.get(..2).is_some_and(|c| d.combination(c).is_some()) => {
            filled.push("attack from the combination".into());
            Skill::Attack
        }
        None if t.pre.starts_with(|c: char| c.is_ascii_alphabetic()) => {
            let c = t.pre.chars().next().unwrap_or('?');
            return Err(format!("Unknown skill letter {c}."));
        }
        None => match predicted(before, team, d) {
            Some(s) => {
                filled.push("skill predicted".into());
                s
            }
            None => {
                filled.push("skill default".into());
                d.skill
            }
        },
    };
    let player = if t.player == "$$" {
        crate::code::Player::Unknown
    } else {
        match Code::parse(&format!("*{}S", t.player)) {
            Code::Touch(x) => x.player,
            _ => return Err("Shirt numbers go from 0 to 99.".into()),
        }
    };
    let rest = layout(skill, t, d)?;
    // An attack typed with a combination straight after the skill
    // (`5AX5`): the combination stands in for type and evaluation.
    let combination = (skill == Skill::Attack)
        .then(|| rest.get(..2))
        .flatten()
        .and_then(|code| d.combination(code));
    // A set typed with its setter call (`4EK1`): calls start with `K`, a
    // letter no type uses, so type and evaluation take their defaults.
    let call = skill == Skill::Set && rest.starts_with('K') && rest.len() >= 2;
    if t.skill_type.is_none()
        && t.evaluation.is_none()
        && combination.is_none()
        && !call
        && let Some(c) = rest.chars().next().filter(char::is_ascii_alphabetic)
    {
        return Err(format!("Unknown type letter {c}."));
    }
    let skill_type = match t.skill_type {
        Some(c) => c,
        None if combination.is_some() => {
            filled.push("type from the combination".into());
            combination.map_or('H', |c| c.tempo)
        }
        None => {
            match inherited_type(skill, before).filter(|_| before.is_some_and(|b| b.team != team)) {
                Some((c, from)) => {
                    filled.push(format!("type from {from}"));
                    c
                }
                None => {
                    filled.push("type default".into());
                    d.types[slot(skill)]
                }
            }
        }
    };
    let evaluation = match t.evaluation {
        Some(c) => c,
        None => {
            filled.push("evaluation default".into());
            d.evaluations[slot(skill)]
        }
    };
    let mut touch = build(team, player, skill, skill_type, evaluation, &rest);
    // The combination's start zone when none is typed.
    if skill == Skill::Attack
        && touch.start_zone().is_none()
        && let Some(code) = touch.combination()
        && let Some(c) = d.combination(&code)
    {
        filled.push("zone from the combination".into());
        touch = touch.with_start_zone(c.start_zone);
    }
    Ok(touch)
}

/// Positions 7 on, from what was typed around the evaluation
/// (VolleyStation's order, from its public help articles). Before it: a call or combination
/// (two characters), a set's target, one or two zones and an end subzone.
/// One zone is the start zone, except on a set (DataVolley keeps a set's
/// zone as its end zone) or after a combination (which gives the start).
/// After it: the extended part, the skill's subtype, number of players
/// and special (`T2`: a tip against two blockers). What starts with a
/// combination or call, or has `~` in it, is by column, as DataVolley
/// writes it.
fn layout(skill: Skill, t: &Typed, d: &Defaults) -> Result<String, String> {
    let (pre, post) = (t.pre.as_str(), t.post.as_str());
    let named = |s: &str| {
        s.get(..2).is_some_and(|c| {
            (skill == Skill::Set && c.starts_with('K')) || d.combination(c).is_some()
        })
    };
    let ext = extended(skill, post, t.evaluation);
    if post.contains('~') || named(post) || (ext.is_none() && !post.is_empty()) {
        if !pre.is_empty() {
            return Err(format!(
                "Zones go before the evaluation and {post} after it in its columns: type one or the other."
            ));
        }
        return Ok(post.to_owned());
    }
    let mut cols = ['~'; 9];
    let mut p = pre.chars().peekable();
    if p.peek().is_some_and(char::is_ascii_alphabetic) {
        cols[0] = p.next().unwrap_or('~');
        cols[1] = p
            .next()
            .ok_or(format!("{pre}: a call or combination has two characters."))?;
    }
    if skill == Skill::Set
        && cols[0] == 'K'
        && let Some(c) = p.next_if(|c| "FBCPS".contains(*c))
    {
        cols[2] = c;
    }
    let mut zones = Vec::new();
    while let Some(z) = p.next_if(char::is_ascii_digit) {
        zones.push(z);
    }
    match zones[..] {
        [] => {}
        [z] if skill == Skill::Set || cols[0] != '~' => cols[4] = z,
        [z] => cols[3] = z,
        [a, b] => (cols[3], cols[4]) = (a, b),
        _ => return Err(format!("{pre}: at most two zones, start and end.")),
    }
    if !zones.is_empty()
        && let Some(c) = p.next_if(|c| ('A'..='D').contains(c))
    {
        cols[5] = c;
    }
    if let Some(c) = p.next() {
        return Err(format!("{pre}: can't read {c} there."));
    }
    if let Some([a, b, c]) = ext {
        cols[6..9].copy_from_slice(&[a, b, c]);
    }
    let out: String = cols.iter().collect();
    Ok(out.trim_end_matches('~').to_owned())
}

/// The extended part typed after the evaluation, as subtype, number of
/// players and special (`~` where left out), with the values each skill
/// takes (the meanings as openvolley's `datavolley` reads them); `None` if it isn't one.
///
/// VolleyStation types an overhead reception `Z` (its help article
/// "Reception code"), which DataVolley and VolleyStation's own `.dvw`
/// exports write as `O`; DataVolley's `Z` is a referee call, which only
/// comes on a reception error. So `Z` on a reception that isn't an error
/// is overhand.
fn extended(skill: Skill, text: &str, evaluation: Option<char>) -> Option<[char; 3]> {
    let overhead = skill == Skill::Reception && evaluation.is_some_and(|e| e != '=');
    let (subtypes, players, specials) = match skill {
        Skill::Attack => ("HPT", "01234", "ACFINOSXZ"),
        Skill::Block => ("AT", "01234", "FINOPSTXZ"),
        Skill::Reception if overhead => ("LMORWZ", "123456789", "EPUXZ"),
        Skill::Reception => ("LMORW", "123456789", "EPUXZ"),
        Skill::Dig => ("BCES", "", "EFOPUXZ"),
        Skill::Set => ("12345", "", "IUZ"),
        Skill::Serve => ("", "", "LNORZ"),
        Skill::Freeball => ("", "", "EPUXZ"),
        Skill::Green => ("", "", ""),
    };
    let mut p = text.chars().peekable();
    let mut take = |set: &str| p.next_if(|c| set.contains(*c)).unwrap_or('~');
    let mut out = [take(subtypes), take(players), take(specials)];
    if overhead && out[0] == 'Z' {
        out[0] = 'O';
    }
    p.next().is_none().then_some(out)
}

fn build(
    team: Team,
    player: crate::code::Player,
    skill: Skill,
    ty: char,
    ev: char,
    rest: &str,
) -> Touch {
    let p = match &player {
        crate::code::Player::Number(d) => d.as_str().to_owned(),
        crate::code::Player::Unknown => "$$".into(),
    };
    match Code::parse(&format!(
        "{}{p}{}{ty}{ev}{rest}",
        team.as_char(),
        skill.as_char()
    )) {
        Code::Touch(t) => t,
        // Unreachable for the shapes built here; keep what we have.
        _ => Touch::new(team, player, skill)
            .with_skill_type(ty)
            .with_evaluation(ev),
    }
}

/// What the second half of a pair may be rated, given the first half's
/// rating (GitHub #22): each rating whose usual first half is `ev`
/// (`first_from_second` read backwards), a good one before a perfect one;
/// a weak attack can also be dug weakly.
fn second_allowed(first: Skill, second: Skill, ev: char) -> Option<Vec<char>> {
    let mut allowed: Vec<char> = ['+', '#', '-', '!', '/', '=']
        .into_iter()
        .filter(|&r| first_from_second(first, second, r) == Some(ev))
        .collect();
    if (first, second, ev) == (Skill::Attack, Skill::Dig, '-') {
        allowed.push('-');
    }
    (!allowed.is_empty()).then_some(allowed)
}

/// The second half's rating for the first half's: `keep` (the season's
/// default, or what the half already had) when the first half allows it,
/// else the first rating it allows. So a
/// serve `-` gives the default `+` pass, not a perfect one.
fn second_from_first(first: Skill, second: Skill, ev: char, keep: Option<char>) -> Option<char> {
    let allowed = second_allowed(first, second, ev)?;
    keep.filter(|k| allowed.contains(k))
        .or_else(|| allowed.first().copied())
}

/// The first half's evaluation for the second half's, by pair: the usual
/// reading of a rally (a perfect pass means an easy serve, a kill a dig
/// error, a stuff block a blocked attack).
fn first_from_second(first: Skill, second: Skill, ev: char) -> Option<char> {
    Some(match (first, second, ev) {
        (Skill::Serve, Skill::Reception, '/') => '/',
        (Skill::Serve, Skill::Reception, '#' | '+') => '-',
        (Skill::Serve, Skill::Reception, '!') => '!',
        (Skill::Serve, Skill::Reception, '-') => '+',
        (Skill::Serve, Skill::Reception, '=') => '#',
        (Skill::Attack, Skill::Block, '#') => '/',
        (Skill::Attack, Skill::Block, '+') => '-',
        (Skill::Attack, Skill::Block, '!') => '!',
        (Skill::Attack, Skill::Block, '-') => '+',
        (Skill::Attack, Skill::Block, '=') => '#',
        (Skill::Attack, Skill::Dig, '#' | '+') => '-',
        (Skill::Attack, Skill::Dig, '-') => '+',
        (Skill::Attack, Skill::Dig, '=') => '#',
        _ => return None,
    })
}

fn mirror(z: char) -> char {
    match z {
        '4' => '2',
        '2' => '4',
        z => z,
    }
}

fn compound(
    first: &str,
    second: &str,
    before: Option<&Touch>,
    d: &Defaults,
) -> Result<Expanded, String> {
    let a = typed_touch(first, true)?;
    let mut b = typed_touch(second, true)?;
    let mut filled = Vec::new();
    let team_a = a.team.unwrap_or(Team::Home);
    let team_b = other(team_a);
    if b.team.is_some_and(|t| t != team_b) {
        return Err("The part after the dot is the other team's touch.".into());
    }
    b.team = Some(team_b);
    let skill_a = match a.skill {
        Some(s) => s,
        // A combination stands in for the attack (`2X2.a8=`).
        None if a.pre.get(..2).is_some_and(|c| d.combination(c).is_some()) => Skill::Attack,
        None => match predicted(before, team_a, d) {
            Some(s @ (Skill::Serve | Skill::Attack)) => s,
            _ => Skill::Attack,
        },
    };
    let skill_b = match (skill_a, b.skill) {
        (Skill::Serve, None | Some(Skill::Reception)) => Skill::Reception,
        (Skill::Attack, None | Some(Skill::Block)) => Skill::Block,
        (Skill::Attack, Some(Skill::Dig)) => Skill::Dig,
        _ => {
            return Err(
                "Dot codes pair a serve with a reception, or an attack with a block or dig.".into(),
            );
        }
    };
    if a.skill.is_none() {
        filled.push(format!("{} from the dot", name(skill_a)));
    }
    let rest_a = layout(skill_a, &a, d)?;
    // The attack's combination gives a missing type and start zone, as on
    // a single code.
    let combination = (skill_a == Skill::Attack)
        .then(|| rest_a.get(..2))
        .flatten()
        .and_then(|code| d.combination(code));
    // Type: one half's for the other, else the combination's, else the
    // first half's default.
    let ty = a
        .skill_type
        .or(b.skill_type)
        .or_else(|| {
            let c = combination?;
            filled.push("type from the combination".into());
            Some(c.tempo)
        })
        .or_else(|| {
            filled.push("type default".into());
            Some(d.types[slot(skill_a)])
        })
        .unwrap_or('H');
    let (ev_a, ev_b) = match (a.evaluation, b.evaluation) {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) => {
            filled.push("second evaluation derived".into());
            (
                x,
                second_from_first(skill_a, skill_b, x, Some(d.evaluations[slot(skill_b)]))
                    .unwrap_or(d.evaluations[slot(skill_b)]),
            )
        }
        (None, Some(y)) => {
            filled.push("first evaluation derived".into());
            (
                first_from_second(skill_a, skill_b, y).unwrap_or(d.evaluations[slot(skill_a)]),
                y,
            )
        }
        (None, None) => {
            filled.push("evaluations default".into());
            let y = d.evaluations[slot(skill_b)];
            (
                first_from_second(skill_a, skill_b, y).unwrap_or(d.evaluations[slot(skill_a)]),
                y,
            )
        }
    };
    let player = |t: &Typed| match Code::parse(&format!("*{}S", t.player)) {
        Code::Touch(x) => x.player,
        _ => crate::code::Player::Unknown,
    };
    let mut first = build(team_a, player(&a), skill_a, ty, ev_a, &rest_a);
    if let Some(c) = combination
        && first.start_zone().is_none()
    {
        filled.push("zone from the combination".into());
        first = first.with_start_zone(c.start_zone);
    }
    let mut second = build(
        team_b,
        player(&b),
        skill_b,
        ty,
        ev_b,
        &layout(skill_b, &b, d)?,
    );
    // Zones typed on one half go on the other.
    let zones = |t: &Touch| (t.start_zone(), t.end_zone(), t.end_subzone());
    let (za, zb) = (zones(&first), zones(&second));
    if skill_b == Skill::Block {
        if let (None, Some(s)) = (zb.0, za.0) {
            second = second.with_start_zone(mirror(s));
        }
    } else {
        let pick = |x: Option<char>, y: Option<char>| x.or(y);
        let (s, e, sub) = (pick(za.0, zb.0), pick(za.1, zb.1), pick(za.2, zb.2));
        for (t, z) in [(&mut first, za), (&mut second, zb)] {
            if z.0.is_none()
                && let Some(c) = s
            {
                *t = t.clone().with_start_zone(c);
            }
            if z.1.is_none()
                && let Some(c) = e
            {
                *t = t.clone().with_end_zone(c);
            }
            if z.2.is_none()
                && let Some(c) = sub
            {
                *t = t.clone().with_end_subzone(c);
            }
        }
    }
    Ok(Expanded {
        codes: vec![Code::Touch(first), Code::Touch(second)],
        filled,
    })
}

/// Whether `first` then `second` is a compound pair: a serve and the other
/// team's reception, or an attack and the other team's block or dig.
pub fn is_pair(first: &Touch, second: &Touch) -> bool {
    first.team != second.team
        && matches!(
            (first.skill, second.skill),
            (Skill::Serve, Skill::Reception) | (Skill::Attack, Skill::Block | Skill::Dig)
        )
}

/// One half of a pair was corrected from `old` to `new`; the other half,
/// `partner`, follows what changed, as when the pair was typed: the type,
/// the evaluation through the pairing table, the zones (a block's start
/// zone mirrors the attack's). `edited_first` says which half was edited.
/// `None` when nothing follows or the edit broke the pair.
pub fn follow_pair(old: &Touch, new: &Touch, partner: &Touch, edited_first: bool) -> Option<Touch> {
    let (first, second) = if edited_first {
        (new, partner)
    } else {
        (partner, new)
    };
    if !is_pair(first, second) {
        return None;
    }
    let mut p = partner.clone();
    if new.skill_type() != old.skill_type()
        && let Some(t) = new.skill_type()
    {
        p = p.with_skill_type(t);
    }
    if new.evaluation() != old.evaluation()
        && let Some(e) = new.evaluation()
    {
        let derived = if edited_first {
            // The other half keeps its rating while it still fits.
            second_from_first(first.skill, second.skill, e, partner.evaluation())
        } else {
            first_from_second(first.skill, second.skill, e)
        };
        if let Some(d) = derived {
            p = p.with_evaluation(d);
        }
    }
    let slot = |z: Option<char>| z.unwrap_or('~');
    if second.skill == Skill::Block {
        if new.start_zone() != old.start_zone() {
            p = p.with_start_zone(new.start_zone().map_or('~', mirror));
        }
    } else {
        if new.start_zone() != old.start_zone() {
            p = p.with_start_zone(slot(new.start_zone()));
        }
        if new.end_zone() != old.end_zone() {
            p = p.with_end_zone(slot(new.end_zone()));
        }
        if new.end_subzone() != old.end_subzone() {
            p = p.with_end_subzone(slot(new.end_subzone()));
        }
    }
    (p != *partner).then_some(p)
}

/// An attack's combination was corrected from `old` to `new`; `set` is the
/// touch straight before it. When that is the same team's set, its target
/// follows the new combination's, as DataVolley and VolleyStation re-derive
/// it: only where the set had no target
/// or still had the old combination's, so a target the scout typed stays.
/// `None` when nothing changes.
pub fn follow_combination(old: &Touch, new: &Touch, set: &Touch, d: &Defaults) -> Option<Touch> {
    if new.skill != Skill::Attack || set.skill != Skill::Set || set.team != new.team {
        return None;
    }
    let (was, now) = (old.combination(), new.combination());
    if was == now {
        return None;
    }
    let target = |code: Option<String>| code.and_then(|c| d.combination(&c)?.target);
    let next = target(now);
    let derived = set.target().is_none() || set.target() == target(was);
    (derived && next != set.target()).then(|| set.clone().with_target(next.unwrap_or('~')))
}

fn other(t: Team) -> Team {
    match t {
        Team::Home => Team::Visiting,
        Team::Visiting => Team::Home,
    }
}

fn name(s: Skill) -> &'static str {
    match s {
        Skill::Serve => "serve",
        Skill::Attack => "attack",
        _ => "skill",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(code: &str, tempo: char, zone: char, target: char) -> Combination {
        Combination {
            code: code.into(),
            tempo,
            start_zone: zone,
            target: Some(target),
        }
    }

    fn touch(s: &str) -> Touch {
        match Code::parse(s) {
            Code::Touch(t) => t,
            _ => panic!("not a touch: {s}"),
        }
    }

    #[test]
    fn a_set_follows_its_attack_combination() {
        let d = Defaults {
            combinations: vec![combo("X5", 'T', '4', 'F'), combo("X1", 'Q', '3', 'C')],
            ..Defaults::default()
        };
        let follow = |old: &str, new: &str, set: &str| {
            follow_combination(&touch(old), &touch(new), &touch(set), &d).map(|t| t.to_string())
        };
        let (x5, x1) = ("a02AT+X5~4", "a07AQ+X1~3");
        assert_eq!(follow(x5, x1, "a05EH+~~F").as_deref(), Some("a05EH+~~C"));
        assert_eq!(
            follow(x5, x1, "a05EH+").as_deref(),
            Some("a05EH+~~C"),
            "none yet"
        );
        assert_eq!(follow(x5, x1, "a05EH+~~B"), None, "typed by the scout");
        assert_eq!(follow(x5, x1, "*05EH+~~F"), None, "the other team's set");
        assert_eq!(follow(x5, x5, "a05EH+~~F"), None, "same combination");
        assert_eq!(
            follow(x5, "a02AT+", "a05EH+~~F").as_deref(),
            Some("a05EH+"),
            "combination taken off"
        );
        assert_eq!(follow(x1, "a02AT+V9", "a05EH+"), None, "not in the table");
    }

    fn exp(token: &str, before: Option<&str>) -> Vec<String> {
        let b = before.map(|s| match Code::parse(s) {
            Code::Touch(t) => t,
            _ => panic!("not a touch: {s}"),
        });
        expand(token, b.as_ref(), &Defaults::default())
            .unwrap_or_else(|e| panic!("{token}: {e}"))
            .codes
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn fills_what_is_missing() {
        assert_eq!(
            exp("7", None),
            ["*07SH+"],
            "first touch of a rally is the serve"
        );
        assert_eq!(
            exp("a10", Some("*07SM-")),
            ["a10RM+"],
            "reception takes the serve's type"
        );
        assert_eq!(
            exp("8", Some("*05RH+")),
            ["*08AH+"],
            "the attack follows the reception: sets aren't predicted"
        );
        assert_eq!(exp("8#", Some("*05RH+")), ["*08AH#"], "a kill");
        assert_eq!(exp("8=", Some("*05DH+")), ["*08AH="]);
        assert_eq!(exp("8F", Some("*05RH+")), ["*08FH+"], "a typed skill stays");
        assert_eq!(exp("a15E", Some("a10RM+")), ["a15EH+"], "a set is typed");
        assert_eq!(
            exp("4EK1", Some("*05RH+")),
            ["*04EH+K1"],
            "a set with its call"
        );
        assert_eq!(
            exp("4E#K1F", Some("*05RH+")),
            ["*04EH#K1F"],
            "call and target"
        );
        assert_eq!(
            exp("4EQ#K7C~~~1", Some("*05RH+")),
            ["*04EQ#K7C~~~1"],
            "everything after the evaluation is kept, by column"
        );
        assert_eq!(
            exp("8A#~~~48~T2", Some("*04EH+")),
            ["*08AH#~~~48~T2"],
            "zones, tip, two blockers"
        );
        assert_eq!(
            exp("a8#", Some("*07SH+")),
            ["a08RH#"],
            "a perfect pass stays"
        );
        assert_eq!(exp("a2", Some("a15EQ+")), ["a02AH+"]);
        assert_eq!(
            exp("8", Some("a02AQ+")),
            ["*08AH+"],
            "after the other team's attack: the counter-attack (#24)"
        );
        assert_eq!(
            exp("8D", Some("a02AQ+")),
            ["*08DQ+"],
            "a typed dig takes the attack's type"
        );
        assert_eq!(exp("8", Some("a03BH+")), ["*08AH+"], "after a block");
        assert_eq!(
            exp("a3", Some("a07BH-")),
            ["a03AH+"],
            "after their own block"
        );
        assert_eq!(exp("8", Some("a04EH-")), ["*08AH+"], "after an overpass");
    }

    /// The season's default skill decides every bare number except the
    /// reception after a serve; with prediction off, that one too (#24).
    #[test]
    fn a_bare_number_takes_the_default_skill() {
        let rec = match Code::parse("*07RH+") {
            Code::Touch(t) => t,
            _ => unreachable!(),
        };
        let serve = match Code::parse("a11SH+") {
            Code::Touch(t) => t,
            _ => unreachable!(),
        };
        let one = |token: &str, before: &Touch, d: &Defaults| {
            expand(token, Some(before), d).map(|x| x.codes[0].to_string())
        };
        let sets = Defaults {
            skill: Skill::Set,
            ..Defaults::default()
        };
        assert_eq!(one("4", &rec, &sets).as_deref(), Ok("*04EH+"));
        assert_eq!(
            one("7", &serve, &sets).as_deref(),
            Ok("*07RH+"),
            "still the reception"
        );
        let raw = Defaults {
            predict: false,
            ..Defaults::default()
        };
        assert_eq!(
            one("7", &serve, &raw).as_deref(),
            Ok("*07AH+"),
            "VolleyStation's way"
        );
        // A rally still starts with the serve, single or dot code.
        let first = |token: &str| expand(token, None, &raw).map(|x| x.codes[0].to_string());
        assert_eq!(first("7").as_deref(), Ok("*07SH+"));
        assert_eq!(first("7.a8#").as_deref(), Ok("*07SH-"));
        assert_eq!(exp("5a#", Some("*08DQ+")), ["*05AH#"], "lower case letters");
        assert_eq!(
            exp("a2AQ#X5~42", None),
            ["a02AQ#X5~42"],
            "a full code is kept"
        );
        assert_eq!(
            exp("a2AQX1", None),
            ["a02AQ+X1"],
            "combination without evaluation"
        );
        assert_eq!(exp("$$&", None), ["*$$&H+"]);
        let d = Defaults {
            combinations: vec![combo("X5", 'T', '4', 'F'), combo("X1", 'Q', '3', 'C')],
            ..Defaults::default()
        };
        let with = |s: &str| expand(s, None, &d).expect(s).codes[0].to_string();
        assert_eq!(
            with("a2AX5"),
            "a02AT+X5~4",
            "combination gives type and zone"
        );
        assert_eq!(with("a2AQ#X1"), "a02AQ#X1~3");
        assert_eq!(with("a2AH#X5~2"), "a02AH#X5~2", "typed values win");
        assert!(expand("a2AZ9", None, &d).is_err(), "not a combination");
        assert_eq!(
            exp("a13#", Some("a14EQ+")),
            ["a13AH#"],
            "skill predicted, evaluation typed"
        );
        assert_eq!(
            exp("7M-", None),
            ["*07SM-"],
            "type and evaluation, skill predicted"
        );
    }

    /// VolleyStation's documented input order, with made-up codes: zones
    /// before the evaluation, the extended part after it, no `~`; each
    /// kind of code its help describes (attack, block, set, reception).
    #[test]
    fn volleystation_input_order() {
        let all = |s: &str, before: &str| {
            let b = touch(before);
            expand(s, Some(&b), &Defaults::default())
                .unwrap_or_else(|e| panic!("{s}: {e}"))
                .codes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        let set = "*01EH+";
        // Attack: zones, subzone; hit, blockers, how it ended.
        assert_eq!(all("*15AQ37B#", "*01EQ+"), ["*15AQ#~~~37B"]);
        assert_eq!(all("*08AH49#T2", set), ["*08AH#~~~49~T2"]);
        assert_eq!(all("*13AT25=P1S", set), ["*13AT=~~~25~P1S"]);
        // A dot adds the other team's block or dig.
        assert_eq!(all("a06AT43#.4=", "a03ET+"), ["a06AT#~~~43", "*04BT=~~~2"]);
        assert_eq!(all("*11AH25-.9D", set), ["*11AH-~~~25", "a09DH+~~~25"]);
        // Block: assist or attempt, blockers, how it ended.
        assert_eq!(
            all("*14AH27/.8B#A3S", set),
            ["*14AH/~~~27", "a08BH#~~~4~~A3S"]
        );
        assert_eq!(
            all("*14AH27/.8B#T2O", set),
            ["*14AH/~~~27", "a08BH#~~~4~~T2O"]
        );
        assert_eq!(
            all("*14AH27/.8B=2N", set),
            ["*14AH/~~~27", "a08BH=~~~4~~~2N"]
        );
        // Set: call, zone and subzone; the hands; unhittable, net, referee.
        let pass = "*14RH#";
        assert_eq!(all("*09K22B+", pass), ["*09EH+K2~~2B"]);
        assert_eq!(all("*09K22B+1I", pass), ["*09EH+K2~~2B1~I"]);
        assert_eq!(all("*09K7+3U", pass), ["*09EH+K7~~~~3~U"]);
        // Reception: VolleyStation's Z is overhead, written O as its .dvw
        // exports do; on a reception error Z stays DataVolley's referee call.
        let serve = "a07SH-";
        assert_eq!(all("*05RH+Z", serve), ["*05RH+~~~~~~O"]);
        assert_eq!(all("*05RH#R5", serve), ["*05RH#~~~~~~R5"]);
        assert_eq!(all("*05RH=Z", serve), ["*05RH=~~~~~~~~Z"]);
    }

    #[test]
    fn volleystation_order() {
        let d = Defaults {
            combinations: vec![combo("X5", 'T', '4', 'F'), combo("P2", 'O', '3', '-')],
            ..Defaults::default()
        };
        let with = |s: &str, before: Option<&str>| {
            let b = before.map(touch);
            let e = expand(s, b.as_ref(), &d).unwrap_or_else(|e| panic!("{s}: {e}"));
            e.codes[0].to_string()
        };
        let rec = Some("*05RH+");
        assert_eq!(with("9AQ35#", rec), "*09AQ#~~~35", "start and end zone");
        assert_eq!(with("9AQ36A#", rec), "*09AQ#~~~36A", "and subzone");
        assert_eq!(with("8A4", rec), "*08AH+~~~4", "one zone is the start");
        assert_eq!(
            with("8X56#", rec),
            "*08AT#X5~46",
            "combination: start from it"
        );
        assert_eq!(with("8X5#T2", rec), "*08AT#X5~4~~T2", "tip, two blockers");
        assert_eq!(with("8AH48#T3", rec), "*08AH#~~~48~T3");
        assert_eq!(
            with("8AT26=H1O", rec),
            "*08AT=~~~26~H1O",
            "hard, one blocker, out"
        );
        assert_eq!(with("8#T2", rec), "*08AH#~~~~~~T2", "skill predicted");
        assert_eq!(with("8=O", rec), "*08AH=~~~~~~~~O", "out");
        assert_eq!(
            with("8A#P2", rec),
            "*08AO#P2~3",
            "a combination after # is by column"
        );
        assert_eq!(
            with("4K1", rec),
            "*04EH+K1",
            "a call stands in for the skill"
        );
        assert_eq!(with("4K1F#", rec), "*04EH#K1F");
        assert_eq!(
            with("5K12B+", rec),
            "*05EH+K1~~2B",
            "a set's zone is its end zone"
        );
        assert_eq!(with("4E#2", rec), "*04EH#~~~~~~2", "two hands");
        assert_eq!(with("a5S#", None), "a05SH#", "nothing extra");
        assert_eq!(with("5S16-", None), "*05SH-~~~16");
        assert_eq!(
            with("a05AT45#.5=", Some("a04EH+")),
            "a05AT#~~~45",
            "dot code halves"
        );
        let err = |s: &str| expand(s, rec.map(touch).as_ref(), &d).unwrap_err();
        assert_eq!(err("8Z5#"), "Unknown skill letter Z.");
        assert_eq!(err("8A4567#"), "4567: at most two zones, start and end.");
        assert!(err("8A45#~~~45").contains("one or the other"));
    }

    #[test]
    fn a_dot_in_custom_text_is_text() {
        let d = Defaults::default();
        for code in ["*06EQ+K2C~3~2~~25B.+", "a14RQ=~~~31C~~~."] {
            let x = expand(code, None, &d).expect(code);
            assert_eq!(x.codes.len(), 1, "{code}");
            assert_eq!(x.codes[0].to_string(), code);
        }
    }

    /// A combination in a dot code gives the attack its type and start
    /// zone as it does on its own; the block mirrors the zone.
    #[test]
    fn a_dot_code_takes_its_combination() {
        let d = Defaults {
            combinations: vec![combo("X5", 'T', '4', 'F')],
            ..Defaults::default()
        };
        let dot = |token: &str| -> Vec<String> {
            expand(token, None, &d)
                .unwrap_or_else(|e| panic!("{token}: {e}"))
                .codes
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(dot("2X5.a8="), ["*02AT#X5~4", "a08BT=~~~2"]);
        assert_eq!(dot("2AX5.a8D="), ["*02AT#X5~4", "a08DT=~~~4"]);
    }

    #[test]
    fn dot_codes() {
        assert_eq!(
            exp("5S.a12#", None),
            ["*05SH-", "a12RH#"],
            "serve from the reception"
        );
        assert_eq!(
            exp("5SM#.12", None),
            ["*05SM#", "a12RM="],
            "ace: reception error"
        );
        assert_eq!(
            exp("a9SQ~~~16B.4", None),
            ["a09SQ-~~~16B", "*04RQ+~~~16B"],
            "zones shared"
        );
        assert_eq!(
            exp("a5AH~~~4.12#", None),
            ["a05AH/~~~4", "*12BH#~~~2"],
            "stuff block, zone mirrored"
        );
        assert_eq!(exp("5A.a12D#", None), ["*05AH-", "a12DH#"]);
        assert_eq!(exp("5A#.a12D", None), ["*05AH#", "a12DH="]);
        // A serve or attack rated `-`: the default pass or dig (`+`), as
        // VolleyStation fills it, not a perfect one.
        assert_eq!(
            exp("5S-.a12", None),
            ["*05SH-", "a12RH+"],
            "serve - : pass +"
        );
        assert_eq!(
            exp("5A-.a12D", None),
            ["*05AH-", "a12DH+"],
            "attack - : dig +"
        );
        // DataVolley 4 and VolleyStation Pro write the same pair (#22).
        assert_eq!(
            exp("12A.6d=", None),
            ["*12AH#", "a06DH="],
            "dig error: a kill for the attacker"
        );
    }

    fn follow(old: &str, new: &str, partner: &str, edited_first: bool) -> Option<String> {
        follow_pair(&touch(old), &touch(new), &touch(partner), edited_first).map(|t| t.to_string())
    }

    #[test]
    fn a_corrected_serve_keeps_a_pass_that_still_fits() {
        // Serve corrected from + to -: a # pass still fits (- allows + or #),
        // so the pass needn't change.
        assert_eq!(follow("*05SH+", "*05SH-", "a12RH#", true), None);
        // A pass that no longer fits takes the first that does.
        assert_eq!(
            follow("*05SH#", "*05SH-", "a12RH=", true).as_deref(),
            Some("a12RH+")
        );
    }

    #[test]
    fn corrected_pairs_follow() {
        // The reception was really perfect: the serve becomes `-`.
        assert_eq!(
            follow("a12RH+", "a12RH#", "*05SH!", false).as_deref(),
            Some("*05SH-")
        );
        // The serve was an ace: the reception becomes an error.
        assert_eq!(
            follow("*05SH-", "*05SH#", "a12RH#", true).as_deref(),
            Some("a12RH=")
        );
        // A jump serve, not a float: the reception's type follows.
        assert_eq!(
            follow("*05SM-", "*05SQ-", "a12RM#", true).as_deref(),
            Some("a12RQ#")
        );
        // Zones are shared; a block's zone mirrors the attack's.
        assert_eq!(
            follow("*05SH-~~~16", "*05SH-~~~56C", "a12RH#~~~16", true).as_deref(),
            Some("a12RH#~~~56C")
        );
        assert_eq!(
            follow("a5AH/~~~3", "a5AH/~~~4", "*12BH#~~~3", true).as_deref(),
            Some("*12BH#~~~2")
        );
        assert_eq!(
            follow("a5AH-", "a5AH+", "*12DH#", true).as_deref(),
            Some("*12DH-")
        );
        // A change of player only, or a broken pair, changes nothing.
        assert_eq!(follow("*05SH-", "*07SH-", "a12RH#", true), None);
        assert_eq!(follow("*05SH-", "*05AH-", "a12RH#", true), None);
        assert!(is_pair(&touch("*05AH-"), &touch("a12DH#")));
        assert!(!is_pair(&touch("*05AH-"), &touch("*12DH#")));
    }

    #[test]
    fn commands() {
        assert_eq!(exp("T", None), ["*T"]);
        assert_eq!(exp("ac6.7", None), ["ac06:07"]);
        assert_eq!(exp("z3", None), ["*z3"]);
        assert_eq!(exp("aP5", None), ["aP5"]);
        assert_eq!(exp("p5:3", None), ["*p05:03"]);
        assert_eq!(exp("**2set", None), ["**2set"]);
    }

    #[test]
    fn says_what_was_filled() {
        let e = expand("a10", None, &Defaults::default()).expect("expand");
        assert_eq!(
            e.filled,
            ["skill predicted", "type default", "evaluation default"]
        );
        let e = expand("a10RM#", None, &Defaults::default()).expect("expand");
        assert!(e.filled.is_empty());
    }

    #[test]
    fn explains_mistakes() {
        let err = |s: &str| expand(s, None, &Defaults::default()).unwrap_err();
        assert_eq!(
            err("x"),
            "Start with the shirt number (a for the away team)."
        );
        assert_eq!(err("7K"), "Unknown skill letter K.");
        assert_eq!(err("7AXH"), "Unknown type letter X.");
        assert_eq!(
            err("5S.a12A"),
            "Dot codes pair a serve with a reception, or an attack with a block or dig."
        );
        assert_eq!(
            err("5S.*12"),
            "The part after the dot is the other team's touch."
        );
        assert_eq!(err("120S"), "Shirt numbers go from 0 to 99.");
    }
}
