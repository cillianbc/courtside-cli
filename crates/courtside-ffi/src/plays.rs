//! `.dvw` to JSON (PRD I-08): one record per `[3SCOUT]` line, field names
//! as in openvolley's `plays` data frame so R and Python users need no
//! adapter. Where openvolley decodes a position to text (skill type,
//! subtype, number of players, special code), we give the raw character as
//! `<name>_raw` instead, so a field never means something different from
//! the openvolley column of the same name. Derived columns (score, set
//! number, setter positions) arrive with vb-engine.

use dvw_codec::Dvw;
use dvw_codec::scout::{AttackPhase, PointPhase, ScoutLine};
use serde_json::{Map, Value, json};
use vb_grammar::{Code, Dialect, EvaluationTable, Player, Skill, Team};

pub fn to_json(dvw: &Dvw) -> Value {
    let evaluations = EvaluationTable::new(Dialect::Standard);
    let first_line = dvw.raw().header_line("3SCOUT").map_or(0, |l| l + 1);
    let plays: Vec<Value> = dvw
        .scout
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.code().is_empty())
        .map(|(i, line)| play(dvw, line, first_line + i, &evaluations))
        .collect();
    let teams: Vec<Value> = dvw
        .teams
        .iter()
        .zip(["*", "a"])
        .map(|(t, code)| {
            json!({
                "team_id": t.id(),
                "team": t.name(),
                "home_away_team": code,
                "sets_won": t.sets_won(),
                "coach": t.coach(),
                "assistant": t.assistant(),
            })
        })
        .collect();
    json!({
        "format": "courtside-plays",
        "version": 1,
        "evaluation_dialect": "standard",
        "match": {
            "date": dvw.match_date().map(|(d, _)| d.to_string()),
            "time": hms(dvw.match_info.time()),
            "season": dvw.match_info.season(),
            "league": dvw.match_info.league(),
            "phase": dvw.match_info.phase(),
            "text_encoding": dvw.encoding.name(),
        },
        "teams": teams,
        "plays": plays,
    })
}

/// `15.30.00` or `15:30:00` as `15:30:00`; anything else as written.
fn hms(time: &str) -> String {
    let parts: Vec<&str> = time.trim().split(['.', ':']).collect();
    let two_digits = |p: &&str| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit());
    match parts[..] {
        [h, m, s] if parts.iter().all(two_digits) => format!("{h}:{m}:{s}"),
        _ => time.to_owned(),
    }
}

fn opt<T: Into<Value>>(v: Option<T>) -> Value {
    v.map_or(Value::Null, Into::into)
}

fn text(c: Option<char>) -> Value {
    opt(c.map(String::from))
}

fn play(dvw: &Dvw, line: &ScoutLine, file_line: usize, evaluations: &EvaluationTable) -> Value {
    let code = Code::parse(line.code());
    let mut m = Map::new();
    let mut put = |k: &str, v: Value| {
        m.insert(k.to_owned(), v);
    };
    put("file_line_number", json!(file_line));
    put("code", json!(line.code()));
    let team_name = |t: Team| {
        let i = usize::from(t == Team::Visiting);
        dvw.teams.get(i).map(|t| t.name())
    };
    put("team", opt(code.team().and_then(team_name)));

    let touch = match &code {
        Code::Touch(t) if t.skill != Skill::Green => Some(t),
        _ => None,
    };
    let number = touch.and_then(|t| match &t.player {
        Player::Number(d) => d.value().and_then(|v| u8::try_from(v).ok()),
        Player::Unknown => None,
    });
    let roster = touch
        .zip(number)
        .and_then(|(t, n)| dvw.player(t.team == Team::Home, n));
    put("player_number", opt(number));
    put(
        "player_name",
        // Each part trimmed, as openvolley does.
        opt(roster.map(|p| {
            format!("{} {}", p.first_name().trim(), p.last_name().trim())
                .trim()
                .to_owned()
        })),
    );
    put("player_id", opt(roster.map(|p| p.id().to_owned())));
    put("skill", opt(touch.map(|t| skill_name(t.skill))));
    put("skill_type_raw", text(touch.and_then(|t| t.skill_type())));
    put("evaluation_code", text(touch.and_then(|t| t.evaluation())));
    put(
        "evaluation",
        opt(touch.and_then(|t| {
            evaluations
                .meaning(t.skill, t.evaluation()?)
                .map(str::to_owned)
        })),
    );
    let only = |skill: Skill| touch.filter(|t| t.skill == skill);
    put(
        "attack_code",
        opt(only(Skill::Attack).and_then(|t| t.combination())),
    );
    put(
        "set_code",
        opt(only(Skill::Set).and_then(|t| t.combination())),
    );
    put("set_type", text(only(Skill::Set).and_then(|t| t.target())));
    let digit = |c: Option<char>| opt(c.and_then(|c| c.to_digit(10)));
    put("start_zone", digit(touch.and_then(|t| t.start_zone())));
    put("end_zone", digit(touch.and_then(|t| t.end_zone())));
    put("end_subzone", text(touch.and_then(|t| t.end_subzone())));
    put(
        "skill_subtype_raw",
        text(touch.and_then(|t| t.skill_subtype())),
    );
    put("num_players_raw", text(touch.and_then(|t| t.num_players())));
    put("special_raw", text(touch.and_then(|t| t.special())));
    put(
        "custom_code",
        opt(touch.map(|t| t.custom()).filter(|c| !c.is_empty())),
    );
    put("timeout", json!(matches!(code, Code::Timeout { .. })));
    put(
        "substitution",
        json!(match &code {
            Code::Substitution { .. } => true,
            Code::SetterPlayer { note, .. } => !note.to_ascii_lowercase().contains(">lup"),
            _ => false,
        }),
    );
    put("point", json!(matches!(code, Code::Point { .. })));
    put("end_of_set", json!(matches!(code, Code::SetEnd { .. })));
    put(
        "point_phase",
        opt(line.point_phase().map(|p| match p {
            PointPhase::Breakpoint => "Breakpoint",
            PointPhase::Sideout => "Sideout",
        })),
    );
    put(
        "attack_phase",
        opt(line.attack_phase().map(|p| match p {
            AttackPhase::Reception => "Reception",
            AttackPhase::TransitionSideout => "Transition sideout",
            AttackPhase::TransitionBreakpoint => "Transition breakpoint",
        })),
    );
    put("start_coordinate", opt(line.start_coordinate()));
    put("mid_coordinate", opt(line.mid_coordinate()));
    put("end_coordinate", opt(line.end_coordinate()));
    put(
        "time",
        opt(line
            .time_hms()
            .map(|(h, m, s)| format!("{h:02}:{m:02}:{s:02}"))),
    );
    put("video_file_number", opt(line.video_file_number()));
    put("video_time", opt(line.video_time()));
    for (i, p) in line.home_players().into_iter().enumerate() {
        put(&format!("home_p{}", i + 1), opt(p));
    }
    for (i, p) in line.visiting_players().into_iter().enumerate() {
        put(&format!("visiting_p{}", i + 1), opt(p));
    }
    Value::Object(m)
}

fn skill_name(skill: Skill) -> &'static str {
    match skill {
        Skill::Serve => "Serve",
        Skill::Reception => "Reception",
        Skill::Set => "Set",
        Skill::Attack => "Attack",
        Skill::Block => "Block",
        Skill::Dig => "Dig",
        Skill::Freeball => "Freeball",
        Skill::Green => "Green code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_time() {
        assert_eq!(hms("15.30.00"), "15:30:00");
        assert_eq!(hms("15:30:00"), "15:30:00");
        assert_eq!(hms(""), "");
        assert_eq!(hms("3pm"), "3pm");
    }
}
