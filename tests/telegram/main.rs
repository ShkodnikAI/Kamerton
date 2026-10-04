//! Integration tests for the Telegram side (naryad Н1-06, deterministic
//! part). The real network path is covered by the transport with timeout
//! and retries; here a mock transport drives the logic.

use std::path::PathBuf;

use kamerton::bandit::{Bandit, BanditConfig};
use kamerton::store::Store;
use kamerton::telegram::{
    escape_html, load_offset, morning_tick, parse_updates, pending_trial_for_response,
    record_immediate, set_prediction, set_sleep_quality, token_from_env, with_retries, LongPoll,
    TgError, Transport, Update,
};

/// A scripted transport: every call consumes the next instruction.
enum Step {
    Ok(String),
    Fail(String),
}

struct MockTransport {
    steps: std::collections::VecDeque<Step>,
    calls: Vec<(String, String)>,
}

impl MockTransport {
    fn new(steps: Vec<Step>) -> Self {
        MockTransport {
            steps: steps.into(),
            calls: Vec::new(),
        }
    }
}

impl Transport for MockTransport {
    fn call(&mut self, api_method: &str, json_body: &str) -> Result<String, TgError> {
        self.calls
            .push((api_method.to_string(), json_body.to_string()));
        match self.steps.pop_front() {
            Some(Step::Ok(body)) => Ok(body),
            Some(Step::Fail(e)) => Err(TgError::Network(e)),
            None => Err(TgError::Network("no more scripted steps".into())),
        }
    }
}

struct NoSleep;
impl kamerton::telegram::Sleeper for NoSleep {
    fn sleep(&mut self, _ms: u64) {}
}

fn updates_body(ids: &[i64]) -> String {
    let items: Vec<String> = ids
        .iter()
        .map(|id| {
            format!(
                "{{\"update_id\":{id},\"message\":{{\"chat\":{{\"id\":777}},\"text\":\"отклик 0.6\"}}}}"
            )
        })
        .collect();
    format!("{{\"ok\":true,\"result\":[{}]}}", items.join(","))
}

#[test]
fn outage_of_ten_minutes_loses_nothing_and_duplicates_nothing() {
    // Five messages arrive; then the network dies for the length of a 10
    // minute outage (several failed long polls); on recovery the loop
    // re-asks from the stored offset. Every message is handled exactly
    // once, in order.
    let tmp = std::env::temp_dir().join(format!("kamerton-tg-{}-offset", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    // Phase 1: two updates land.
    let mut mock = MockTransport::new(vec![
        Step::Ok(updates_body(&[100, 101])),
        Step::Fail("connection reset".into()),
        Step::Fail("timeout".into()),
        Step::Fail("connection reset".into()),
        Step::Ok(updates_body(&[102])), // recovery: offset was persisted at 102
    ]);
    let handler_seen = std::cell::RefCell::new(Vec::<i64>::new());
    let mut handler = |u: &Update| handler_seen.borrow_mut().push(u.update_id);
    {
        let mut poll = LongPoll::new(&mut mock, "TOKEN", Some(tmp.clone())).unwrap();
        assert_eq!(poll.run_step(&mut handler).unwrap(), 2);
        assert_eq!(poll.offset, 102);
        assert!(poll.run_step(&mut handler).is_err());
        assert!(poll.run_step(&mut handler).is_err());
        assert!(poll.run_step(&mut handler).is_err());
        // Offset unchanged by failures: nothing is "confirmed" twice.
        assert_eq!(poll.offset, 102);
        assert_eq!(poll.run_step(&mut handler).unwrap(), 1);
        assert_eq!(poll.offset, 103);
    }
    assert_eq!(
        handler_seen.borrow().clone(),
        vec![100, 101, 102],
        "no loss, no duplicates"
    );
    // The offset file carries the state across a restart.
    assert_eq!(load_offset(&tmp).unwrap(), 103);

    // Phase 2: restart with the persisted offset — the first getUpdates
    // call already asks from 103, not from zero.
    let mut restart = MockTransport::new(vec![Step::Ok("{\"ok\":true,\"result\":[]}".into())]);
    {
        let mut poll = LongPoll::new(&mut restart, "TOKEN", Some(tmp.clone())).unwrap();
        poll.run_step(&mut handler).unwrap();
    }
    let (method, body) = &restart.calls[0];
    assert_eq!(method, "getUpdates");
    assert!(body.contains("\"offset\":103"), "recovered offset: {body}");

    std::fs::remove_file(&tmp).unwrap();
}

#[test]
fn retry_policy_backs_off_and_gives_up() {
    struct Recorder(Vec<u64>);
    impl kamerton::telegram::Sleeper for Recorder {
        fn sleep(&mut self, ms: u64) {
            self.0.push(ms);
        }
    }
    let mut rec = Recorder(Vec::new());
    let mut attempts = 0;
    let result: Result<u8, String> = with_retries(4, &mut rec, |_| {
        attempts += 1;
        Err(format!("fail {attempts}"))
    });
    assert_eq!(attempts, 4);
    assert!(result.is_err());
    assert_eq!(rec.0, vec![1000, 2000, 4000], "exponential backoff, capped");

    // Recovers when the call starts succeeding.
    let mut fails = 2;
    let ok: Result<&str, String> = with_retries(3, &mut NoSleep, |_| {
        if fails > 0 {
            fails -= 1;
            Err("transient".into())
        } else {
            Ok("done")
        }
    });
    assert_eq!(ok.unwrap(), "done");
}

#[test]
fn parse_updates_and_escape_html() {
    let ups = parse_updates(&updates_body(&[5, 6])).unwrap();
    assert_eq!(ups.len(), 2);
    assert_eq!(ups[0].chat_id, Some(777));
    assert_eq!(ups[0].text.as_deref(), Some("отклик 0.6"));

    let bad = parse_updates("{\"ok\":false,\"description\":\"boom\"}").unwrap_err();
    assert!(matches!(bad, TgError::Api(ref s) if s == "boom"));

    assert_eq!(escape_html("plain"), "plain");
    assert_eq!(escape_html("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
}

#[test]
fn token_comes_only_from_the_environment() {
    std::env::remove_var("KAMERTON_TG_TOKEN_TEST");
    assert!(token_from_env("KAMERTON_TG_TOKEN_TEST").is_err());
    std::env::set_var("KAMERTON_TG_TOKEN_TEST", "123:abc");
    assert!(token_from_env("KAMERTON_TG_TOKEN_TEST").is_ok());
    std::env::set_var("KAMERTON_TG_TOKEN_TEST", "with space");
    assert!(token_from_env("KAMERTON_TG_TOKEN_TEST").is_err());
    std::env::remove_var("KAMERTON_TG_TOKEN_TEST");
}

#[test]
fn full_cycle_offer_prediction_response_rows() {
    let s = Store::open_in_memory().unwrap();
    kamerton::arms::seed(&s, kamerton::arms::SEEDS_JSON).unwrap();
    s.conn()
        .execute(
            "INSERT INTO note(text, created_at) VALUES ('зазор желанием и действием', '2026-10-04T07:00:00Z')",
            [],
        )
        .unwrap();

    let now = 1_760_000_000; // fixed "today"
    let mut bandit = Bandit::new(
        &vec![(0.5, 0.2); 20],
        BanditConfig {
            seed: 11,
            ..BanditConfig::default()
        },
    );

    // Morning tick: an offer goes out, the trial row is on disk.
    let sel = morning_tick(&s, &mut bandit, now, 3).unwrap();
    let trial_id: i64 = s
        .conn()
        .query_row("SELECT id FROM trial ORDER BY id DESC LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let prob: f64 = s
        .conn()
        .query_row(
            "SELECT selection_prob FROM trial WHERE id=?1",
            [trial_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        (prob - sel.selection_prob).abs() < 1e-12,
        "propensity logged"
    );

    // The prediction is collected in conversation and stored.
    set_prediction(&s, trial_id, "к вечеру зазор уже").unwrap();
    let predicted: String = s
        .conn()
        .query_row("SELECT predicted FROM trial WHERE id=?1", [trial_id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(predicted, "к вечеру зазор уже");

    // Sleep quality lands in the (still blind) context.
    set_sleep_quality(&s, trial_id, 4).unwrap();

    // Blind until the response: reveal is part of record_immediate.
    assert!(kamerton::context::read_context(&s, trial_id)
        .unwrap()
        .is_none());

    // Evening: the response is recorded, the context opens.
    assert_eq!(pending_trial_for_response(&s).unwrap(), Some(trial_id));
    record_immediate(&s, trial_id, 0.6, now + 52_800).unwrap();
    let score: f64 = s
        .conn()
        .query_row(
            "SELECT score FROM response WHERE trial_id=?1 AND kind='immediate'",
            [trial_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!((score - 0.6).abs() < 1e-9);
    assert!(kamerton::context::read_context(&s, trial_id)
        .unwrap()
        .is_some());
    assert_eq!(pending_trial_for_response(&s).unwrap(), None);

    // The honesty valve can already read this trial once it matures.
    let matured = kamerton::honesty::load_matured_trials(&s, now + 4 * 86_400).unwrap();
    assert_eq!(matured.len(), 1);
    assert!((matured[0].reward - 0.2 * 0.6).abs() < 1e-9);
}

#[test]
fn send_message_escapes_and_sets_parse_mode() {
    let mut mock = MockTransport::new(vec![Step::Ok("{\"ok\":true,\"result\":{}}".into())]);
    {
        let mut poll = LongPoll::new(&mut mock, "TOKEN", None::<PathBuf>).unwrap();
        poll.send(777, "<a> & <b>").unwrap();
    }
    let (method, body) = &mock.calls[0];
    assert_eq!(method, "sendMessage");
    assert!(body.contains("parse_mode\":\"HTML"), "{body}");
    assert!(body.contains("&lt;a&gt; &amp; &lt;b&gt;"), "{body}");
    assert!(
        !body.contains("<a>"),
        "raw html must not leak into the body"
    );
}
