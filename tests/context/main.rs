//! Integration tests for the blind context logger (naryad Н1-07).
//!
//! Acceptance: a test proves the context is unreachable before the
//! response is logged; the Moon phase is checked by a second method
//! against an independent source on 12 dates with divergence under one
//! day. The independent source: the 2024 new-moon instants, two of which
//! are solar eclipses (Apr 8 total, Oct 2 annular) — astronomically
//! verified new moons.

use kamerton::context::{self, DayWindow};
use kamerton::store::Store;

/// 2024 new moons (UTC instants) — the independent reference list. Two of
/// them are solar eclipses (Apr 8 total, Oct 2 annular): astronomically
/// verified new moons. Unix seconds are derived from the almanac date and
/// time through `unix_from_iso`, whose calendar math is verified against
/// known epoch anchors separately.
const NEW_MOONS_2024: [(&str, &str); 12] = [
    ("2024-01-11", "11:57"),
    ("2024-02-09", "22:59"),
    ("2024-03-10", "09:00"),
    ("2024-04-08", "18:21"), // total solar eclipse
    ("2024-05-08", "03:22"),
    ("2024-06-06", "12:38"),
    ("2024-07-05", "23:57"),
    ("2024-08-04", "11:13"),
    ("2024-09-03", "01:56"),
    ("2024-10-02", "18:49"), // annular solar eclipse
    ("2024-11-01", "12:47"),
    ("2024-12-01", "06:21"),
];

fn almanac_unix(date: &str, time: &str) -> i64 {
    let iso = format!("{date}T{time}:00Z");
    kamerton::store::time::unix_from_iso(&iso).unwrap_or_else(|| panic!("bad almanac date: {iso}"))
}

#[test]
fn moon_phase_matches_independent_source_on_12_dates() {
    for (date, time) in NEW_MOONS_2024 {
        let unix = almanac_unix(date, time);
        let phase = context::moon_phase(unix);
        let distance_days = phase.min(1.0 - phase) * 29.530_588_853;
        assert!(
            distance_days < 1.0,
            "new moon {date} {time}: computed phase {phase} is {distance_days:.2} days off, must be < 1 day"
        );
        // Within one day of the new moon the synodic-calendar day is 30 or
        // 1; the mean-synodic formula deviates from true lunations by up
        // to ~14 hours, so 29 / 2 also satisfy the < 1 day requirement.
        assert!(
            matches!(context::moon_day(unix), 29 | 30 | 1 | 2),
            "new moon {date} must fall on moon day 29-30 or 1-2"
        );
    }
}

#[test]
fn moon_phase_advances_by_synodic_rate() {
    // One day later the phase must move by ~1/29.53 of the cycle.
    let t = almanac_unix("2024-04-08", "18:21"); // the eclipse new moon
    let d = (context::moon_phase(t + 86_400) + 1.0 - context::moon_phase(t)).rem_euclid(1.0);
    assert!((d - 1.0 / 29.530_588_853).abs() < 1e-6);
    // Half a synodic month after a new moon: full moon.
    let half = (context::SYNODIC_SECONDS / 2.0) as i64;
    let full_phase = context::moon_phase(t + half);
    assert!(
        (full_phase - 0.5).abs() < 0.02,
        "half cycle after new moon must be near 0.5, got {full_phase}"
    );
}

#[test]
fn local_hour_maps_to_day_window() {
    assert_eq!(DayWindow::from_local_hour(7), DayWindow::Morning);
    assert_eq!(DayWindow::from_local_hour(15), DayWindow::Morning);
    assert_eq!(DayWindow::from_local_hour(16), DayWindow::Evening);
    assert_eq!(DayWindow::from_local_hour(21), DayWindow::Evening);
    assert_eq!(DayWindow::from_local_hour(3), DayWindow::Evening);
}

/// Seeds one note + arm + trial + response-independent context row.
fn seed_trial(s: &Store, with_response: bool) -> i64 {
    s.conn()
        .execute(
            "INSERT INTO note(text, created_at) VALUES ('nota', '2026-10-04T07:00:00Z')",
            [],
        )
        .unwrap();
    let note = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO arm(method, duration_min, day_window, prior_mean, prior_sd, prior_source, created_at)
             VALUES ('woop', 5, 'morning', 0.6, 0.15, 'Gollwitzer & Sheeran 2006', '2026-10-04T07:00:00Z')",
            [],
        )
        .unwrap();
    let arm = s.conn().last_insert_rowid();
    s.conn()
        .execute(
            "INSERT INTO trial(note_id, arm_id, started_at, predicted, selection_prob, is_exploration)
             VALUES (?1, ?2, '2026-10-04T07:00:00Z', 'pred', 0.5, 0)",
            rusqlite::params![note, arm],
        )
        .unwrap();
    let trial = s.conn().last_insert_rowid();
    if with_response {
        s.conn()
            .execute(
                "INSERT INTO response(trial_id, kind, score, logged_at)
                 VALUES (?1, 'immediate', 0.6, '2026-10-04T21:30:00Z')",
                rusqlite::params![trial],
            )
            .unwrap();
    }
    trial
}

#[test]
fn context_is_blind_until_the_response_is_logged() {
    let s = Store::open_in_memory().unwrap();
    let trial = seed_trial(&s, false);

    let now = 1_759_564_800; // 2025-10-04T08:00:00Z
    let sample = context::capture(now, 3, Some(4));
    assert_eq!(sample.day_window, DayWindow::Morning);
    assert!(sample.moon_phase >= 0.0 && sample.moon_phase < 1.0);
    assert!((1..=30).contains(&sample.moon_day));

    context::log_context(&s, trial, &sample).unwrap();

    // Blind: the row exists but no accessor returns it.
    assert!(context::read_context(&s, trial).unwrap().is_none());
    let rows: i64 = s
        .conn()
        .query_row(
            "SELECT count(*) FROM context WHERE trial_id=?1",
            [trial],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "the context row is written, just invisible");

    // Reveal without a response is denied.
    assert!(context::reveal(&s, trial, now + 50_000).is_err());
    assert!(context::read_context(&s, trial).unwrap().is_none());

    // Log the response: only now the context opens.
    s.conn()
        .execute(
            "INSERT INTO response(trial_id, kind, score, logged_at)
             VALUES (?1, 'immediate', 0.6, '2026-10-04T21:30:00Z')",
            rusqlite::params![trial],
        )
        .unwrap();
    context::reveal(&s, trial, now + 50_000).unwrap();
    let opened = context::read_context(&s, trial).unwrap().expect("opened");
    assert_eq!(opened.logged_at_unix, now);
    assert_eq!(opened.moon_day, sample.moon_day);
    assert!((opened.moon_phase - sample.moon_phase).abs() < 1e-12);
    assert_eq!(opened.sleep_quality, Some(4));
}

#[test]
fn reveal_gate_is_structural_even_with_manipulated_flag() {
    // Someone sets revealed_at by hand, bypassing reveal(): the reader
    // still refuses while the response is missing.
    let s = Store::open_in_memory().unwrap();
    let trial = seed_trial(&s, false);
    let sample = context::capture(1_759_564_800, 3, None);
    context::log_context(&s, trial, &sample).unwrap();
    s.conn()
        .execute(
            "UPDATE context SET revealed_at = '2026-10-04T21:30:00Z'",
            [],
        )
        .unwrap();
    assert!(
        context::read_context(&s, trial).unwrap().is_none(),
        "blindness must not depend on the flag alone"
    );
}
