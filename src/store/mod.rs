//! Storage layer: SQLite database, migrations and CSV export.
//!
//! Invariants served here (AGENTS.md): 6 — data outlives code, the plain
//! SQLite file must stay readable by any third-party client; 7 — data
//! never leaves the owner's machine.

pub mod csv;
pub mod time;

use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::Connection;

pub const TABLES: [&str; 6] = ["note", "arm", "trial", "response", "context", "candidate"];

/// Storage error: SQL or I/O (export writes files).
#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sql(e) => write!(f, "storage sql error: {e}"),
            Error::Io(e) => write!(f, "storage io error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Sql(e) => Some(e),
            Error::Io(e) => Some(e),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sql(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

struct Migration {
    version: i64,
    up: &'static str,
    down: &'static str,
}

const MIGRATIONS: [Migration; 1] = [Migration {
    version: 1,
    up: include_str!("../../migrations/0001_init.sql"),
    down: include_str!("../../migrations/0001_rollback.sql"),
}];

/// Owns the SQLite connection and keeps the schema at the latest version.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (or creates) the database file and applies pending migrations.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let mut store = Store { conn };
        store.apply_migrations()?;
        Ok(store)
    }

    /// Applies every pending migration. Idempotent: a second call on an
    /// up-to-date database applies nothing.
    pub fn apply_migrations(&mut self) -> rusqlite::Result<usize> {
        let mut applied = 0;
        for m in MIGRATIONS.iter() {
            if self.user_version()? < m.version {
                self.conn.execute_batch(&format!(
                    "BEGIN; {} PRAGMA user_version = {}; COMMIT;",
                    m.up, m.version
                ))?;
                applied += 1;
            }
        }
        Ok(applied)
    }

    /// Rolls the schema back down to `target`. Idempotent: rolling back a
    /// database already at or below `target` does nothing.
    pub fn rollback_to(&mut self, target: i64) -> rusqlite::Result<()> {
        for m in MIGRATIONS.iter().rev() {
            if m.version > target && self.user_version()? > target {
                self.conn.execute_batch(&format!(
                    "BEGIN; {} PRAGMA user_version = {}; COMMIT;",
                    m.down,
                    m.version - 1
                ))?;
            }
        }
        Ok(())
    }

    /// Current schema version (`PRAGMA user_version`).
    pub fn user_version(&self) -> rusqlite::Result<i64> {
        self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))
    }

    /// Read-only access to the connection for callers that need raw SQL.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Column names of `table`, from `PRAGMA table_info`.
    pub fn column_names(&self, table: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        rows.collect()
    }

    /// Exports all six tables to `{dir}/{table}.csv` (RFC 4180, header row,
    /// rows ordered by `id`). NULL values become empty fields.
    pub fn export_csv(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        fs::create_dir_all(dir)?;
        let mut written = Vec::with_capacity(TABLES.len());
        for table in TABLES {
            let path = dir.join(format!("{table}.csv"));
            let file = File::create(&path)?;
            let mut w = BufWriter::new(file);

            let cols = self.column_names(table)?;
            csv::write_row(&mut w, &cols)?;

            let mut stmt = self
                .conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY id"))?;
            let ncols = stmt.column_count();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let mut fields = Vec::with_capacity(ncols);
                for i in 0..ncols {
                    fields.push(value_to_string(row.get_ref(i)?)?);
                }
                csv::write_row(&mut w, &fields)?;
            }
            w.flush()?;
            written.push(path);
        }
        Ok(written)
    }
}

fn value_to_string(v: ValueRef<'_>) -> rusqlite::Result<String> {
    let s = match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(b) => String::from_utf8_lossy(b).into_owned(),
        ValueRef::Blob(b) => b.iter().map(|x| format!("{x:02x}")).collect(),
    };
    Ok(s)
}
