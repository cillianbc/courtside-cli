//! The DataVolley scouting code: one `[3SCOUT]` code string to a [`Code`].
//!
//! Parsing never fails. A string that fits no known shape is
//! [`Code::Unrecognised`]; whether a recognised code is *legal* is the
//! validator's job, not the parser's. Every variant keeps exactly what was
//! written, so `Code::parse(s).to_string() == s` for every `s`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Team {
    /// `*`
    Home,
    /// `a`
    Visiting,
}

impl Team {
    fn from_char(c: char) -> Option<Self> {
        match c {
            '*' => Some(Team::Home),
            'a' => Some(Team::Visiting),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Team::Home => '*',
            Team::Visiting => 'a',
        }
    }
}

/// A run of ASCII digits kept as written, so `01` and `1` round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digits(String);

impl Digits {
    fn take(s: &str) -> Option<(Self, &str)> {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        (end > 0).then(|| (Digits(s[..end].to_owned()), &s[end..]))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `None` only if the number overflows `u32`.
    pub fn value(&self) -> Option<u32> {
        self.0.parse().ok()
    }
}

impl fmt::Display for Digits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Skill {
    Serve,
    Reception,
    Set,
    Attack,
    Block,
    Dig,
    Freeball,
    /// `&`: a "green code" crediting a point without a specific skill,
    /// written with the unknown player as `$$&`.
    Green,
}

impl Skill {
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            'S' => Skill::Serve,
            'R' => Skill::Reception,
            'E' => Skill::Set,
            'A' => Skill::Attack,
            'B' => Skill::Block,
            'D' => Skill::Dig,
            'F' => Skill::Freeball,
            '&' => Skill::Green,
            _ => return None,
        })
    }

    pub fn as_char(self) -> char {
        match self {
            Skill::Serve => 'S',
            Skill::Reception => 'R',
            Skill::Set => 'E',
            Skill::Attack => 'A',
            Skill::Block => 'B',
            Skill::Dig => 'D',
            Skill::Freeball => 'F',
            Skill::Green => '&',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Player {
    Number(Digits),
    /// `$$`
    Unknown,
}

/// Positions after the skill letter, 0-based from the skill type. The
/// PRD's 1-based positions in parentheses.
mod pos {
    pub const SKILL_TYPE: usize = 0; // (5)
    pub const EVALUATION: usize = 1; // (6)
    pub const COMBINATION: usize = 2; // (7–8), two characters
    pub const TARGET: usize = 4; // (9)
    pub const START_ZONE: usize = 5; // (10)
    pub const END_ZONE: usize = 6; // (11)
    pub const END_SUBZONE: usize = 7; // (12)
    pub const SKILL_SUBTYPE: usize = 8; // (13)
    pub const NUM_PLAYERS: usize = 9; // (14)
    pub const SPECIAL: usize = 10; // (15)
    pub const COUNT: usize = 11;
}

/// A skill code such as `a10AH#X5~43C` with everything after the player as
/// written: each fixed position is a character or empty (`~`), and the
/// custom part (position 16 on) is free text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Touch {
    pub team: Team,
    pub player: Player,
    pub skill: Skill,
    slots: [Option<char>; pos::COUNT],
    /// How many fixed positions were written (trailing ones may be `~`).
    written: usize,
    custom: String,
}

impl Touch {
    pub fn new(team: Team, player: Player, skill: Skill) -> Self {
        Touch {
            team,
            player,
            skill,
            slots: [None; pos::COUNT],
            written: 0,
            custom: String::new(),
        }
    }

    pub fn skill_type(&self) -> Option<char> {
        self.slots[pos::SKILL_TYPE]
    }
    pub fn evaluation(&self) -> Option<char> {
        self.slots[pos::EVALUATION]
    }
    /// Attack combination (`X5`) on attacks, setter call (`K1`) on sets.
    /// `None` if both characters are empty; a half-filled pair is returned
    /// with `~` in the empty half.
    pub fn combination(&self) -> Option<String> {
        let (a, b) = (
            self.slots[pos::COMBINATION],
            self.slots[pos::COMBINATION + 1],
        );
        (a.is_some() || b.is_some()).then(|| [a.unwrap_or('~'), b.unwrap_or('~')].iter().collect())
    }
    /// Set target (`F`, `B`, `C`, `P`, `S`).
    pub fn target(&self) -> Option<char> {
        self.slots[pos::TARGET]
    }
    pub fn start_zone(&self) -> Option<char> {
        self.slots[pos::START_ZONE]
    }
    /// End zone, or end cone for attacks in a cone-coded match.
    pub fn end_zone(&self) -> Option<char> {
        self.slots[pos::END_ZONE]
    }
    pub fn end_subzone(&self) -> Option<char> {
        self.slots[pos::END_SUBZONE]
    }
    pub fn skill_subtype(&self) -> Option<char> {
        self.slots[pos::SKILL_SUBTYPE]
    }
    pub fn num_players(&self) -> Option<char> {
        self.slots[pos::NUM_PLAYERS]
    }
    pub fn special(&self) -> Option<char> {
        self.slots[pos::SPECIAL]
    }
    /// Free text from position 16, as written (may contain `~`).
    pub fn custom(&self) -> &str {
        &self.custom
    }

    pub fn with_skill_type(self, c: char) -> Self {
        self.with(pos::SKILL_TYPE, c)
    }
    pub fn with_evaluation(self, c: char) -> Self {
        self.with(pos::EVALUATION, c)
    }
    pub fn with_combination(self, code: &str) -> Self {
        let mut chars = code.chars();
        let t = match chars.next() {
            Some(c) => self.with(pos::COMBINATION, c),
            None => self,
        };
        match chars.next() {
            Some(c) => t.with(pos::COMBINATION + 1, c),
            None => t,
        }
    }
    pub fn with_target(self, c: char) -> Self {
        self.with(pos::TARGET, c)
    }
    pub fn with_start_zone(self, c: char) -> Self {
        self.with(pos::START_ZONE, c)
    }
    pub fn with_end_zone(self, c: char) -> Self {
        self.with(pos::END_ZONE, c)
    }
    pub fn with_end_subzone(self, c: char) -> Self {
        self.with(pos::END_SUBZONE, c)
    }
    pub fn with_skill_subtype(self, c: char) -> Self {
        self.with(pos::SKILL_SUBTYPE, c)
    }
    pub fn with_num_players(self, c: char) -> Self {
        self.with(pos::NUM_PLAYERS, c)
    }
    pub fn with_special(self, c: char) -> Self {
        self.with(pos::SPECIAL, c)
    }
    /// Sets the custom text, padding the fixed positions with `~`.
    pub fn with_custom(mut self, text: &str) -> Self {
        self.custom = text.to_owned();
        if !text.is_empty() {
            self.written = pos::COUNT;
        }
        self
    }

    /// `~` means empty, so it cannot be stored as a value: it empties the
    /// position, and empty positions left at the end aren't written
    /// (unless custom text follows them).
    fn with(mut self, i: usize, c: char) -> Self {
        if c == '~' {
            self.slots[i] = None;
            if self.custom.is_empty() {
                while self.written > 0 && self.slots[self.written - 1].is_none() {
                    self.written -= 1;
                }
            }
            return self;
        }
        self.slots[i] = Some(c);
        self.written = self.written.max(i + 1);
        self
    }

    fn parse_tail(team: Team, player: Player, skill: Skill, rest: &str) -> Self {
        let mut touch = Touch::new(team, player, skill);
        let mut chars = rest.char_indices();
        for slot in &mut touch.slots {
            let Some((_, c)) = chars.next() else { break };
            *slot = (c != '~').then_some(c);
            touch.written += 1;
        }
        touch.custom = chars
            .next()
            .map_or(String::new(), |(i, _)| rest[i..].to_owned());
        touch
    }
}

impl fmt::Display for Touch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.team.as_char())?;
        match &self.player {
            Player::Number(d) => write!(f, "{d}")?,
            Player::Unknown => f.write_str("$$")?,
        }
        write!(f, "{}", self.skill.as_char())?;
        for slot in &self.slots[..self.written] {
            write!(f, "{}", slot.unwrap_or('~'))?;
        }
        f.write_str(&self.custom)
    }
}

/// One `[3SCOUT]` code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Code {
    Touch(Touch),
    /// `*p25:20`: point to `team`, score after it (home:visiting).
    Point {
        team: Team,
        home: Digits,
        visiting: Digits,
        note: String,
    },
    /// `*z1`: `team`'s setter is in position `position`.
    SetterPosition {
        team: Team,
        position: Digits,
        note: String,
    },
    /// `*P08`: `team`'s setter is player `player`. With note `>LUp` it is
    /// part of the starting lineup.
    SetterPlayer {
        team: Team,
        player: Digits,
        note: String,
    },
    /// `*c06:07`: player `out` replaced by player `in`.
    Substitution {
        team: Team,
        out: Digits,
        r#in: Digits,
        note: String,
    },
    /// `*T`
    Timeout {
        team: Team,
        note: String,
    },
    /// `**1set`
    SetEnd {
        set: Digits,
        note: String,
    },
    /// `>CHALLENGE…`: a free-text line DataVolley writes among the codes
    /// (challenges, referee notes), kept verbatim with its `>`.
    Comment(String),
    Unrecognised(String),
}

impl Code {
    pub fn parse(s: &str) -> Self {
        parse(s).unwrap_or_else(|| Code::Unrecognised(s.to_owned()))
    }

    pub fn team(&self) -> Option<Team> {
        match self {
            Code::Touch(t) => Some(t.team),
            Code::Point { team, .. }
            | Code::SetterPosition { team, .. }
            | Code::SetterPlayer { team, .. }
            | Code::Substitution { team, .. }
            | Code::Timeout { team, .. } => Some(*team),
            Code::SetEnd { .. } | Code::Comment(_) | Code::Unrecognised(_) => None,
        }
    }
}

/// Trailing text after a non-skill code, kept verbatim. Only `>...`
/// (e.g. `>LUp`) is accepted; anything else makes the code unrecognised.
fn note(rest: &str) -> Option<String> {
    (rest.is_empty() || rest.starts_with('>')).then(|| rest.to_owned())
}

fn parse(s: &str) -> Option<Code> {
    if s.starts_with('>') {
        return Some(Code::Comment(s.to_owned()));
    }
    if let Some(rest) = s.strip_prefix("**") {
        let (set, rest) = Digits::take(rest)?;
        let rest = rest.strip_prefix("set")?;
        return Some(Code::SetEnd {
            set,
            note: note(rest)?,
        });
    }
    let mut chars = s.chars();
    let team = Team::from_char(chars.next()?)?;
    let rest = chars.as_str();

    if let Some(rest) = rest.strip_prefix("$$") {
        let mut chars = rest.chars();
        let skill = Skill::from_char(chars.next()?)?;
        return Some(Code::Touch(Touch::parse_tail(
            team,
            Player::Unknown,
            skill,
            chars.as_str(),
        )));
    }
    if let Some((player, rest)) = Digits::take(rest) {
        let mut chars = rest.chars();
        let skill = Skill::from_char(chars.next()?).filter(|s| *s != Skill::Green)?;
        return Some(Code::Touch(Touch::parse_tail(
            team,
            Player::Number(player),
            skill,
            chars.as_str(),
        )));
    }

    let mut chars = rest.chars();
    let kind = chars.next()?;
    let rest = chars.as_str();
    let pair = |rest| -> Option<(Digits, Digits, &str)> {
        let (a, rest) = Digits::take(rest)?;
        let (b, rest) = Digits::take(rest.strip_prefix(':')?)?;
        Some((a, b, rest))
    };
    Some(match kind {
        'p' => {
            let (home, visiting, rest) = pair(rest)?;
            Code::Point {
                team,
                home,
                visiting,
                note: note(rest)?,
            }
        }
        'c' => {
            let (out, r#in, rest) = pair(rest)?;
            Code::Substitution {
                team,
                out,
                r#in,
                note: note(rest)?,
            }
        }
        'z' => {
            let (position, rest) = Digits::take(rest)?;
            Code::SetterPosition {
                team,
                position,
                note: note(rest)?,
            }
        }
        'P' => {
            let (player, rest) = Digits::take(rest)?;
            Code::SetterPlayer {
                team,
                player,
                note: note(rest)?,
            }
        }
        'T' => Code::Timeout {
            team,
            note: note(rest)?,
        },
        _ => return None,
    })
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Code::Touch(t) => write!(f, "{t}"),
            Code::Point {
                team,
                home,
                visiting,
                note,
            } => write!(f, "{}p{home}:{visiting}{note}", team.as_char()),
            Code::SetterPosition {
                team,
                position,
                note,
            } => write!(f, "{}z{position}{note}", team.as_char()),
            Code::SetterPlayer { team, player, note } => {
                write!(f, "{}P{player}{note}", team.as_char())
            }
            Code::Substitution {
                team,
                out,
                r#in,
                note,
            } => write!(f, "{}c{out}:{}{note}", team.as_char(), r#in),
            Code::Timeout { team, note } => write!(f, "{}T{note}", team.as_char()),
            Code::SetEnd { set, note } => write!(f, "**{set}set{note}"),
            Code::Comment(s) | Code::Unrecognised(s) => f.write_str(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_text_lines_are_comments() {
        for line in [">CHALLENGE_HOME", ">REFEREE ERROR", ">ROT<"] {
            let code = Code::parse(line);
            assert_eq!(code, Code::Comment(line.to_owned()));
            assert_eq!(code.to_string(), line);
            assert_eq!(code.team(), None);
        }
    }
    use proptest::prelude::*;

    fn touch(s: &str) -> Touch {
        match Code::parse(s) {
            Code::Touch(t) => t,
            other => panic!("{s:?} parsed as {other:?}"),
        }
    }

    #[test]
    fn simple_touch() {
        let t = touch("a10AH#");
        assert_eq!(t.team, Team::Visiting);
        assert_eq!(t.player, Player::Number(Digits("10".into())));
        assert_eq!(t.skill, Skill::Attack);
        assert_eq!(t.skill_type(), Some('H'));
        assert_eq!(t.evaluation(), Some('#'));
        assert_eq!(t.combination(), None);
        assert_eq!(t.custom(), "");
    }

    #[test]
    fn full_touch() {
        let t = touch("*19AT+X5F43CH2Sxyz~");
        assert_eq!(t.combination().as_deref(), Some("X5"));
        assert_eq!(t.target(), Some('F'));
        assert_eq!(t.start_zone(), Some('4'));
        assert_eq!(t.end_zone(), Some('3'));
        assert_eq!(t.end_subzone(), Some('C'));
        assert_eq!(t.skill_subtype(), Some('H'));
        assert_eq!(t.num_players(), Some('2'));
        assert_eq!(t.special(), Some('S'));
        assert_eq!(t.custom(), "xyz~");
    }

    #[test]
    fn tildes_are_empty_but_kept() {
        let t = touch("*13AH-~~~~~~H2");
        assert_eq!(t.combination(), None);
        assert_eq!(t.start_zone(), None);
        assert_eq!(t.skill_subtype(), Some('H'));
        assert_eq!(t.num_players(), Some('2'));
        assert_eq!(t.to_string(), "*13AH-~~~~~~H2");
        assert_eq!(touch("*19SM+~~~").to_string(), "*19SM+~~~");
    }

    #[test]
    fn unknown_player_and_green_code() {
        let t = touch("*$$&H#");
        assert_eq!(t.player, Player::Unknown);
        assert_eq!(t.skill, Skill::Green);
        assert_eq!(touch("a$$BH=").skill, Skill::Block);
        // Green codes only with the unknown player.
        assert!(matches!(Code::parse("*13&H#"), Code::Unrecognised(_)));
    }

    #[test]
    fn non_skill_codes() {
        assert_eq!(
            Code::parse("*p25:20"),
            Code::Point {
                team: Team::Home,
                home: Digits("25".into()),
                visiting: Digits("20".into()),
                note: String::new()
            }
        );
        assert!(
            matches!(Code::parse("az2>LUp"), Code::SetterPosition { ref note, .. } if note == ">LUp")
        );
        assert!(matches!(Code::parse("*P08>LUp"), Code::SetterPlayer { .. }));
        assert!(
            matches!(Code::parse("ac06:07"), Code::Substitution { ref out, .. } if out.value() == Some(6))
        );
        assert!(matches!(
            Code::parse("*T"),
            Code::Timeout {
                team: Team::Home,
                ..
            }
        ));
        assert!(
            matches!(Code::parse("**3set"), Code::SetEnd { ref set, .. } if set.value() == Some(3))
        );
    }

    #[test]
    fn unrecognised() {
        for s in [
            "", "x10AH#", "*", "*10", "*10X", "*p25", "*p25:", "**set", "*Tx", "*z",
        ] {
            assert_eq!(Code::parse(s), Code::Unrecognised(s.to_owned()), "{s:?}");
        }
    }

    #[test]
    fn builder_matches_parser() {
        let built = Touch::new(Team::Home, Player::Number(Digits("07".into())), Skill::Set)
            .with_skill_type('Q')
            .with_evaluation('#')
            .with_combination("K1")
            .with_target('C');
        assert_eq!(built.to_string(), "*07EQ#K1C");
        assert_eq!(Code::parse("*07EQ#K1C"), Code::Touch(built));
        let custom = Touch::new(Team::Visiting, Player::Unknown, Skill::Serve).with_custom("note");
        assert_eq!(custom.to_string(), "a$$S~~~~~~~~~~~note");
    }

    fn slot() -> impl Strategy<Value = char> {
        prop_oneof![Just('~'), proptest::char::range('!', '}')]
    }

    /// Any well-formed skill code: team, player, skill, 0–11 positions,
    /// optional custom text.
    fn skill_code() -> impl Strategy<Value = String> {
        (
            prop_oneof![Just('*'), Just('a')],
            prop_oneof!["[0-9]{1,2}", Just("$$".to_owned())],
            proptest::sample::select(vec!['S', 'R', 'E', 'A', 'B', 'D', 'F']),
            proptest::collection::vec(slot(), 0..=11),
            "[ -~]{0,8}",
        )
            .prop_map(|(team, player, skill, slots, custom)| {
                let mut s = format!("{team}{player}{skill}");
                s.extend(slots.iter());
                if slots.len() == 11 {
                    s.push_str(&custom);
                }
                s
            })
    }

    fn other_code() -> impl Strategy<Value = String> {
        let team = prop_oneof![Just('*'), Just('a')];
        let note = prop_oneof![Just(String::new()), Just(">LUp".to_owned()), ">[ -~]{0,6}"];
        (team, "[0-9]{1,2}", "[0-9]{1,2}", note, 0..6usize).prop_map(
            |(t, a, b, n, kind)| match kind {
                0 => format!("{t}p{a}:{b}{n}"),
                1 => format!("{t}c{a}:{b}{n}"),
                2 => format!("{t}z{a}{n}"),
                3 => format!("{t}P{a}{n}"),
                4 => format!("{t}T{n}"),
                _ => format!("**{a}set{n}"),
            },
        )
    }

    proptest! {
        #[test]
        fn skill_codes_parse_and_round_trip(s in skill_code()) {
            let code = Code::parse(&s);
            prop_assert!(matches!(code, Code::Touch(_)), "{:?}", code);
            prop_assert_eq!(code.to_string(), s);
        }

        #[test]
        fn other_codes_parse_and_round_trip(s in other_code()) {
            let code = Code::parse(&s);
            prop_assert!(!matches!(code, Code::Touch(_) | Code::Unrecognised(_)), "{:?}", code);
            prop_assert_eq!(code.to_string(), s);
        }

        #[test]
        fn any_string_round_trips(s in "\\PC{0,30}") {
            prop_assert_eq!(Code::parse(&s).to_string(), s);
        }
    }
}
