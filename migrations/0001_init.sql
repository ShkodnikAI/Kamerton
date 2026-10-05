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
