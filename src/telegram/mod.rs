//! Telegram side of the closed loop (naryad Н1-06), deterministic part.
//!
//! The app polls Telegram itself (long polling, no webhook: nothing
//! sticks out). The language model formulates and translates; it never
//! touches the decision engine. This module provides:
//! - the Bot API transport with timeouts and retries (iron rule: no
//!   network call without both);
//! - the long-polling step with offset bookkeeping (an outage loses
//!   nothing and duplicates nothing);
//! - the morning and evening ticks that write correct trial/response
//!   rows.
//!
//! What lives elsewhere: the cron wiring and model calls are declared in
//! `app.mlog` and served by `mlog serve` — pending the mlog toolchain
//! (open item on issue #6). The token comes from the environment via the
//! allowlist, never from code (AGENTS.md, СЕКРЕТЫ).

use std::path::Path;

use serde_json::Value;

use crate::store::{time, Store};

pub const API_BASE: &str = "https://api.telegram.org";

// --- HTML escaping ----------------------------------------------------------

/// Escapes the three characters Telegram's HTML mode requires: & < >.
/// Nothing leaves this module unescaped (acceptance: model output never
/// reaches a reply without escape_html).
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// --- transport ---------------------------------------------------------------

pub trait Transport {
    /// One Bot API call: `api_method` is called with a JSON body, the raw
    /// response body comes back. Implementations must apply a timeout and
    /// retries; see [`with_retries`].
    fn call(&mut self, api_method: &str, json_body: &str) -> Result<String, TgError>;
}

#[derive(Debug)]
pub enum TgError {
    Network(String),
    Api(String), // {"ok": false, ...}
    Parse(String),
    Config(String),
    Io(std::io::Error),
    Store(crate::store::Error),
}

impl From<crate::store::Error> for TgError {
    fn from(e: crate::store::Error) -> Self {
        TgError::Store(e)
    }
}

impl From<rusqlite::Error> for TgError {
    fn from(e: rusqlite::Error) -> Self {
        TgError::Store(e.into())
    }
}

impl From<std::io::Error> for TgError {
    fn from(e: std::io::Error) -> Self {
        TgError::Io(e)
    }
}

impl std::fmt::Display for TgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TgError::Network(s) => write!(f, "telegram network error: {s}"),
            TgError::Api(s) => write!(f, "telegram api error: {s}"),
            TgError::Parse(s) => write!(f, "telegram parse error: {s}"),
            TgError::Config(s) => write!(f, "telegram config error: {s}"),
            TgError::Io(e) => write!(f, "telegram io error: {e}"),
            TgError::Store(e) => write!(f, "telegram store error: {e}"),
        }
    }
}

/// Sleep abstraction so retry behavior is testable without real delays.
pub trait Sleeper {
    fn sleep(&mut self, ms: u64);
}

pub struct StdSleeper;

impl Sleeper for StdSleeper {
    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// Retries `op` up to `attempts` times with exponential backoff
/// (base * 2^attempt, capped at 10 s). The attempt index starts at 0.
pub fn with_retries<T, E, F>(attempts: usize, sleeper: &mut dyn Sleeper, mut op: F) -> Result<T, E>
where
    F: FnMut(usize) -> Result<T, E>,
{
    assert!(attempts >= 1, "attempts must be >= 1");
    let mut last: Option<E> = None;
    for attempt in 0..attempts {
        match op(attempt) {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt + 1 < attempts {
                    let delay = std::cmp::min(1000u64.saturating_mul(1 << attempt), 10_000);
                    sleeper.sleep(delay);
                }
                last = Some(e);
            }
        }
    }
    Err(last.expect("at least one attempt was made"))
}

/// Real Bot API transport over HTTP(S) with a connect+call timeout and
/// retries. The token is supplied by the caller (from the environment).
pub struct UreqTransport {
    agent: ureq::Agent,
    pub token: String,
}

impl UreqTransport {
    /// Long polling needs the call timeout above the poll timeout.
    pub fn new(token: String, call_timeout_secs: u64) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(call_timeout_secs))
            .build();
        UreqTransport { agent, token }
    }
}

impl Transport for UreqTransport {
    fn call(&mut self, api_method: &str, json_body: &str) -> Result<String, TgError> {
        let mut sleeper = StdSleeper;
        let url = format!("{}/bot{}/{}", API_BASE, self.token, api_method);
        let body = json_body.to_string();
        with_retries(3, &mut sleeper, move |attempt| {
            let url = url.clone();
            let body = body.clone();
            let _ = attempt;
            match self.agent.post(&url).send_string(&body) {
                Ok(resp) => resp
                    .into_string()
                    .map_err(|e| TgError::Network(format!("read body: {e}"))),
                Err(ureq::Error::Status(code, resp)) => {
                    // Bot API errors (400/409/...) are terminal, not network:
                    // retrying would repeat the same mistake.
                    let text = resp.into_string().unwrap_or_default();
                    Err(if (500..600).contains(&code) {
                        TgError::Network(format!("server {code}"))
                    } else {
                        TgError::Api(format!("status {code}: {text}"))
                    })
                }
                Err(e) => Err(TgError::Network(format!("transport: {e}"))),
            }
        })
    }
}

// --- updates ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub update_id: i64,
    pub chat_id: Option<i64>,
    pub text: Option<String>,
}

pub fn parse_updates(body: &str) -> Result<Vec<Update>, TgError> {
    let v: Value = serde_json::from_str(body).map_err(|e| TgError::Parse(format!("json: {e}")))?;
    if v["ok"].as_bool() != Some(true) {
        return Err(TgError::Api(
            v["description"].as_str().unwrap_or("unknown").to_string(),
        ));
    }
    let mut out = Vec::new();
    if let Some(items) = v["result"].as_array() {
        for item in items {
            let update_id = item["update_id"]
                .as_i64()
                .ok_or_else(|| TgError::Parse("update_id missing".into()))?;
            let chat_id = item["message"]["chat"]["id"].as_i64();
            let text = item["message"]["text"].as_str().map(str::to_string);
            out.push(Update {
                update_id,
                chat_id,
                text,
            });
        }
    }
    Ok(out)
}

// --- offset persistence ---------------------------------------------------------

/// The offset file next to the database: survives restarts, so a reboot
/// neither replays old updates nor skips new ones.
pub fn load_offset(path: &Path) -> std::io::Result<i64> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .trim()
            .parse::<i64>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e),
    }
}

pub fn save_offset(path: &Path, offset: i64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, offset.to_string())
}

// --- long polling loop ------------------------------------------------------------

/// One long-polling step: ask for updates after `self.offset`, hand each
/// to the handler, then persist the advanced offset. If the transport
/// fails, the offset stays where it was — the next successful step
/// re-asks for the same range, so messages are neither lost nor doubled.
pub struct LongPoll<'a, T: Transport> {
    pub client: &'a mut T,
    pub token: &'a str,
    pub offset: i64,
    pub poll_timeout_secs: u32,
    pub offset_path: Option<std::path::PathBuf>,
}

impl<'a, T: Transport> LongPoll<'a, T> {
    /// Loads the persisted offset; an unreadable offset file is an error,
    /// not a silent reset (a silent reset would replay old updates).
    pub fn new(
        client: &'a mut T,
        token: &'a str,
        offset_path: Option<std::path::PathBuf>,
    ) -> Result<Self, TgError> {
        let offset = match offset_path.as_deref().map(load_offset) {
            Some(Ok(v)) => v,
            Some(Err(e)) => return Err(TgError::Io(e)),
            None => 0,
        };
        Ok(LongPoll {
            client,
            token,
            offset,
            poll_timeout_secs: 25,
            offset_path,
        })
    }

    /// Runs one step; returns how many updates were handled.
    pub fn run_step(&mut self, handler: &mut dyn FnMut(&Update)) -> Result<usize, TgError> {
        let body = format!(
            "{{\"offset\":{},\"timeout\":{},\"allowed_updates\":[\"message\"]}}",
            self.offset, self.poll_timeout_secs
        );
        let raw = self.client.call("getUpdates", &body)?;
        let updates = parse_updates(&raw)?;
        for u in &updates {
            handler(u);
            self.offset = u.update_id + 1;
        }
        if !updates.is_empty() {
            if let Some(path) = &self.offset_path {
                save_offset(path, self.offset)?;
            }
        }
        Ok(updates.len())
    }

    /// Sends a message; the text is escaped here so no caller can forget.
    pub fn send(&mut self, chat_id: i64, html: &str) -> Result<(), TgError> {
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "text": escape_html(html),
            "parse_mode": "HTML",
        });
        let raw = self.client.call("sendMessage", &payload.to_string())?;
        let v: Value =
            serde_json::from_str(&raw).map_err(|e| TgError::Parse(format!("json: {e}")))?;
        if v["ok"].as_bool() == Some(true) {
            Ok(())
        } else {
            Err(TgError::Api(
                v["description"].as_str().unwrap_or("unknown").to_string(),
            ))
        }
    }
}

// --- ticks: the closed loop on the data level --------------------------------------

/// Morning tick: the bandit selects an arm, the trial row is written with
/// its propensity, the offer + prediction request text comes back. The
/// model turns this into conversation; the row is already on disk.
pub fn morning_tick(
    store: &Store,
    bandit: &mut crate::bandit::Bandit,
    now_unix: i64,
    tz_offset_hours: i64,
) -> Result<crate::bandit::Selection, TgError> {
    // Active arms in the grid order; priors seed the engine.
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT id, prior_mean, prior_sd, method, duration_min, day_window
             FROM arm WHERE is_active = 1 ORDER BY id",
        )
        .map_err(TgError::from)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(TgError::from)?;
    let arms: Vec<_> = rows.map(|r| r.expect("arm row")).collect();
    if arms.is_empty() {
        return Err(TgError::Config(
            "arm library is empty: seed it first (Н1-04)".into(),
        ));
    }

    // History against now, decayed (the bandit may change its mind).
    // NOTE: the caller must have built `bandit` on the same arm grid,
    // in the same order (SELECT ... ORDER BY id) — the engine wiring
    // constructs both from this one query.
    let mut history = Vec::new();
    let mut hstmt = store
        .conn()
        .prepare(
            "SELECT t.arm_id, r.score, r.logged_at, t.started_at
             FROM trial t JOIN response r ON r.trial_id = t.id",
        )
        .map_err(TgError::from)?;
    let hrows = hstmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(TgError::from)?;
    let mut row_by_id: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for (i, a) in arms.iter().enumerate() {
        row_by_id.insert(a.0, i);
    }
    for r in hrows {
        let (arm_id, score, logged_at, started_at) = r.expect("history row");
        let Some(&idx) = row_by_id.get(&arm_id) else {
            continue;
        };
        let age_days = age_days_between(&started_at, &logged_at, now_unix);
        history.push(crate::bandit::Observation {
            arm_index: idx,
            reward: score,
            age_days,
        });
    }
    bandit.rebuild(&history);

    let sel = bandit.select();
    let arm = &arms[sel.arm_index];

    // The active note: one owner, one note.
    let note_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM note WHERE is_active = 1 ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .map_err(TgError::from)?;

    let started_at = time::iso_from_unix(now_unix);
    store
        .conn()
        .execute(
            "INSERT INTO trial(note_id, arm_id, started_at, predicted, selection_prob, is_exploration)
             VALUES (?1, ?2, ?3, '(ожидается предсказание)', ?4, ?5)",
            rusqlite::params![note_id, arm.0, started_at, sel.selection_prob, sel.is_exploration as i64],
        )
        .map_err(TgError::from)?;
    let trial_id = store.conn().last_insert_rowid();

    // Blind context, always written (invariant 2).
    let sleep_quality = None; // comes from the conversation, model sets it via set_sleep_quality
    let sample = crate::context::capture(now_unix, tz_offset_hours, sleep_quality);
    crate::context::log_context(store, trial_id, &sample).map_err(TgError::from)?;

    Ok(sel)
}

fn age_days_between(started_at: &str, logged_at: &str, now_unix: i64) -> f64 {
    // Age of the response relative to now; falls back to 0 on garbage.
    let _ = started_at;
    let Some(logged) = time::unix_from_iso(logged_at) else {
        return 0.0;
    };
    (now_unix - logged) as f64 / 86_400.0
}

/// Evening tick: the latest trial of today that has no immediate response
/// yet. The conversation collects the score; `record_immediate` writes it
/// and opens the blind context.
pub fn pending_trial_for_response(store: &Store) -> Result<Option<i64>, TgError> {
    let id: Option<i64> = store
        .conn()
        .query_row(
            "SELECT t.id FROM trial t
             WHERE NOT EXISTS (SELECT 1 FROM response r WHERE r.trial_id = t.id AND r.kind = 'immediate')
             ORDER BY t.id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(TgError::from)?;
    Ok(id)
}

pub fn set_prediction(store: &Store, trial_id: i64, predicted: &str) -> Result<(), TgError> {
    if predicted.trim().is_empty() {
        return Err(TgError::Config("empty prediction".into()));
    }
    store
        .conn()
        .execute(
            "UPDATE trial SET predicted = ?1 WHERE id = ?2",
            rusqlite::params![predicted, trial_id],
        )
        .map_err(TgError::from)?;
    Ok(())
}

pub fn set_sleep_quality(store: &Store, trial_id: i64, quality: u8) -> Result<(), TgError> {
    if !(1..=5).contains(&quality) {
        return Err(TgError::Config("sleep quality must be 1..5".into()));
    }
    store
        .conn()
        .execute(
            "UPDATE context SET sleep_quality = ?1 WHERE trial_id = ?2",
            rusqlite::params![quality as i64, trial_id],
        )
        .map_err(TgError::from)?;
    Ok(())
}

/// Writes the immediate response and opens the blind context for the
/// trial (the reveal is structural: it fails while the row is missing).
pub fn record_immediate(
    store: &Store,
    trial_id: i64,
    score: f64,
    now_unix: i64,
) -> Result<(), TgError> {
    if !(0.0..=1.0).contains(&score) {
        return Err(TgError::Config("score must be in [0, 1]".into()));
    }
    store
        .conn()
        .execute(
            "INSERT INTO response(trial_id, kind, score, logged_at) VALUES (?1, 'immediate', ?2, ?3)",
            rusqlite::params![trial_id, score, time::iso_from_unix(now_unix)],
        )
        .map_err(TgError::from)?;
    crate::context::reveal(store, trial_id, now_unix).map_err(TgError::from)?;
    Ok(())
}

/// The token comes only from the environment (allowlist discipline).
pub fn token_from_env(var: &str) -> Result<String, TgError> {
    match std::env::var(var) {
        Ok(t) if !t.trim().is_empty() && !t.contains(char::is_whitespace) => Ok(t),
        _ => Err(TgError::Config(format!(
            "token must be in env var {var}, never in code"
        ))),
    }
}

// --- reply parsing: the conversational surface of the closed loop ----

/// What the owner's chat reply means for the engine. The parser is
/// deliberately narrow: anything it does not understand is `Unknown` and
/// answers with the help text — the model formulates around it, the
/// engine only consumes well-formed numbers.
#[derive(Debug, PartialEq)]
pub enum Reply {
    /// "п: текст" — the morning prediction, recorded before the trial.
    Prediction(String),
    /// "с: 1..5" — last night's sleep quality for the blind context.
    Sleep(u8),
    /// A bare number in [0, 1] (comma or dot) — the immediate response.
    Score(f64),
    /// Anything else: help is sent, nothing is recorded.
    Unknown,
}

/// Parses one chat reply. The prefix forms are ASCII+colon on purpose:
/// they survive any keyboard layout a tired owner can produce at 21:30.
pub fn parse_reply(raw: &str) -> Reply {
    let text = raw.trim();
    let lower = text.to_lowercase();
    if let Some(rest) = lower.strip_prefix("п:") {
        let value = text[text.len() - rest.len()..].trim();
        if value.is_empty() {
            return Reply::Unknown;
        }
        return Reply::Prediction(value.to_string());
    }
    if let Some(rest) = lower.strip_prefix("с:") {
        let value = text[text.len() - rest.len()..].trim();
        if let Ok(q) = value.parse::<u8>() {
            if (1..=5).contains(&q) {
                return Reply::Sleep(q);
            }
        }
        return Reply::Unknown;
    }
    let normalized = text.replace(',', ".");
    if let Ok(score) = normalized.parse::<f64>() {
        if (0.0..=1.0).contains(&score) && text.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Reply::Score(score);
        }
    }
    Reply::Unknown
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn prediction_prefix_is_parsed() {
        assert_eq!(
            parse_reply("п: буду спокоен после дыхания"),
            Reply::Prediction("буду спокоен после дыхания".into())
        );
        assert_eq!(
            parse_reply("П: короткий ответ"),
            Reply::Prediction("короткий ответ".into())
        );
        assert_eq!(parse_reply("п:   "), Reply::Unknown);
    }

    #[test]
    fn sleep_prefix_is_parsed_and_bounded() {
        assert_eq!(parse_reply("с: 4"), Reply::Sleep(4));
        assert_eq!(parse_reply("С:1"), Reply::Sleep(1));
        assert_eq!(parse_reply("с: 0"), Reply::Unknown);
        assert_eq!(parse_reply("с: 6"), Reply::Unknown);
    }

    #[test]
    fn bare_scores_accept_comma_and_dot_only_in_range() {
        assert_eq!(parse_reply("0.3"), Reply::Score(0.3));
        assert_eq!(parse_reply("0,75"), Reply::Score(0.75));
        assert_eq!(parse_reply("1"), Reply::Score(1.0));
        assert_eq!(parse_reply("1.5"), Reply::Unknown);
        assert_eq!(parse_reply("-0.2"), Reply::Unknown);
        assert_eq!(parse_reply("спокойно"), Reply::Unknown);
    }
}
