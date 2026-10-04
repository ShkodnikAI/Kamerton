//! Blind context logger (invariant 2, naryad Н1-07).
//!
//! The context of every trial — time, day window, sleep quality, Moon
//! phase and Moon day — is always written, but stays invisible until the
//! response is logged. Moon math uses plain synodic-month arithmetic with
//! no external libraries.

use crate::store::{Error, Result, Store};

/// Reference new moon: 2000-01-06 18:14 UTC (mean lunation epoch used by
/// most almanacs).
pub const NEW_MOON_EPOCH_UNIX: i64 = 947_182_440;
/// Mean synodic month: 29 d 12 h 44 min 2.9 s.
pub const SYNODIC_SECONDS: f64 = 2_551_442.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayWindow {
    Morning,
    Evening,
}

impl DayWindow {
    pub fn as_str(self) -> &'static str {
        match self {
            DayWindow::Morning => "morning",
            DayWindow::Evening => "evening",
        }
    }

    /// The morning tick fires at 7:00, the evening one at 21:30 (owner's
    /// local time). Everything before 16:00 local counts as morning.
    pub fn from_local_hour(hour: i64) -> Self {
        if (4..16).contains(&hour) {
            DayWindow::Morning
        } else {
            DayWindow::Evening
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextSample {
    pub logged_at_unix: i64,
    pub day_window: DayWindow,
    pub sleep_quality: Option<u8>,
    /// Fraction of the synodic cycle at the trial moment: 0 = new moon,
    /// 0.5 = full moon. Stored with the row, hidden until the response.
    pub moon_phase: f64,
    /// Moon day 1..=30 (1 = the new moon day).
    pub moon_day: u8,
}

/// Fractional phase of the synodic cycle at `unix_seconds`:
/// 0 = new moon, 0.5 = full moon.
pub fn moon_phase(unix_seconds: i64) -> f64 {
    let since_epoch = (unix_seconds - NEW_MOON_EPOCH_UNIX) as f64;
    let cycles = since_epoch / SYNODIC_SECONDS;
    let phase = cycles.rem_euclid(1.0);
    // Guard against a float artefact landing exactly on 1.0.
    if phase >= 1.0 {
        0.0
    } else {
        phase
    }
}

/// Moon day 1..=30 from the phase.
pub fn moon_day(unix_seconds: i64) -> u8 {
    let phase = moon_phase(unix_seconds);
    let day = (phase * 29.530_588_853).floor() as i64 + 1;
    day.clamp(1, 30) as u8
}

/// Captures the blind context at `now_unix` (owner's clock). The timezone
/// offset comes from the engine config, not from the environment.
pub fn capture(now_unix: i64, tz_offset_hours: i64, sleep_quality: Option<u8>) -> ContextSample {
    let local_hour = (now_unix.div_euclid(3600) + tz_offset_hours).rem_euclid(24);
    ContextSample {
        logged_at_unix: now_unix,
        day_window: DayWindow::from_local_hour(local_hour),
        sleep_quality,
        moon_phase: moon_phase(now_unix),
        moon_day: moon_day(now_unix),
    }
}

/// Writes the context row for a trial. The row starts blind:
/// `revealed_at` is NULL and no accessor returns it until the response is
/// logged (invariant 2).
pub fn log_context(store: &Store, trial_id: i64, sample: &ContextSample) -> Result<()> {
    store.conn().execute(
        "INSERT INTO context(trial_id, logged_at, day_window, sleep_quality, moon_phase, moon_day, revealed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
        rusqlite::params![
            trial_id,
            crate::store::time::iso_from_unix(sample.logged_at_unix),
            sample.day_window.as_str(),
            sample.sleep_quality,
            sample.moon_phase,
            sample.moon_day as i64,
        ],
    )?;
    Ok(())
}

/// Opens the context after the response is logged. Fails when the response
/// is not in the database yet — the gate is structural, not a promise.
pub fn reveal(store: &Store, trial_id: i64, revealed_at_unix: i64) -> Result<()> {
    let changed = store.conn().execute(
        "UPDATE context SET revealed_at = ?1
         WHERE trial_id = ?2
           AND EXISTS (SELECT 1 FROM response WHERE response.trial_id = context.trial_id)",
        rusqlite::params![
            crate::store::time::iso_from_unix(revealed_at_unix),
            trial_id
        ],
    )?;
    if changed == 0 {
        return Err(Error::Io(std::io::Error::other(format!(
            "reveal denied for trial {trial_id}: no response logged (invariant 2)"
        ))));
    }
    Ok(())
}

/// Reads the context of a trial. Returns `Ok(None)` while the context is
/// still blind; some data only after the response opened it.
pub fn read_context(store: &Store, trial_id: i64) -> Result<Option<ContextSample>> {
    let mut stmt = store.conn().prepare(
        "SELECT logged_at, day_window, sleep_quality, moon_phase, moon_day
         FROM context
         WHERE trial_id = ?1
           AND revealed_at IS NOT NULL
           AND EXISTS (SELECT 1 FROM response WHERE response.trial_id = context.trial_id)",
    )?;
    let mut rows = stmt.query(rusqlite::params![trial_id])?;
    match rows.next()? {
        None => Ok(None),
        Some(row) => {
            let logged_at: String = row.get(0)?;
            let window: String = row.get(1)?;
            let sleep_quality: Option<i64> = row.get(2)?;
            let moon_phase: f64 = row.get(3)?;
            let moon_day: i64 = row.get(4)?;
            let logged_at_unix =
                crate::store::time::unix_from_iso(&logged_at).ok_or_else(|| {
                    Error::Io(std::io::Error::other(format!(
                        "bad timestamp in context row: {logged_at}"
                    )))
                })?;
            let day_window = if window == DayWindow::Morning.as_str() {
                DayWindow::Morning
            } else {
                DayWindow::Evening
            };
            Ok(Some(ContextSample {
                logged_at_unix,
                day_window,
                sleep_quality: sleep_quality.map(|q| q.clamp(1, 5) as u8),
                moon_phase,
                moon_day: moon_day.clamp(1, 30) as u8,
            }))
        }
    }
}
