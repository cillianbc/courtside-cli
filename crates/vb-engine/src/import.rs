//! `.dvw` to [`Match`]: one event per `[3SCOUT]` line, in file order.

use dvw_codec::Dvw;
use dvw_codec::dvw::{Player, Regulation as DvwRegulation, Role};
use vb_grammar::{Code, Skill};

use crate::event::{ClockTime, IdGen, ImportFormat, NewEvent, Points, Producer};
use crate::model::{
    Conventions, Match, Regulation, RosterPlayer, Sides, StartingZone, TeamSheet, WinningSymbols,
};

impl Match {
    /// Every `[3SCOUT]` line becomes one event (an empty line becomes an
    /// unrecognised empty code), so event `seq` equals the line index and
    /// `source_line` points back at it. Players without a readable shirt
    /// number are left off the team sheet.
    pub fn from_dvw(dvw: &Dvw, ids: &mut IdGen) -> Self {
        let volleystation = dvw.header.get("GENERATOR-PRG") == Some("VolleyStation");
        let mut m = Match {
            teams: Sides {
                home: sheet(&dvw.players_home, volleystation),
                visiting: sheet(&dvw.players_visiting, volleystation),
            },
            regulation: match dvw.match_info.regulation() {
                Some(DvwRegulation::IndoorSideout) => Regulation::IndoorSideout,
                Some(DvwRegulation::BeachRallyPoint) => Regulation::Beach,
                Some(DvwRegulation::IndoorRallyPoint) | None => Regulation::IndoorRallyPoint,
            },
            winning_symbols: winning_symbols(dvw),
            conventions: Conventions {
                setter_change_follows_rotation: volleystation,
            },
            ..Match::default()
        };
        for (i, line) in dvw.scout.iter().enumerate() {
            let new = NewEvent {
                video_ms: line
                    .video_time()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .map(|s| (s * 1000.0).round() as u64),
                clock: line.time_hms().map(|(hour, minute, second)| ClockTime {
                    hour,
                    minute,
                    second,
                }),
                ..NewEvent::code(
                    Code::parse(line.code()),
                    Producer::Import(ImportFormat::Dvw),
                )
            };
            let id = m.log.push(ids, new, None, None, Some(i));
            let points = Points::of_line(line);
            if !points.is_empty() {
                // The event was just pushed, so it is there.
                let _ = m.log.set_points(id, points);
            }
        }
        m
    }
}

/// Liberos: `L` in the special-role column; DataVolley also accepts role
/// 1, VolleyStation only reads `L`.
fn sheet(players: &[Player], volleystation: bool) -> TeamSheet {
    TeamSheet {
        players: players
            .iter()
            .filter_map(|p| {
                Some(RosterPlayer {
                    number: p.number()?,
                    starting_zones: std::array::from_fn(|i| {
                        match p.starting_zone(i + 1).map(str::trim) {
                            Some("*") => StartingZone::Substitute,
                            Some(z) => z
                                .parse::<u8>()
                                .ok()
                                .filter(|z| (1..=6).contains(z))
                                .map_or(StartingZone::None, StartingZone::Zone),
                            None => StartingZone::None,
                        }
                    }),
                    libero: p.special_role().trim() == "L"
                        || (!volleystation && p.role() == Some(Role::Libero)),
                    role: p.role().map(|r| match r {
                        Role::Libero => 1,
                        Role::Outside => 2,
                        Role::Opposite => 3,
                        Role::Middle => 4,
                        Role::Setter => 5,
                        Role::Unknown => 6,
                    }),
                })
            })
            .collect(),
    }
}

/// The file's `[3WINNINGSYMBOLS]`, or DataVolley's defaults if it has
/// none or it can't be read.
fn winning_symbols(dvw: &Dvw) -> WinningSymbols {
    let skills = [
        Skill::Serve,
        Skill::Reception,
        Skill::Attack,
        Skill::Block,
        Skill::Dig,
        Skill::Set,
        Skill::Freeball,
    ];
    let table: Option<Vec<_>> = skills
        .iter()
        .map(|s| {
            let (lose, win) = dvw.winning_symbols.for_skill(s.as_char())?;
            Some((*s, lose, win))
        })
        .collect();
    table.map_or_else(WinningSymbols::default, WinningSymbols::new)
}
