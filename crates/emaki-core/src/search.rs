//! Full-text search across every conversation, live and archived alike.
//!
//! SQLite FTS5, one document per *item*: a prompt, a reply paragraph, a
//! thought, a tool call. Sync is incremental on (size, mtime). The database is
//! `~/.emaki/search.db`, separate from the Python daemon's `index.db` so the
//! two never fight over a schema version.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::adapters;
use crate::model::{Item, Round, Session};
use crate::paths;
use crate::transcript::SessionRef;

pub const SCHEMA_VERSION: i32 = 3;
pub const MAX_DOC_CHARS: usize = 4000;

pub fn db_path() -> PathBuf {
    paths::root().join("search.db")
}

static TERM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+)"|(\S+)"#).unwrap());
static WORDY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w*]+").unwrap());

/// Turn what someone typed into a valid FTS5 MATCH expression. Every term is
/// quoted (a literal phrase); a trailing `*` survives as a prefix match.
pub fn fts_query(text: &str) -> String {
    let mut parts = Vec::new();
    for cap in TERM.captures_iter(text) {
        let token = cap.get(1).or(cap.get(2)).map(|m| m.as_str().trim()).unwrap_or("");
        if token.is_empty() {
            continue;
        }
        let prefix = token.ends_with('*');
        let cleaned = WORDY.replace_all(token, " ").trim().trim_end_matches('*').trim().to_string();
        if cleaned.is_empty() {
            continue;
        }
        let escaped = cleaned.replace('"', "\"\"");
        parts.push(if prefix { format!("\"{escaped}\"*") } else { format!("\"{escaped}\"") });
    }
    parts.join(" ")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Match {
    pub round: i64,
    pub ts: String,
    pub kind: String,
    /// Highlight delimited with `\x02` / `\x03`, never markup.
    pub snippet: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionHits {
    pub agent: String,
    pub id: String,
    pub title: String,
    pub project: String,
    pub updated: String,
    pub archived: bool,
    pub path: String,
    pub hits: usize,
    pub matches: Vec<Match>,
    pub rank: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Results {
    pub query: String,
    pub sessions: Vec<SessionHits>,
    pub total: usize,
    pub truncated: bool,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub indexed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub documents: usize,
    pub seconds: f64,
}

pub struct SearchIndex {
    conn: Connection,
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl SearchIndex {
    pub fn open() -> rusqlite::Result<Self> {
        Self::open_at(&db_path())
    }

    pub fn open_at(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let me = Self { conn };
        me.ensure_schema()?;
        Ok(me)
    }

    fn ensure_schema(&self) -> rusqlite::Result<()> {
        let version: i32 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            self.conn.execute_batch("DROP TABLE IF EXISTS docs; DROP TABLE IF EXISTS sources;")?;
        }
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sources (
                key        TEXT PRIMARY KEY,
                agent      TEXT,
                session_id TEXT,
                path       TEXT,
                size       INTEGER,
                mtime      REAL,
                project    TEXT,
                title      TEXT,
                cwd        TEXT,
                updated    TEXT,
                archived   INTEGER DEFAULT 0,
                rounds     INTEGER DEFAULT 0,
                indexed_at REAL
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
                key UNINDEXED,
                round_index UNINDEXED,
                ts UNINDEXED,
                kind UNINDEXED,
                body,
                tokenize = "unicode61 remove_diacritics 2"
            );
            "#,
        )?;
        self.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    fn key(r: &SessionRef) -> String {
        format!("{}:{}", r.agent.as_str(), r.session_id)
    }

    pub fn needs_reindex(&self, r: &SessionRef) -> bool {
        let row: Option<(i64, f64)> = self
            .conn
            .query_row("SELECT size, mtime FROM sources WHERE key=?1", params![Self::key(r)], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()
            .unwrap_or(None);
        match row {
            None => true,
            Some((size, mtime)) => size as u64 != r.size || (mtime - r.mtime).abs() > 0.001,
        }
    }

    /// (Re)index one session. Returns the number of documents written.
    pub fn index_ref(&mut self, r: &SessionRef, session: Option<&Session>) -> rusqlite::Result<usize> {
        let built;
        let session = match session {
            Some(s) => s,
            None => {
                built = adapters::for_agent(r.agent).load(r);
                &built
            }
        };
        let docs = documents(session);
        let key = Self::key(r);
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM docs WHERE key=?1", params![key])?;
        {
            let mut stmt = tx.prepare("INSERT INTO docs(key, round_index, ts, kind, body) VALUES (?1,?2,?3,?4,?5)")?;
            for d in &docs {
                stmt.execute(params![key, d.0 as i64, d.1, d.2, d.3])?;
            }
        }
        tx.execute(
            r#"INSERT INTO sources(key, agent, session_id, path, size, mtime, project, title, cwd, updated, archived, rounds, indexed_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
               ON CONFLICT(key) DO UPDATE SET
                 agent=excluded.agent, session_id=excluded.session_id, path=excluded.path, size=excluded.size, mtime=excluded.mtime,
                 project=excluded.project, title=excluded.title, cwd=excluded.cwd,
                 updated=excluded.updated, archived=excluded.archived,
                 rounds=excluded.rounds, indexed_at=excluded.indexed_at"#,
            params![
                key,
                r.agent.as_str(),
                r.session_id,
                r.path.to_string_lossy(),
                r.size as i64,
                r.mtime,
                r.project(),
                if session.title.is_empty() { r.title.clone() } else { session.title.clone() },
                r.cwd,
                if r.updated.is_empty() { session.updated.clone() } else { r.updated.clone() },
                r.archived as i64,
                session.rounds.len() as i64,
                now(),
            ],
        )?;
        tx.commit()?;
        Ok(docs.len())
    }

    /// Bring the index up to date. Only changed sessions are re-read.
    pub fn sync(&mut self, refs: &[SessionRef], force: bool) -> SyncReport {
        let started = now();
        let mut report = SyncReport::default();
        for r in refs {
            if !force && !self.needs_reindex(r) {
                report.skipped += 1;
                continue;
            }
            match self.index_ref(r, None) {
                Ok(n) => {
                    report.indexed += 1;
                    report.documents += n;
                }
                Err(_) => report.failed += 1,
            }
        }
        self.prune(refs);
        report.seconds = now() - started;
        report
    }

    fn prune(&mut self, refs: &[SessionRef]) {
        let alive: std::collections::HashSet<String> = refs.iter().map(Self::key).collect();
        let known: Vec<String> = self
            .conn
            .prepare("SELECT key FROM sources")
            .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0)).map(|it| it.filter_map(Result::ok).collect()))
            .unwrap_or_default();
        let stale: Vec<&String> = known.iter().filter(|k| !alive.contains(*k)).collect();
        if stale.is_empty() {
            return;
        }
        if let Ok(tx) = self.conn.transaction() {
            for k in stale {
                let _ = tx.execute("DELETE FROM docs WHERE key=?1", params![k]);
                let _ = tx.execute("DELETE FROM sources WHERE key=?1", params![k]);
            }
            let _ = tx.commit();
        }
    }

    /// Ranked matches, grouped by conversation.
    pub fn search(&self, text: &str, limit: usize, per_session: usize) -> Results {
        let query = fts_query(text);
        let mut out = Results { query: text.into(), ..Default::default() };
        if query.is_empty() {
            return out;
        }
        let rows: Vec<(String, i64, String, String, String, f64)> = match self.conn.prepare(
            r#"SELECT d.key, d.round_index, d.ts, d.kind,
                      snippet(docs, 4, char(2), char(3), '…', 14) AS snip,
                      bm25(docs) AS rank
               FROM docs d WHERE docs MATCH ?1 ORDER BY rank LIMIT ?2"#,
        ) {
            Ok(mut stmt) => match stmt.query_map(params![query, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            }) {
                Ok(it) => it.filter_map(Result::ok).collect(),
                Err(_) => {
                    out.error = "could not parse that query".into();
                    return out;
                }
            },
            Err(_) => {
                out.error = "could not parse that query".into();
                return out;
            }
        };
        let mut grouped: Vec<SessionHits> = Vec::new();
        for (key, round, ts, kind, snip, rank) in rows {
            let entry = match grouped.iter_mut().position(|g| format!("{}:{}", g.agent, g.id) == key) {
                Some(i) => &mut grouped[i],
                None => {
                    let meta: Option<(String, String, String, String, String, i64, String)> = self
                        .conn
                        .query_row(
                            "SELECT agent, session_id, title, project, updated, archived, path FROM sources WHERE key=?1",
                            params![key],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
                        )
                        .optional()
                        .unwrap_or(None);
                    let Some((agent, id, title, project, updated, archived, path)) = meta else { continue };
                    grouped.push(SessionHits {
                        agent,
                        title: if title.is_empty() { id.chars().take(8).collect() } else { title },
                        id,
                        project,
                        updated,
                        archived: archived != 0,
                        path,
                        rank,
                        ..Default::default()
                    });
                    grouped.last_mut().unwrap()
                }
            };
            entry.hits += 1;
            entry.rank = entry.rank.min(rank);
            if entry.matches.len() < per_session {
                entry.matches.push(Match { round, ts, kind, snippet: snip });
            }
        }
        grouped.sort_by(|a, b| a.rank.partial_cmp(&b.rank).unwrap_or(std::cmp::Ordering::Equal).then(b.hits.cmp(&a.hits)));
        out.truncated = grouped.iter().map(|g| g.hits).sum::<usize>() >= limit;
        out.total = grouped.iter().map(|g| g.hits).sum();
        out.sessions = grouped;
        out
    }

    pub fn counts(&self) -> (i64, i64) {
        let sources = self.conn.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0)).unwrap_or(0);
        let docs = self.conn.query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0)).unwrap_or(0);
        (sources, docs)
    }
}

fn clip_chars(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect()
}

/// (round_index, ts, kind, body) for everything worth searching.
fn documents(session: &Session) -> Vec<(usize, String, String, String)> {
    let mut out = Vec::new();
    fn emit(rounds: &[Round], prefix: &str, out: &mut Vec<(usize, String, String, String)>) {
        for rnd in rounds {
            if !rnd.prompt.is_empty() {
                out.push((rnd.index, rnd.ts.clone(), format!("{prefix}prompt"), clip_chars(&rnd.prompt, MAX_DOC_CHARS)));
            }
            for item in &rnd.items {
                match item {
                    Item::Text { ts, md, .. } if !md.trim().is_empty() => {
                        out.push((rnd.index, ts.clone(), format!("{prefix}reply"), clip_chars(md, MAX_DOC_CHARS)))
                    }
                    Item::Thinking { ts, md, .. } if !md.trim().is_empty() => {
                        out.push((rnd.index, ts.clone(), format!("{prefix}thinking"), clip_chars(md, MAX_DOC_CHARS)))
                    }
                    Item::Notice { ts, text, variant } if !text.trim().is_empty() => {
                        out.push((rnd.index, ts.clone(), format!("{prefix}notice"), clip_chars(&variant.said(text), 500)))
                    }
                    Item::Tool(call) => {
                        let body = tool_text(call);
                        if !body.trim().is_empty() {
                            out.push((rnd.index, call.ts.clone(), format!("{prefix}tool"), clip_chars(&body, MAX_DOC_CHARS)));
                        }
                        if !call.subagent.is_empty() {
                            emit(&call.subagent, "subagent:", out);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    emit(&session.rounds, "", &mut out);
    out
}

fn tool_text(call: &crate::model::ToolCall) -> String {
    let mut parts = vec![call.name.clone(), call.subject.clone()];
    for key in ["command", "pattern", "query", "url", "description", "prompt", "file_path"] {
        if let Some(v) = call.input.get(key).and_then(serde_json::Value::as_str) {
            if !v.trim().is_empty() {
                parts.push(v.to_string());
            }
        }
    }
    if !call.explanation.is_empty() {
        parts.push(call.explanation.clone());
    }
    for chunk in [&call.stdout, &call.stderr, &call.result_text] {
        if !chunk.trim().is_empty() {
            parts.push(clip_chars(chunk, MAX_DOC_CHARS));
            break;
        }
    }
    parts.join("\n")
}
