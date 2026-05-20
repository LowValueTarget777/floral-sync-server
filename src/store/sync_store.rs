use crate::protocol::{ChangesResponse, PushRequest, PushResponse, RevisionedChange, SyncChange};
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, Connection, DatabaseName, OptionalExtension};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::sync::watch;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("time parse error: {0}")]
    Time(#[from] chrono::ParseError),
    #[error("database lock was poisoned")]
    LockPoisoned,
}

#[derive(Clone)]
pub struct SyncStore {
    conn: Arc<Mutex<Connection>>,
    revision_events: watch::Sender<u64>,
}

impl SyncStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Arc::new(Mutex::new(Connection::open(path)?));
        let initial_revision = {
            let conn = conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            init_connection(&conn)?;
            current_revision(&conn)?
        };
        let (revision_events, _) = watch::channel(initial_revision);
        Ok(Self {
            conn,
            revision_events,
        })
    }

    pub fn revision(&self) -> Result<u64, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        current_revision(&conn)
    }

    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.conn)
    }

    pub fn subscribe_revisions(&self) -> watch::Receiver<u64> {
        self.revision_events.subscribe()
    }

    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.backup(DatabaseName::Main, path, None)?;
        Ok(())
    }

    pub fn restore_from(&self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let previous_revision = current_revision(&conn)?;
        let previous_notes = load_all_changes(&conn)?;
        conn.restore(
            DatabaseName::Main,
            path,
            None::<fn(rusqlite::backup::Progress)>,
        )?;

        if current_revision(&conn)? <= previous_revision {
            rebase_all_revisions(&conn, previous_revision)?;
        }

        let restored_notes = load_all_changes(&conn)?;
        let revision = emit_restore_tombstones(&mut conn, &previous_notes, &restored_notes)?;
        self.revision_events.send_replace(revision);
        Ok(())
    }

    pub fn changes_since(&self, since: u64) -> Result<ChangesResponse, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT revision, id, title, content, category, created_at, updated_at,
                   deleted_at, content_hash, device_id
            FROM notes
            WHERE revision > ?1
            ORDER BY revision ASC
            "#,
        )?;
        let changes = stmt
            .query_map(params![since], |row| {
                Ok(RevisionedChange {
                    revision: row.get::<_, i64>(0)? as u64,
                    note: SyncChange {
                        id: row.get(1)?,
                        title: row.get(2)?,
                        content: row.get(3)?,
                        category: row.get(4)?,
                        created_at: parse_row_time(row.get::<_, String>(5)?, 5)?,
                        updated_at: parse_row_time(row.get::<_, String>(6)?, 6)?,
                        deleted_at: row
                            .get::<_, Option<String>>(7)?
                            .map(|value| parse_row_time(value, 7))
                            .transpose()?,
                        content_hash: row.get(8)?,
                        device_id: row.get(9)?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ChangesResponse {
            revision: current_revision(&conn)?,
            changes,
        })
    }

    pub fn push(&self, request: PushRequest) -> Result<PushResponse, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let starting_revision = current_revision(&tx)?;

        for mut change in request.changes {
            if change.device_id.trim().is_empty() {
                change.device_id = request.device_id.clone();
            }

            if let Some(existing) = existing_change(&tx, &change.id)? {
                let server_time = change_time(&existing);
                let incoming_time = change_time(&change);
                if server_time > incoming_time
                    || (server_time == incoming_time && existing == change)
                {
                    continue;
                }
            }

            let revision = current_revision(&tx)? + 1;
            // The server is intentionally a latest-state store. It keeps tombstones and the
            // newest note body, while clients create local conflict backups when needed.
            store_change(&tx, &change, revision)?;
        }

        let revision = current_revision(&tx)?;
        tx.commit()?;
        if revision > starting_revision {
            self.revision_events.send_replace(revision);
        }
        Ok(PushResponse { revision })
    }
}

fn init_connection(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS notes (
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

        CREATE INDEX IF NOT EXISTS notes_revision_idx ON notes(revision);

        CREATE TABLE IF NOT EXISTS note_snapshots (
            snapshot_id INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            category TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT,
            content_hash TEXT NOT NULL,
            device_id TEXT NOT NULL,
            captured_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS note_snapshots_note_revision_idx
        ON note_snapshots(note_id, revision DESC);
        "#,
    )?;
    backfill_missing_snapshots(conn)?;
    Ok(())
}

fn store_change(
    tx: &rusqlite::Transaction<'_>,
    change: &SyncChange,
    revision: u64,
) -> Result<(), StoreError> {
    tx.execute(
        r#"
        INSERT INTO notes (
            id, title, content, category, created_at, updated_at, deleted_at,
            content_hash, device_id, revision
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            content = excluded.content,
            category = excluded.category,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            deleted_at = excluded.deleted_at,
            content_hash = excluded.content_hash,
            device_id = excluded.device_id,
            revision = excluded.revision
        "#,
        params![
            change.id,
            change.title,
            change.content,
            change.category,
            change.created_at.to_rfc3339(),
            change.updated_at.to_rfc3339(),
            change.deleted_at.map(|value| value.to_rfc3339()),
            change.content_hash,
            change.device_id,
            revision as i64,
        ],
    )?;
    insert_snapshot(tx, change, revision)?;
    Ok(())
}

fn load_all_changes(conn: &Connection) -> Result<HashMap<String, SyncChange>, StoreError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, title, content, category, created_at, updated_at, deleted_at, content_hash,
               device_id
        FROM notes
        ORDER BY id ASC
        "#,
    )?;
    let changes = stmt
        .query_map([], |row| {
            Ok(SyncChange {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                created_at: parse_row_time(row.get::<_, String>(4)?, 4)?,
                updated_at: parse_row_time(row.get::<_, String>(5)?, 5)?,
                deleted_at: row
                    .get::<_, Option<String>>(6)?
                    .map(|value| parse_row_time(value, 6))
                    .transpose()?,
                content_hash: row.get(7)?,
                device_id: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(changes
        .into_iter()
        .map(|change| (change.id.clone(), change))
        .collect())
}

fn rebase_all_revisions(conn: &Connection, offset: u64) -> Result<(), StoreError> {
    if offset == 0 {
        return Ok(());
    }

    conn.execute(
        "UPDATE notes SET revision = revision + ?1",
        params![offset as i64],
    )?;
    conn.execute(
        "UPDATE note_snapshots SET revision = revision + ?1",
        params![offset as i64],
    )?;
    Ok(())
}

fn emit_restore_tombstones(
    conn: &mut Connection,
    previous_notes: &HashMap<String, SyncChange>,
    restored_notes: &HashMap<String, SyncChange>,
) -> Result<u64, StoreError> {
    let mut missing_note_ids = previous_notes
        .keys()
        .filter(|note_id| !restored_notes.contains_key(*note_id))
        .cloned()
        .collect::<Vec<_>>();

    if missing_note_ids.is_empty() {
        return current_revision(conn);
    }

    missing_note_ids.sort();

    let tx = conn.transaction()?;
    let mut next_revision = current_revision(&tx)?;
    let restore_time = Utc::now();

    for note_id in missing_note_ids {
        let Some(previous_note) = previous_notes.get(&note_id) else {
            continue;
        };

        let mut tombstone = previous_note.clone();
        let deleted_at = std::cmp::max(change_time(previous_note), restore_time);
        tombstone.updated_at = std::cmp::max(tombstone.updated_at, deleted_at);
        tombstone.deleted_at = Some(deleted_at);

        next_revision += 1;
        store_change(&tx, &tombstone, next_revision)?;
    }

    tx.commit()?;
    current_revision(conn)
}

fn insert_snapshot(
    tx: &rusqlite::Transaction<'_>,
    change: &SyncChange,
    revision: u64,
) -> Result<(), StoreError> {
    tx.execute(
        r#"
        INSERT INTO note_snapshots (
            note_id, revision, title, content, category, created_at, updated_at,
            deleted_at, content_hash, device_id, captured_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
        params![
            change.id,
            revision as i64,
            change.title,
            change.content,
            change.category,
            change.created_at.to_rfc3339(),
            change.updated_at.to_rfc3339(),
            change.deleted_at.map(|value| value.to_rfc3339()),
            change.content_hash,
            change.device_id,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn backfill_missing_snapshots(conn: &Connection) -> Result<(), StoreError> {
    conn.execute(
        r#"
        INSERT INTO note_snapshots (
            note_id, revision, title, content, category, created_at, updated_at,
            deleted_at, content_hash, device_id, captured_at
        )
        SELECT
            notes.id,
            notes.revision,
            notes.title,
            notes.content,
            notes.category,
            notes.created_at,
            notes.updated_at,
            notes.deleted_at,
            notes.content_hash,
            notes.device_id,
            COALESCE(notes.deleted_at, notes.updated_at)
        FROM notes
        WHERE NOT EXISTS (
            SELECT 1
            FROM note_snapshots
            WHERE note_snapshots.note_id = notes.id
              AND note_snapshots.revision = notes.revision
        )
        "#,
        [],
    )?;
    Ok(())
}

fn existing_change(conn: &Connection, id: &str) -> Result<Option<SyncChange>, StoreError> {
    conn.query_row(
        r#"
        SELECT id, title, content, category, created_at, updated_at, deleted_at, content_hash,
               device_id
        FROM notes
        WHERE id = ?1
        "#,
        params![id],
        |row| {
            Ok(SyncChange {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                created_at: parse_row_time(row.get::<_, String>(4)?, 4)?,
                updated_at: parse_row_time(row.get::<_, String>(5)?, 5)?,
                deleted_at: row
                    .get::<_, Option<String>>(6)?
                    .map(|value| parse_row_time(value, 6))
                    .transpose()?,
                content_hash: row.get(7)?,
                device_id: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(StoreError::from)
}

fn current_revision(conn: &Connection) -> Result<u64, StoreError> {
    let revision: i64 =
        conn.query_row("SELECT COALESCE(MAX(revision), 0) FROM notes", [], |row| {
            row.get(0)
        })?;
    Ok(revision as u64)
}

fn change_time(change: &SyncChange) -> DateTime<Utc> {
    change.deleted_at.unwrap_or(change.updated_at)
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
    use super::SyncStore;
    use crate::protocol::{PushRequest, SyncChange};
    use chrono::{DateTime, Utc};
    use rusqlite::{params, Connection};

    #[test]
    fn stores_push_changes_and_returns_incremental_changes() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");
        let first = change("note-1", "First", "2026-05-18T08:00:00Z", None);

        let pushed = store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![first],
            })
            .expect("push change");
        let changes = store.changes_since(0).expect("load changes");

        assert_eq!(pushed.revision, 1);
        assert_eq!(changes.revision, 1);
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].note.id, "note-1");
    }

    #[test]
    fn push_and_changes_since_remain_compatible() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");

        assert_eq!(store.revision().expect("initial revision"), 0);

        let response = store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Title", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push");

        assert_eq!(response.revision, 1);
        assert_eq!(store.revision().expect("revision after push"), 1);

        let changes = store.changes_since(0).expect("changes");
        assert_eq!(changes.revision, 1);
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].note.id, "note-1");
    }

    #[test]
    fn keeps_newer_server_version_when_old_change_is_pushed_again() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");
        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "New", "2026-05-18T08:10:00Z", None)],
            })
            .expect("push newer change");
        store
            .push(PushRequest {
                device_id: "device-b".into(),
                changes: vec![change("note-1", "Old", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push older change");

        let changes = store.changes_since(0).expect("load changes");

        assert_eq!(changes.revision, 1);
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].note.title, "New");
    }

    #[test]
    fn replaying_the_same_change_does_not_advance_revision() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");
        let request = PushRequest {
            device_id: "device-a".into(),
            changes: vec![change("note-1", "Title", "2026-05-18T08:00:00Z", None)],
        };

        let first = store.push(request.clone()).expect("first push");
        let second = store.push(request).expect("second push");
        let later_changes = store.changes_since(first.revision).expect("later changes");

        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 1);
        assert_eq!(store.revision().expect("store revision"), 1);
        assert!(later_changes.changes.is_empty());
        assert_eq!(later_changes.revision, 1);
    }

    #[test]
    fn accepted_push_creates_note_snapshot_rows() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Title", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push");

        let conn = store.connection();
        let conn = conn.lock().expect("lock connection");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_snapshots WHERE note_id = ?1",
                params!["note-1"],
                |row| row.get(0),
            )
            .expect("snapshot count");

        assert_eq!(count, 1);
    }

    #[test]
    fn outdated_pushes_do_not_create_note_snapshot_rows() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "New", "2026-05-18T08:10:00Z", None)],
            })
            .expect("push newer change");
        store
            .push(PushRequest {
                device_id: "device-b".into(),
                changes: vec![change("note-1", "Old", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push older change");

        let conn = store.connection();
        let conn = conn.lock().expect("lock connection");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_snapshots WHERE note_id = ?1",
                params!["note-1"],
                |row| row.get(0),
            )
            .expect("snapshot count");

        assert_eq!(count, 1);
    }

    #[test]
    fn backup_to_captures_committed_wal_changes() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("sync.sqlite3");
        let backup_path = temp.path().join("backup").join("sync.sqlite3");
        let store = SyncStore::open(&db_path).expect("open store");

        {
            let conn = store.connection();
            let conn = conn.lock().expect("lock connection");
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;")
                .expect("enable wal");
        }

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Title", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push");

        store.backup_to(&backup_path).expect("backup database");

        let backup = Connection::open(&backup_path).expect("open backup");
        let count: i64 = backup
            .query_row("SELECT COUNT(*) FROM notes WHERE id = ?1", params!["note-1"], |row| {
                row.get(0)
            })
            .expect("backup note count");

        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn revision_subscribers_are_notified_when_push_advances_revision() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = SyncStore::open(temp.path().join("sync.sqlite3")).expect("open store");
        let mut revisions = store.subscribe_revisions();

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Title", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push note");

        revisions.changed().await.expect("revision change");
        assert_eq!(*revisions.borrow_and_update(), 1);
    }

    #[test]
    fn restore_from_replaces_live_database_contents() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("sync.sqlite3");
        let backup_path = temp.path().join("backup").join("sync.sqlite3");
        let store = SyncStore::open(&db_path).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Original", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push original note");
        store.backup_to(&backup_path).expect("backup database");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Updated", "2026-05-18T08:10:00Z", None)],
            })
            .expect("push updated note");

        let revision_before_restore = store.revision().expect("revision before restore");

        store.restore_from(&backup_path).expect("restore backup");

        let changes = store
            .changes_since(revision_before_restore)
            .expect("changes after restore");

        let conn = store.connection();
        let conn = conn.lock().expect("lock connection");
        let title: String = conn
            .query_row(
                "SELECT title FROM notes WHERE id = ?1",
                params!["note-1"],
                |row| row.get(0),
            )
            .expect("restored title");
        let revision: i64 = conn
            .query_row("SELECT revision FROM notes WHERE id = ?1", params!["note-1"], |row| {
                row.get(0)
            })
            .expect("restored revision");

        assert_eq!(title, "Original");
        assert_eq!(revision, 3);
        assert_eq!(changes.revision, 3);
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].note.id, "note-1");
        assert_eq!(changes.changes[0].note.title, "Original");
        assert!(changes.changes[0].note.deleted_at.is_none());
    }

    #[test]
    fn restore_from_replays_deleted_note_for_clients_with_newer_cursor() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("sync.sqlite3");
        let backup_path = temp.path().join("backup").join("sync.sqlite3");
        let store = SyncStore::open(&db_path).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "测试1", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push original note");
        store.backup_to(&backup_path).expect("backup database");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change(
                    "note-1",
                    "测试1",
                    "2026-05-18T08:10:00Z",
                    Some("2026-05-18T08:10:00Z"),
                )],
            })
            .expect("push tombstone");

        let revision_before_restore = store.revision().expect("revision before restore");

        store.restore_from(&backup_path).expect("restore backup");

        let changes = store
            .changes_since(revision_before_restore)
            .expect("changes after restore");

        assert_eq!(changes.revision, 3);
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].note.id, "note-1");
        assert_eq!(changes.changes[0].note.title, "测试1");
        assert!(changes.changes[0].note.deleted_at.is_none());
    }

    #[test]
    fn restore_from_emits_tombstones_for_notes_missing_from_backup() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("sync.sqlite3");
        let backup_path = temp.path().join("backup").join("sync.sqlite3");
        let store = SyncStore::open(&db_path).expect("open store");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-1", "Kept", "2026-05-18T08:00:00Z", None)],
            })
            .expect("push first note");
        store.backup_to(&backup_path).expect("backup database");

        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![change("note-2", "Removed", "2026-05-18T08:05:00Z", None)],
            })
            .expect("push second note");

        let revision_before_restore = store.revision().expect("revision before restore");

        store.restore_from(&backup_path).expect("restore backup");

        let changes = store
            .changes_since(revision_before_restore)
            .expect("changes after restore");

        assert_eq!(changes.revision, 4);
        assert_eq!(changes.changes.len(), 2);
        assert_eq!(changes.changes[0].note.id, "note-1");
        assert!(changes.changes[0].note.deleted_at.is_none());
        assert_eq!(changes.changes[1].note.id, "note-2");
        assert!(changes.changes[1].note.deleted_at.is_some());
    }

    fn change(id: &str, title: &str, updated_at: &str, deleted_at: Option<&str>) -> SyncChange {
        SyncChange {
            id: id.into(),
            title: title.into(),
            content: "body".into(),
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
}
