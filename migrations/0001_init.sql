-- Kamerton migration 0001_init (naryad Н1-03) — the OPERATOR-SIDE mirror
-- of the schema-as-code declaration in src/store/store.mlog.
--
-- Discipline (ADR-0183, naryad №593 variant B):
--   * The schema evolves ADDITIVE-ONLY: every statement here is
--     CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS, so this
--     file is idempotent — re-applying changes nothing.
--   * There is deliberately NO down-migration: the rollback path is
--     backup restore owned by the operating environment
--     (deploy/backup.sh). Never hand this file a DROP or ALTER.
--   * Apply with a plain sqlite3 client (invariant 6: the database is
--     readable and maintainable without the app):
--         sqlite3 /data/kamerton/kamerton.db < migrations/0001_init.sql
--   * DELTA vs the language replay (loud, §2): the mlog v0.28.1
--     schema-as-code parser silently drops the references(t.f)
--     modifier, so tables created by the language replay alone lack
--     the FOREIGN KEY clauses that this mirror carries. The language
--     naryad must fix the parse; until then the operator path is the
--     way to get the declared constraints. The mirror is otherwise
--     schema-identical to the replay (verified by .schema comparison
--     in the naryad worklog).
--
-- Foreign keys are declared inline; SQLite enforces them per
-- connection — turn the pragma on when working by hand:
--   PRAGMA foreign_keys = ON;

-- ── practice_session: the Mini App practice journal (Н2-03) ────────────
-- One row per practice session opened from the Mini App (the page is
-- delivered per ADR-0002 variant (а), private tunnel). kind:
-- 'pacer' | 'ppg' | 'sweep' (Н2-06). tempo_bpm: the session tempo in
-- breaths/min (Н2-04; 0.0 in the skeleton). steps_total/steps_done: the
-- protocol step plan and the completed-step count (Н2-04, «по две минуты
-- на темп»). step_marks: JSON array of the per-step records; from Н2-06
-- each record carries the step's RR segment {"step","tempo_bpm","rr",
-- "rr_reason"} — the sweep protocol walks the tempos with the pulse
-- recording on. rr_json: JSON array of RR intervals in ms (Н2-05, the
-- standalone PPG screen); rr_source: 'ppg' | 'manual' (Н2-05).
-- arm_id/trial_id: linkage to the day's proposal — NULL when the session
-- runs without one (0 is never stored).
CREATE TABLE IF NOT EXISTS practice_session (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT DEFAULT 'pacer',
    arm_id INTEGER REFERENCES arm(id),
    trial_id INTEGER REFERENCES trial(id),
    tempo_bpm REAL DEFAULT '0.0',
    steps_total INTEGER DEFAULT '0',
    steps_done INTEGER DEFAULT '0',
    step_marks TEXT DEFAULT '',
    rr_json TEXT DEFAULT '',
    rr_source TEXT DEFAULT '',
    session_date TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── sweep_result: the accepted runs of the resonance-finding protocol
--    (Н2-06) ──────────────────────────────────────────────────────────────
-- One row per ACCEPTED protocol run (the prereg gate: >= 6 valid steps
-- of 11). resonance_bpm: the PACED tempo of the max-power-fraction step
-- — the individual resonance frequency; peak_freq_bpm: the spectral peak
-- of the winning segment (the Факт band: near 0.1 Hz); power_fraction /
-- p_value: the №591 spectral verdict of the winning segment. The
-- reproducibility verdict compares against the previous row per the
-- preregistration: freq_delta_bpm <= 0.3 AND share_ratio in [0.5; 2.0]
-- → 'reproducible', otherwise 'divergent'; the first recorded run
-- carries '' (nothing to reproduce against yet). Invariant 6: a
-- third-party sqlite3 client reads the same rows.
CREATE TABLE IF NOT EXISTS sweep_result (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES practice_session(id),
    result_date TEXT DEFAULT '',
    resonance_bpm REAL DEFAULT '0.0',
    peak_freq_bpm REAL DEFAULT '0.0',
    power_fraction REAL DEFAULT '0.0',
    p_value REAL DEFAULT '1.0',
    steps_valid INTEGER DEFAULT '0',
    steps_total INTEGER DEFAULT '0',
    prev_resonance_bpm REAL DEFAULT '0.0',
    freq_delta_bpm REAL DEFAULT '0.0',
    share_ratio REAL DEFAULT '0.0',
    verdict TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── practice_text: the practice delivery of a proposal (Н2-09) ──────────
-- One row per methodology group: the 10-minute instruction the morning
-- proposal carries. The wave-1 gap (issue #63): the arm NAMES the
-- protocol, but nothing told the owner what to actually do — the M1
-- window burned every trial as a miss while the machine contour ran
-- clean. Seeded idempotently by seed_arms (UNIQUE(group_name) +
-- INSERT OR IGNORE), grounded in the same literature sources as the
-- arms (Lehrer & Gevirtz 2014; Gollwitzer & Sheeran 2006 / Oettingen;
-- Driskell, Copper & Moran 1994; Hendricks 2002 / Gendlin; Moszeik et
-- al. 2022). The proposal joins practice_text by arm.group_name; a
-- missing or empty row degrades LOUD to '(нет текста практики)'.
CREATE TABLE IF NOT EXISTS practice_text (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_name TEXT,
    body TEXT,
    active INTEGER DEFAULT '1',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── arm: the hand library (Н1-04 seeds rows) ────────────────────────────
CREATE TABLE IF NOT EXISTS arm (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    group_name TEXT,
    day_window TEXT,
    duration_min INTEGER DEFAULT '10',
    prior_mean REAL DEFAULT '0.0',
    prior_std REAL DEFAULT '1.0',
    active INTEGER DEFAULT '1',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── note: the owner's formulation of the desire ─────────────────────────
CREATE TABLE IF NOT EXISTS note (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    body TEXT,
    active INTEGER DEFAULT '1',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── trial: the unit of the system; a miss is a status STRING, never a
--    missing row; the prediction is stored BEFORE the trial (ADR-0001) ───
CREATE TABLE IF NOT EXISTS trial (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trial_date TEXT,
    day_window TEXT,
    arm_id INTEGER REFERENCES arm(id),
    note_id INTEGER REFERENCES note(id),
    status TEXT DEFAULT 'proposed',
    prediction_depth REAL DEFAULT '0.0',
    prediction_confidence REAL DEFAULT '0.0',
    moment_value REAL DEFAULT '0.0',
    moment_present INTEGER DEFAULT '0',
    moment_source TEXT DEFAULT '',
    next_day_value REAL DEFAULT '0.0',
    next_day_present INTEGER DEFAULT '0',
    action_value REAL DEFAULT '0.0',
    action_present INTEGER DEFAULT '0',
    reward REAL DEFAULT '0.0',
    free_note TEXT DEFAULT '',
    closed_at TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── response: raw responses; kind = moment | next_day | action ──────────
CREATE TABLE IF NOT EXISTS response (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trial_id INTEGER REFERENCES trial(id),
    kind TEXT,
    depth REAL DEFAULT '0.0',
    details TEXT DEFAULT '',
    created_at TEXT DEFAULT (datetime('now'))
);

-- ── context: blind until the response is logged (invariant 2) ───────────
CREATE TABLE IF NOT EXISTS context (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    trial_id INTEGER REFERENCES trial(id),
    moon_phase TEXT DEFAULT '',
    weather TEXT DEFAULT '',
    sound_mode TEXT DEFAULT '',
    sleep_quality REAL DEFAULT '0.0',
    logged_at TEXT DEFAULT (datetime('now'))
);

-- ── candidate: context features stay candidates until an ADR promotes
--    them; status is a string: candidate | promoted | rejected ───────────
CREATE TABLE IF NOT EXISTS candidate (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    kind TEXT DEFAULT 'non_periodic',
    status TEXT DEFAULT 'candidate',
    stat_value REAL DEFAULT '0.0',
    threshold REAL DEFAULT '0.0',
    tested_at TEXT DEFAULT '',
    note TEXT DEFAULT ''
);

-- ── indexes (identical to the language path's ensure_indexes()) ─────────
CREATE INDEX IF NOT EXISTS idx_trial_date ON trial(trial_date);
CREATE INDEX IF NOT EXISTS idx_trial_arm ON trial(arm_id);
CREATE INDEX IF NOT EXISTS idx_trial_status ON trial(status);
CREATE INDEX IF NOT EXISTS idx_response_trial ON response(trial_id);
CREATE INDEX IF NOT EXISTS idx_context_trial ON context(trial_id);
CREATE INDEX IF NOT EXISTS idx_candidate_name ON candidate(name);
CREATE INDEX IF NOT EXISTS idx_arm_active ON arm(active);
