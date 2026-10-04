//! The Kamerton binary: the thin executable surface of the engine.
//!
//! The schedule lives in `app.mlog` (`mlog serve`, cron ticks per Н1-06);
//! the ticks exec this binary, so the deterministic engine stays in Rust
//! and Metalogos only orchestrates (invariant 1: the model never decides).
//!
//! Subcommands:
//!   seed          — idempotent arm-library seed (seeds/arms.json, Н1-04)
//!   tick-morning  — bandit selects an arm, trial + blind context written,
//!                   the offer and the prediction request go to the chat
//!   tick-evening  — prompts for the immediate response of today's trial
//!   poll          — long-polling loop: parses replies, records responses
//!
//! Configuration (env only, per СЕКРЕТЫ and allowlist discipline):
//!   KAMERTON_TG_TOKEN   — Telegram bot token (never in code)
//!   KAMERTON_CHAT_ID    — the owner's chat
//!   KAMERTON_DB         — SQLite path (default ./kamerton.db)
//!   KAMERTON_TZ_OFFSET  — hours east of UTC (default 3, Moscow)
//!   KAMERTON_OFFSET     — getUpdates offset file (default <db>.offset)

use std::path::PathBuf;
use std::process::ExitCode;

use kamerton::arms;
use kamerton::bandit::{Bandit, BanditConfig};
use kamerton::store::Store;
use kamerton::telegram::Update;
use kamerton::telegram::{
    morning_tick, parse_reply, pending_trial_for_response, record_immediate, set_prediction,
    set_sleep_quality, token_from_env, LongPoll, Reply, UreqTransport,
};

fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("missing required env var {name}"))
}

fn db_path() -> PathBuf {
    PathBuf::from(env_or("KAMERTON_DB", "kamerton.db"))
}

fn tz_offset() -> i64 {
    env_or("KAMERTON_TZ_OFFSET", "3").parse().unwrap_or(3)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Opens the store and makes sure the schema is current. The tick
/// processes are one-shot, so a missed migration would be a silent crash
/// loop under systemd — apply it every time (idempotent).
fn open_store() -> Result<Store, String> {
    let mut store = Store::open(&db_path()).map_err(|e| format!("open db: {e}"))?;
    store
        .apply_migrations()
        .map_err(|e| format!("migrations: {e}"))?;
    Ok(store)
}

fn open_transport() -> Result<(UreqTransport, String), String> {
    let token = token_from_env("KAMERTON_TG_TOKEN").map_err(|e| e.to_string())?;
    let transport = UreqTransport::new(token.clone(), 15);
    Ok((transport, token))
}

fn chat_id() -> Result<i64, String> {
    required_env("KAMERTON_CHAT_ID")?
        .trim()
        .parse::<i64>()
        .map_err(|_| "KAMERTON_CHAT_ID must be an integer".to_string())
}

/// Sends one message through a throwaway LongPoll so the escape_html
/// guarantee of `LongPoll::send` is reused verbatim — no caller can
/// bypass it.
fn send_html(
    transport: &mut UreqTransport,
    token: &str,
    chat: i64,
    text: &str,
) -> Result<(), String> {
    let mut lp = LongPoll::new(transport, token, None).map_err(|e| e.to_string())?;
    lp.send(chat, text).map_err(|e| e.to_string())
}

/// The arm grid must be built by the same query the ticks use — the
/// bandit's indices and the DB rows stay in lockstep (see morning_tick).
fn bandit_on_grid(store: &Store) -> Result<Bandit, String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT prior_mean, prior_sd FROM arm WHERE is_active = 1 ORDER BY id")
        .map_err(|e| e.to_string())?;
    let priors: Vec<(f64, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .map(|row| row.expect("prior row"))
        .collect();
    if priors.is_empty() {
        return Err("arm library is empty: run `kamerton seed` first (Н1-04)".into());
    }
    Ok(Bandit::new(&priors, BanditConfig::default()))
}

/// Text of the arm at the bandit's grid index, for the chat message.
fn arm_text_at(store: &Store, index: usize) -> Result<String, String> {
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT method, duration_min, day_window FROM arm
             WHERE is_active = 1 ORDER BY id LIMIT 1 OFFSET ?1",
        )
        .map_err(|e| e.to_string())?;
    let row = stmt
        .query_row([index as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    Ok(format!("{} ({} мин, окно {})", row.0, row.1, row.2))
}

fn cmd_seed() -> Result<(), String> {
    let store = open_store()?;
    let report =
        arms::seed(&store, include_str!("../seeds/arms.json")).map_err(|e| e.to_string())?;
    println!(
        "seed: inserted {}, skipped {} (idempotent)",
        report.inserted, report.skipped
    );
    Ok(())
}

fn cmd_tick_morning() -> Result<(), String> {
    let store = open_store()?;
    let mut bandit = bandit_on_grid(&store)?;
    let selection =
        morning_tick(&store, &mut bandit, now_unix(), tz_offset()).map_err(|e| e.to_string())?;
    let arm = arm_text_at(&store, selection.arm_index)?;
    let (mut transport, token) = open_transport()?;
    let chat = chat_id()?;
    let mode = if selection.is_exploration {
        "разведка"
    } else {
        "выбор"
    };
    let text = format!(
        "Камертон · утро\nПрактика: {}\nРежим: {}, вероятность выбора {:.2}\n\nЗапиши предсказание ответом:\nп: что ты почувствуешь после практики",
        arm, mode, selection.selection_prob
    );
    send_html(&mut transport, &token, chat, &text)?;
    println!("morning tick: trial + blind context written, offer sent");
    Ok(())
}

fn cmd_tick_evening() -> Result<(), String> {
    let store = open_store()?;
    let Some(trial_id) = pending_trial_for_response(&store).map_err(|e| e.to_string())? else {
        println!("evening tick: no trial awaiting an immediate response");
        return Ok(());
    };
    let (mut transport, token) = open_transport()?;
    let chat = chat_id()?;
    let text = "Камертон · вечер\nКак прошла практика? Оцени отклик одним числом от 0 до 1 (например 0.3 или 0,75).\nКачество сна, если помнишь: с: число от 1 до 5";
    send_html(&mut transport, &token, chat, text)?;
    println!("evening tick: prompt sent for trial {trial_id}");
    Ok(())
}

/// Records one chat reply against the store. Returns the reply text to
/// send, if any — the sending happens outside, in the poll loop, so the
/// handler stays transport-free and unit-testable.
///
/// The trial id rule mirrors the evening tick: the latest trial without
/// an immediate response; predictions and sleep quality go to the latest
/// trial. A reply can never attach to a wrong trial.
fn handle_reply(store: &Store, text: &str) -> Option<String> {
    let trial = pending_trial_for_response(store).ok().flatten();
    let latest = latest_trial_id(store);
    match parse_reply(text) {
        Reply::Prediction(value) => {
            if let Some(id) = latest {
                if let Err(e) = set_prediction(store, id, &value) {
                    return Some(format!("предсказание не записано: {e}"));
                }
                return Some("Предсказание записано.".into());
            }
            None
        }
        Reply::Sleep(quality) => {
            if let Some(id) = latest {
                if let Err(e) = set_sleep_quality(store, id, quality) {
                    return Some(format!("качество сна не записано: {e}"));
                }
                return Some("Качество сна записано.".into());
            }
            None
        }
        Reply::Score(score) => {
            if let Some(id) = trial {
                if let Err(e) = record_immediate(store, id, score, now_unix()) {
                    return Some(format!("отклик не записан: {e}"));
                }
                return Some("Отклик записан. Контекст раскроется к следующей пробе.".into());
            }
            None
        }
        Reply::Unknown => Some(
            "Не понял ответ. Число 0–1 — отклик; п: текст — предсказание; с: 1–5 — сон.".into(),
        ),
    }
}

fn latest_trial_id(store: &Store) -> Option<i64> {
    store
        .conn()
        .query_row("SELECT id FROM trial ORDER BY id DESC LIMIT 1", [], |r| {
            r.get(0)
        })
        .ok()
}

fn cmd_poll() -> Result<(), String> {
    let store = open_store()?;
    let (mut transport, token) = open_transport()?;
    let mut offset_path = db_path();
    offset_path.set_extension("offset");
    let mut poll =
        LongPoll::new(&mut transport, &token, Some(offset_path)).map_err(|e| e.to_string())?;
    loop {
        let mut replies: Vec<(i64, String)> = Vec::new();
        poll.run_step(&mut |update: &Update| {
            let Some(text) = &update.text else { return };
            if let Some(reply) = handle_reply(&store, text) {
                if let Some(chat) = update.chat_id {
                    replies.push((chat, reply));
                }
            }
        })
        .map_err(|e| e.to_string())?;
        for (chat, text) in &replies {
            poll.send(*chat, text).map_err(|e| e.to_string())?;
        }
    }
}

fn usage() -> String {
    "usage: kamerton <seed|tick-morning|tick-evening|poll>".into()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("seed") => cmd_seed(),
        Some("tick-morning") => cmd_tick_morning(),
        Some("tick-evening") => cmd_tick_evening(),
        Some("poll") => cmd_poll(),
        _ => Err(usage()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kamerton: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kamerton::context;

    #[test]
    fn reply_scores_a_trial_and_records_context_reveal() {
        let mut store = Store::open_in_memory().expect("store");
        store.apply_migrations().expect("migrations");
        store
            .conn()
            .execute(
                "INSERT INTO note(text, is_active, created_at)
                 VALUES ('test note', 1, '2026-10-04T07:00:00+03:00')",
                [],
            )
            .expect("note");
        store
            .conn()
            .execute(
                "INSERT INTO arm(method, duration_min, day_window, prior_mean, prior_sd, prior_source, is_active, created_at)
                 VALUES ('breathing', 10, 'morning', 0.5, 0.3, 'tests', 1, '2026-10-04T07:00:00+03:00')",
                [],
            )
            .expect("arm");
        store
            .conn()
            .execute(
                "INSERT INTO trial(note_id, arm_id, started_at, predicted, selection_prob, is_exploration)
                 VALUES (1, 1, '2026-10-04T07:00:00+03:00', 'x', 0.5, 0)",
                [],
            )
            .expect("trial");
        // The real morning tick always writes the blind context row; the
        // test setup mirrors that (invariant 2).
        let sample = kamerton::context::capture(1_791_000_000, 3, None);
        kamerton::context::log_context(&store, 1, &sample).expect("blind context");

        // Before the reply the context row is blind; the score reply
        // records the immediate response and opens the context (the
        // structural reveal inside record_immediate).
        let reply = handle_reply(&store, "0,4").expect("reply for a valid score");
        assert!(reply.contains("записан"));
        let revealed: Option<String> = store
            .conn()
            .query_row(
                "SELECT revealed_at FROM context WHERE trial_id = 1",
                [],
                |r| r.get(0),
            )
            .expect("context row must exist after the reveal");
        assert!(revealed.is_some(), "the reply must open the blind context");

        let score: f64 = store
            .conn()
            .query_row(
                "SELECT score FROM response WHERE trial_id = 1 AND kind = 'immediate'",
                [],
                |r| r.get(0),
            )
            .expect("immediate response");
        assert!((score - 0.4).abs() < 1e-9);
    }

    #[test]
    fn unknown_replies_get_help_without_writes() {
        let mut store = Store::open_in_memory().expect("store");
        store.apply_migrations().expect("migrations");
        let reply = handle_reply(&store, "что-то непонятное");
        assert!(reply.is_some(), "unknown text must get the help line");
        let rows: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM response", [], |r| r.get(0))
            .expect("count");
        assert_eq!(rows, 0);
        // Silence check helper: capture() is importable so the blind
        // context discipline stays visible in the binary's own tests.
        let _ = context::capture;
    }
}
