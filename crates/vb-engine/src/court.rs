//! Court positions as `.dvw` writes them (`[3SCOUT]` columns 4–6): an
//! index into a 100 × 101 grid over the court and a margin around it,
//! `x + (y − 1) · 100`. Positions are in court units as openvolley gives
//! them: the court is x 0.5–3.5 (9 m) by y 0.5–6.5 (18 m), net at y 3.5.
//!
//! The frame is the play's own (checked on the corpus, FORMAT-NOTES): the
//! team making the play has its half at the bottom (y < 3.5) and faces up,
//! so its zone 4 is on the left (small x). A reception carries the serve's
//! position and a dig the attack's, in the server's and attacker's frame.

/// Columns of the grid (x).
const COLUMNS: u16 = 100;
/// Rows of the grid (y).
const ROWS: u16 = 101;

/// The centre of grid cell `index`, or `None` outside the grid.
pub fn position(index: u16) -> Option<(f64, f64)> {
    if !(1..=COLUMNS * ROWS).contains(&index) {
        return None;
    }
    let xi = f64::from((index - 1) % COLUMNS + 1);
    let yi = f64::from((index - 1) / COLUMNS + 1);
    Some((
        0.5 + (xi - 11.0) / 80.0 * 3.0 + 3.0 / 160.0,
        0.5 + (yi - 11.0) / 81.0 * 6.0 + 3.0 / 81.0,
    ))
}

/// The grid cell a position falls in; positions past the margin go to
/// the edge cell.
pub fn index(x: f64, y: f64) -> u16 {
    let bin = |v: f64, cells: f64, per: f64, top: u16| -> u16 {
        let i = ((v - 0.5) * cells / per).floor() + 11.0;
        // Clamped to the grid, so the cast can't truncate.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = i.clamp(1.0, f64::from(top)) as u16;
        i
    };
    let xi = bin(x, 80.0, 3.0, COLUMNS);
    let yi = bin(y, 81.0, 6.0, ROWS);
    xi + (yi - 1) * COLUMNS
}

/// Whether a position is on the court, lines included.
pub fn on_court(x: f64, y: f64) -> bool {
    (0.5..=3.5).contains(&x) && (0.5..=6.5).contains(&y)
}

/// Zones 1–9 by row (back, middle, front of the bottom half, then the
/// top half from the net) and column (small x first), as openvolley's
/// `dv_xy2zone`: each half's zones as its own team sees them.
const ZONES: [[u8; 3]; 6] = [
    [5, 6, 1],
    [7, 8, 9],
    [4, 3, 2],
    [2, 3, 4],
    [9, 8, 7],
    [1, 6, 5],
];

/// The zone a position is in; off the court, the nearest zone.
pub fn zone(x: f64, y: f64) -> u8 {
    // Rounded and clamped to 1–3 and 1–6, so the casts can't truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let at = |v: f64, top: f64| v.round_ties_even().clamp(1.0, top) as usize;
    ZONES[at(y, 6.0) - 1][at(x, 3.0) - 1]
}

/// The subzone (`A`–`D`) of the zone a position is in, as openvolley's
/// `dv_xy2subzone`: each zone in quarters, as the half's team sees it, `C`
/// and `B` on the net side (left, right), `D` and `A` behind them.
pub fn subzone(x: f64, y: f64) -> char {
    // Half-units of the court (1.5 m), clamped, so the casts can't truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let at = |v: f64, top: f64| ((v - 0.5) / 0.5).ceil().clamp(1.0, top) as usize;
    let (xi, yi) = (at(x, 6.0), at(y, 12.0));
    let left = xi % 2 == 1;
    let net_side = yi % 2 == 0;
    match (yi <= 6, net_side, left) {
        (true, true, true) => 'C',
        (true, true, false) => 'B',
        (true, false, true) => 'D',
        (true, false, false) => 'A',
        // The top half's team faces the other way: its left is our right.
        (false, false, true) => 'B',
        (false, false, false) => 'C',
        (false, true, true) => 'A',
        (false, true, false) => 'D',
    }
}

/// The serve zone (`5 7 6 9 1` across the end line, as the server's team
/// sees it) a position behind the court is in, as openvolley's
/// `dv_xy2zone(as_for_serve = TRUE)`.
pub fn serve_zone(x: f64, y: f64) -> u8 {
    const ZONES: [u8; 5] = [5, 7, 6, 9, 1];
    let bin = [1.1, 1.7, 2.3, 2.9].iter().take_while(|b| x > **b).count();
    if y > 3.5 { ZONES[4 - bin] } else { ZONES[bin] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from openvolley's `dv_index2xy`.
    #[test]
    fn grid_cells_match_openvolley() {
        let near =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        assert!(near(position(1).expect("1"), (0.14375, -0.203_703_7)));
        assert!(near(position(100).expect("100"), (3.85625, -0.203_703_7)));
        assert!(near(position(101).expect("101"), (0.14375, -0.129_629_6)));
        assert!(near(position(4868).expect("4868"), (2.65625, 3.351_851_9)));
        assert!(near(
            position(10100).expect("10100"),
            (3.85625, 7.203_703_7)
        ));
        assert_eq!(position(0), None);
        assert_eq!(position(10101), None);
    }

    #[test]
    fn every_cell_centre_maps_back_to_its_cell() {
        for i in 1..=10100 {
            let (x, y) = position(i).expect("cell");
            assert_eq!(index(x, y), i);
        }
        assert_eq!(index(-5.0, -5.0), 1);
        assert_eq!(index(9.0, 9.0), 10100);
    }

    /// A grid of positions read by openvolley: (x, y, zone, subzone,
    /// serve zone).
    #[test]
    fn zones_match_openvolley() {
        let cases: &[(f64, f64, u8, char, u8)] = &[
            (0.6, 0.6, 5, 'D', 5),
            (1.2, 0.6, 5, 'A', 7),
            (1.4, 0.6, 5, 'A', 7),
            (1.6, 0.6, 6, 'D', 7),
            (2.2, 0.6, 6, 'A', 6),
            (2.6, 0.6, 1, 'D', 9),
            (3.0, 0.6, 1, 'D', 1),
            (3.4, 0.6, 1, 'A', 1),
            (0.6, 1.1, 5, 'C', 5),
            (1.2, 1.1, 5, 'B', 7),
            (1.4, 1.1, 5, 'B', 7),
            (1.6, 1.1, 6, 'C', 7),
            (2.2, 1.1, 6, 'B', 6),
            (2.6, 1.1, 1, 'C', 9),
            (3.0, 1.1, 1, 'C', 1),
            (3.4, 1.1, 1, 'B', 1),
            (0.6, 1.6, 7, 'D', 5),
            (1.2, 1.6, 7, 'A', 7),
            (1.4, 1.6, 7, 'A', 7),
            (1.6, 1.6, 8, 'D', 7),
            (2.2, 1.6, 8, 'A', 6),
            (2.6, 1.6, 9, 'D', 9),
            (3.0, 1.6, 9, 'D', 1),
            (3.4, 1.6, 9, 'A', 1),
            (0.6, 2.1, 7, 'C', 5),
            (1.2, 2.1, 7, 'B', 7),
            (1.4, 2.1, 7, 'B', 7),
            (1.6, 2.1, 8, 'C', 7),
            (2.2, 2.1, 8, 'B', 6),
            (2.6, 2.1, 9, 'C', 9),
            (3.0, 2.1, 9, 'C', 1),
            (3.4, 2.1, 9, 'B', 1),
            (0.6, 2.7, 4, 'D', 5),
            (1.2, 2.7, 4, 'A', 7),
            (1.4, 2.7, 4, 'A', 7),
            (1.6, 2.7, 3, 'D', 7),
            (2.2, 2.7, 3, 'A', 6),
            (2.6, 2.7, 2, 'D', 9),
            (3.0, 2.7, 2, 'D', 1),
            (3.4, 2.7, 2, 'A', 1),
            (0.6, 3.2, 4, 'C', 5),
            (1.2, 3.2, 4, 'B', 7),
            (1.4, 3.2, 4, 'B', 7),
            (1.6, 3.2, 3, 'C', 7),
            (2.2, 3.2, 3, 'B', 6),
            (2.6, 3.2, 2, 'C', 9),
            (3.0, 3.2, 2, 'C', 1),
            (3.4, 3.2, 2, 'B', 1),
            (0.6, 3.6, 2, 'B', 1),
            (1.2, 3.6, 2, 'C', 9),
            (1.4, 3.6, 2, 'C', 9),
            (1.6, 3.6, 3, 'B', 9),
            (2.2, 3.6, 3, 'C', 6),
            (2.6, 3.6, 4, 'B', 7),
            (3.0, 3.6, 4, 'B', 5),
            (3.4, 3.6, 4, 'C', 5),
            (0.6, 4.2, 2, 'A', 1),
            (1.2, 4.2, 2, 'D', 9),
            (1.4, 4.2, 2, 'D', 9),
            (1.6, 4.2, 3, 'A', 9),
            (2.2, 4.2, 3, 'D', 6),
            (2.6, 4.2, 4, 'A', 7),
            (3.0, 4.2, 4, 'A', 5),
            (3.4, 4.2, 4, 'D', 5),
            (0.6, 4.8, 9, 'B', 1),
            (1.2, 4.8, 9, 'C', 9),
            (1.4, 4.8, 9, 'C', 9),
            (1.6, 4.8, 8, 'B', 9),
            (2.2, 4.8, 8, 'C', 6),
            (2.6, 4.8, 7, 'B', 7),
            (3.0, 4.8, 7, 'B', 5),
            (3.4, 4.8, 7, 'C', 5),
            (0.6, 5.3, 9, 'A', 1),
            (1.2, 5.3, 9, 'D', 9),
            (1.4, 5.3, 9, 'D', 9),
            (1.6, 5.3, 8, 'A', 9),
            (2.2, 5.3, 8, 'D', 6),
            (2.6, 5.3, 7, 'A', 7),
            (3.0, 5.3, 7, 'A', 5),
            (3.4, 5.3, 7, 'D', 5),
            (0.6, 5.9, 1, 'B', 1),
            (1.2, 5.9, 1, 'C', 9),
            (1.4, 5.9, 1, 'C', 9),
            (1.6, 5.9, 6, 'B', 9),
            (2.2, 5.9, 6, 'C', 6),
            (2.6, 5.9, 5, 'B', 7),
            (3.0, 5.9, 5, 'B', 5),
            (3.4, 5.9, 5, 'C', 5),
            (0.6, 6.4, 1, 'A', 1),
            (1.2, 6.4, 1, 'D', 9),
            (1.4, 6.4, 1, 'D', 9),
            (1.6, 6.4, 6, 'A', 9),
            (2.2, 6.4, 6, 'D', 6),
            (2.6, 6.4, 5, 'A', 7),
            (3.0, 6.4, 5, 'A', 5),
            (3.4, 6.4, 5, 'D', 5),
        ];
        for &(x, y, z, sz, serve) in cases {
            assert_eq!(zone(x, y), z, "zone at {x}, {y}");
            assert_eq!(subzone(x, y), sz, "subzone at {x}, {y}");
            assert_eq!(serve_zone(x, y), serve, "serve zone at {x}, {y}");
        }
        assert!(on_court(0.5, 6.5) && !on_court(0.49, 3.0) && !on_court(2.0, 6.6));
    }
}
