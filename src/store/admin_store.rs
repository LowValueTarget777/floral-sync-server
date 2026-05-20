use crate::store::StoreError;
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde::Serialize;
use std::{path::Path, sync::{Arc, Mutex}};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminOverview {
    pub latest_revision: u64,
    pub note_count: u64,
    pub deleted_note_count: u64,
    pub category_count: u64,
    pub latest_snapshot_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteStateFilter {
    #[default]
    All,
    Active,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteListQuery {
    pub page: u64,
    pub page_size: u64,
    pub search: Option<String>,
    pub category: Option<String>,
    pub state: NoteStateFilter,
}

impl Default for NoteListQuery {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 50,
            search: None,
            category: None,
            state: NoteStateFilter::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteListPage {
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub notes: Vec<NoteListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteListItem {
    pub id: String,
    pub title: String,
    pub category: String,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub device_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    pub id: String,
    pub title: String,
    pub content: String,
    pub category: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub content_hash: String,
    pub device_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSnapshot {
    pub snapshot_id: i64,
    pub note_id: String,
    pub revision: u64,
    pub title: String,
    pub content: String,
    pub category: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub content_hash: String,
    pub device_id: String,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDownload {
    pub file_name: String,
    pub markdown: String,
}

#[derive(Clone)]
pub struct AdminStore {
    conn: Arc<Mutex<Connection>>,
}

impl AdminStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn open_shared(conn: Arc<Mutex<Connection>>) -> Result<Self, StoreError> {
        Ok(Self { conn })
    }

    pub fn overview(&self) -> Result<AdminOverview, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let (latest_revision, note_count, deleted_note_count, category_count): (i64, i64, i64, i64) =
            conn.query_row(
                r#"
                SELECT
                    COALESCE(MAX(revision), 0),
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END), 0),
                    COUNT(DISTINCT NULLIF(TRIM(category), ''))
                FROM notes
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        let latest_snapshot_at = conn
            .query_row("SELECT MAX(captured_at) FROM note_snapshots", [], |row| {
                row.get::<_, Option<String>>(0)
            })?
            .map(parse_db_time)
            .transpose()?;

        Ok(AdminOverview {
            latest_revision: latest_revision as u64,
            note_count: note_count as u64,
            deleted_note_count: deleted_note_count as u64,
            category_count: category_count as u64,
            latest_snapshot_at,
        })
    }

    pub fn note_list(&self, query: &NoteListQuery) -> Result<NoteListPage, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let page = query.page.max(1);
        let page_size = query.page_size.clamp(1, 200);
        let search = normalize_optional(query.search.as_deref());
        let search_pattern = search.as_ref().map(|value| format!("%{value}%"));
        let category = normalize_optional(query.category.as_deref());
        let state = query.state.sql_value();
        let offset = page
            .saturating_sub(1)
            .saturating_mul(page_size)
            .min(i64::MAX as u64) as i64;

        let total: i64 = conn.query_row(
            r#"
            SELECT COUNT(*)
            FROM notes
            WHERE (?1 IS NULL OR title LIKE ?2 OR content LIKE ?2)
              AND (?3 IS NULL OR TRIM(category) = ?3)
              AND (
                    ?4 = 'all'
                    OR (?4 = 'active' AND deleted_at IS NULL)
                    OR (?4 = 'deleted' AND deleted_at IS NOT NULL)
                  )
            "#,
            params![
                search.as_deref(),
                search_pattern.as_deref(),
                category.as_deref(),
                state,
            ],
            |row| row.get(0),
        )?;

        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, TRIM(category), updated_at, deleted_at, device_id, revision
            FROM notes
            WHERE (?1 IS NULL OR title LIKE ?2 OR content LIKE ?2)
              AND (?3 IS NULL OR TRIM(category) = ?3)
              AND (
                    ?4 = 'all'
                    OR (?4 = 'active' AND deleted_at IS NULL)
                    OR (?4 = 'deleted' AND deleted_at IS NOT NULL)
                  )
            ORDER BY revision DESC, updated_at DESC, id ASC
            LIMIT ?5 OFFSET ?6
            "#,
        )?;
        let notes = stmt
            .query_map(
                params![
                    search.as_deref(),
                    search_pattern.as_deref(),
                    category.as_deref(),
                    state,
                    page_size as i64,
                    offset,
                ],
                |row| {
                    Ok(NoteListItem {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        category: row.get(2)?,
                        updated_at: parse_row_time(row.get::<_, String>(3)?, 3)?,
                        deleted_at: row
                            .get::<_, Option<String>>(4)?
                            .map(parse_db_time)
                            .transpose()
                            .map_err(|error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    4,
                                    Type::Text,
                                    Box::new(error),
                                )
                            })?,
                        device_id: row.get(5)?,
                        revision: row.get::<_, i64>(6)? as u64,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(NoteListPage {
            page,
            page_size,
            total: total as u64,
            notes,
        })
    }

    pub fn note_detail(&self, note_id: &str) -> Result<Option<NoteDetail>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            r#"
            SELECT id, title, content, TRIM(category), created_at, updated_at, deleted_at,
                   content_hash, device_id, revision
            FROM notes
            WHERE id = ?1
            "#,
            params![note_id],
            |row| {
                Ok(NoteDetail {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    content: row.get(2)?,
                    category: row.get(3)?,
                    created_at: parse_row_time(row.get::<_, String>(4)?, 4)?,
                    updated_at: parse_row_time(row.get::<_, String>(5)?, 5)?,
                    deleted_at: row
                        .get::<_, Option<String>>(6)?
                        .map(parse_db_time)
                        .transpose()
                        .map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                Type::Text,
                                Box::new(error),
                            )
                        })?,
                    content_hash: row.get(7)?,
                    device_id: row.get(8)?,
                    revision: row.get::<_, i64>(9)? as u64,
                })
            },
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn note_history(&self, note_id: &str) -> Result<Vec<NoteSnapshot>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT snapshot_id, note_id, revision, title, content, TRIM(category), created_at,
                   updated_at, deleted_at, content_hash, device_id, captured_at
            FROM note_snapshots
            WHERE note_id = ?1
            ORDER BY revision DESC, snapshot_id DESC
            "#,
        )?;
        let history = stmt
            .query_map(params![note_id], |row| {
            Ok(NoteSnapshot {
                snapshot_id: row.get(0)?,
                note_id: row.get(1)?,
                revision: row.get::<_, i64>(2)? as u64,
                title: row.get(3)?,
                content: row.get(4)?,
                category: row.get(5)?,
                created_at: parse_row_time(row.get::<_, String>(6)?, 6)?,
                updated_at: parse_row_time(row.get::<_, String>(7)?, 7)?,
                deleted_at: row
                    .get::<_, Option<String>>(8)?
                    .map(parse_db_time)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            8,
                            Type::Text,
                            Box::new(error),
                        )
                    })?,
                content_hash: row.get(9)?,
                device_id: row.get(10)?,
                captured_at: parse_row_time(row.get::<_, String>(11)?, 11)?,
            })
        })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(history)
    }

    pub fn markdown_download(&self, note_id: &str) -> Result<Option<MarkdownDownload>, StoreError> {
        let Some(note) = self.note_detail(note_id)? else {
            return Ok(None);
        };

        let heading = if note.title.trim().is_empty() {
            note.id.as_str()
        } else {
            note.title.as_str()
        };
        let mut markdown = format!("# {heading}\n\n");
        if !note.category.trim().is_empty() {
            markdown.push_str(&format!("Category: {}\n", note.category));
        }
        markdown.push_str(&format!("Updated: {}\n", note.updated_at.to_rfc3339()));
        if let Some(deleted_at) = note.deleted_at {
            markdown.push_str(&format!("Deleted: {}\n", deleted_at.to_rfc3339()));
        }
        markdown.push_str(&format!("Revision: {}\n", note.revision));
        markdown.push_str(&format!("Device: {}\n\n", note.device_id));
        markdown.push_str("---\n\n");
        markdown.push_str(&note.content);
        if !markdown.ends_with('\n') {
            markdown.push('\n');
        }

        Ok(Some(MarkdownDownload {
            file_name: format!("{}.md", sanitize_file_stem(heading, &note.id)),
            markdown,
        }))
    }
}

impl NoteStateFilter {
    fn sql_value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Deleted => "deleted",
        }
    }
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn sanitize_file_stem(title: &str, fallback_id: &str) -> String {
    let mut stem = String::new();
    let mut last_was_dash = false;
    for ch in title.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            last_was_dash = false;
            Some(ch.to_ascii_lowercase())
        } else if ch == ' ' || ch == '-' || ch == '_' {
            if last_was_dash {
                None
            } else {
                last_was_dash = true;
                Some('-')
            }
        } else {
            None
        };
        if let Some(ch) = mapped {
            stem.push(ch);
        }
    }

    let stem = stem.trim_matches('-');
    if stem.is_empty() {
        fallback_id.to_string()
    } else {
        stem.to_string()
    }
}

fn parse_db_time(value: String) -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339(&value)?.with_timezone(&Utc))
}

fn parse_row_time(value: String, column: usize) -> rusqlite::Result<DateTime<Utc>> {
    parse_db_time(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
    })
}

#[cfg(test)]
mod tests {
    use super::{AdminStore, NoteListQuery};
    use crate::{
        protocol::{PushRequest, SyncChange},
        store::SyncStore,
    };
    use chrono::{DateTime, Utc};
    use rusqlite::{params, Connection};
    use std::path::Path;

    #[test]
    fn note_list_query_returns_active_and_deleted_notes() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![
                    change("note-1", "Active", "2026-05-18T08:00:00Z", None),
                    change(
                        "note-2",
                        "Deleted",
                        "2026-05-18T08:05:00Z",
                        Some("2026-05-18T08:05:00Z"),
                    ),
                ],
            })
            .expect("push");

        let admin = AdminStore::open_shared(store.connection()).expect("admin store");
        let page = admin
            .note_list(&NoteListQuery {
                page: 1,
                page_size: 10,
                ..NoteListQuery::default()
            })
            .expect("note list");

        assert_eq!(page.total, 2);
        assert_eq!(page.notes.len(), 2);
        assert!(page.notes.iter().any(|note| note.id == "note-1" && note.deleted_at.is_none()));
        assert!(page.notes.iter().any(|note| note.id == "note-2" && note.deleted_at.is_some()));
    }

    #[test]
    fn note_history_returns_snapshots_newest_first() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "First", "2026-05-18T08:00:00Z", None)],
            })
            .expect("first push");
        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Second", "2026-05-18T08:10:00Z", None)],
            })
            .expect("second push");

        let admin = AdminStore::open_shared(store.connection()).expect("admin store");
        let history = admin.note_history("note-1").expect("note history");

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].title, "Second");
        assert_eq!(history[0].revision, 2);
        assert_eq!(history[1].title, "First");
        assert_eq!(history[1].revision, 1);
    }

    #[test]
    fn opening_legacy_db_backfills_snapshot_history_and_overview() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("sync.sqlite3");
        create_legacy_db(
            &db_path,
            &[legacy_note(
                "note-1",
                "Legacy Title",
                " Body category ",
                7,
                "2026-05-18T08:10:00Z",
                None,
            )],
        );

        let store = SyncStore::open(&db_path).expect("open upgraded store");
        let admin = AdminStore::open_shared(store.connection()).expect("admin store");

        let history = admin.note_history("note-1").expect("note history");
        let overview = admin.overview().expect("overview");

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].revision, 7);
        assert_eq!(history[0].title, "Legacy Title");
        assert_eq!(history[0].category, "Body category");
        assert_eq!(
            history[0].captured_at,
            parse_time("2026-05-18T08:10:00Z")
        );
        assert_eq!(overview.latest_revision, 7);
        assert_eq!(overview.note_count, 1);
        assert_eq!(overview.category_count, 1);
        assert_eq!(
            overview.latest_snapshot_at,
            Some(parse_time("2026-05-18T08:10:00Z"))
        );
    }

    #[test]
    fn admin_category_counts_and_filtering_use_trimmed_values() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("sync.sqlite3");
        create_legacy_db(
            &db_path,
            &[
                legacy_note(
                    "note-1",
                    "First",
                    "Work",
                    1,
                    "2026-05-18T08:00:00Z",
                    None,
                ),
                legacy_note(
                    "note-2",
                    "Second",
                    " Work",
                    2,
                    "2026-05-18T08:05:00Z",
                    None,
                ),
                legacy_note(
                    "note-3",
                    "Third",
                    "Work ",
                    3,
                    "2026-05-18T08:10:00Z",
                    None,
                ),
                legacy_note(
                    "note-4",
                    "Fourth",
                    "   ",
                    4,
                    "2026-05-18T08:15:00Z",
                    None,
                ),
            ],
        );

        let store = SyncStore::open(&db_path).expect("open upgraded store");
        let admin = AdminStore::open_shared(store.connection()).expect("admin store");

        let overview = admin.overview().expect("overview");
        let page = admin
            .note_list(&NoteListQuery {
                page: 1,
                page_size: 10,
                category: Some(" Work ".into()),
                ..NoteListQuery::default()
            })
            .expect("filtered note list");

        assert_eq!(overview.category_count, 1);
        assert_eq!(page.total, 3);
        assert_eq!(page.notes.len(), 3);
        assert!(page.notes.iter().all(|note| note.category == "Work"));
    }

    fn change(id: &str, title: &str, updated_at: &str, deleted_at: Option<&str>) -> SyncChange {
        SyncChange {
            id: id.into(),
            title: title.into(),
            content: format!("{title} body"),
            category: String::new(),
            created_at: parse_time("2026-05-18T08:00:00Z"),
            updated_at: parse_time(updated_at),
            deleted_at: deleted_at.map(parse_time),
            content_hash: format!("{id}:{title}"),
            device_id: "device-a".into(),
        }
    }

    fn parse_time(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("time")
            .with_timezone(&Utc)
    }

    fn create_legacy_db(path: &Path, notes: &[LegacyNote<'_>]) {
        let conn = Connection::open(path).expect("open legacy db");
        conn.execute_batch(
            r#"
            CREATE TABLE notes (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                category TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT,
                content_hash TEXT NOT NULL,
                device_id TEXT NOT NULL,
                revision INTEGER NOT NULL
            );

            CREATE INDEX notes_revision_idx ON notes(revision);
            "#,
        )
        .expect("create legacy notes table");

        for note in notes {
            conn.execute(
                r#"
                INSERT INTO notes (
                    id, title, content, category, created_at, updated_at, deleted_at,
                    content_hash, device_id, revision
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    note.id,
                    note.title,
                    format!("{} body", note.title),
                    note.category,
                    "2026-05-18T08:00:00Z",
                    note.updated_at,
                    note.deleted_at,
                    format!("{}:{}", note.id, note.title),
                    "legacy-device",
                    note.revision,
                ],
            )
            .expect("insert legacy note");
        }
    }

    fn legacy_note<'a>(
        id: &'a str,
        title: &'a str,
        category: &'a str,
        revision: i64,
        updated_at: &'a str,
        deleted_at: Option<&'a str>,
    ) -> LegacyNote<'a> {
        LegacyNote {
            id,
            title,
            category,
            revision,
            updated_at,
            deleted_at,
        }
    }

    struct LegacyNote<'a> {
        id: &'a str,
        title: &'a str,
        category: &'a str,
        revision: i64,
        updated_at: &'a str,
        deleted_at: Option<&'a str>,
    }
}
