use rusqlite::Connection;
use secall_core::store::Database;

#[test]
fn migrate_v8_to_v9() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("migration.sqlite");

    let conn = Connection::open(&db_path).expect("open sqlite");
    conn.execute_batch(
        "
        CREATE TABLE config (
            key   TEXT PRIMARY KEY,
            value TEXT
        );
        INSERT INTO config(key, value) VALUES ('schema_version', '8');
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY
        );
        ",
    )
    .expect("seed v8 schema");
    drop(conn);

    let db = Database::open(&db_path).expect("migrate to v9");
    let schema_version: String = db
        .conn()
        .query_row(
            "SELECT value FROM config WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("schema version");
    let wiki_vectors_exists: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='wiki_vectors'",
            [],
            |row| row.get(0),
        )
        .expect("wiki_vectors exists");

    // v8 → … → v12 마이그레이션 체인 — v12 가 벡터 중복 정리 + UNIQUE 인덱스를
    // 추가했으므로 Database::open 후 schema_version 은 최신값(12)이어야 한다.
    // wiki_vectors 테이블은 v9 에서 도입된 후 유지된다.
    assert_eq!(schema_version, "12");
    assert_eq!(wiki_vectors_exists, 1);
}

#[test]
fn migrate_v11_to_v12_dedups_vectors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("migration.sqlite");

    // 동시 sync 가 같은 chunk 를 두 번 넣은 v11 DB
    let conn = Connection::open(&db_path).expect("open sqlite");
    conn.execute_batch(
        "
        CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT);
        INSERT INTO config(key, value) VALUES ('schema_version', '11');
        CREATE TABLE sessions (id TEXT PRIMARY KEY);
        CREATE TABLE turn_vectors (
            id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
            turn_index INTEGER NOT NULL, chunk_seq INTEGER NOT NULL,
            model TEXT NOT NULL, embedded_at TEXT NOT NULL, embedding BLOB NOT NULL
        );
        INSERT INTO turn_vectors(session_id, turn_index, chunk_seq, model, embedded_at, embedding)
        VALUES ('s', 0, 0, 'm', 't', x'0000803f'), ('s', 0, 0, 'm', 't', x'0000803f'),
               ('s', 1, 0, 'm', 't', x'0000803f');
        ",
    )
    .expect("seed v11 schema");
    drop(conn);

    let db = Database::open(&db_path).expect("migrate to v12");
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM turn_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2, "duplicate (s,0,0) must be removed");
    let min_kept: i64 = db
        .conn()
        .query_row(
            "SELECT id FROM turn_vectors WHERE turn_index = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(min_kept, 1, "the oldest row must be kept");
    // UNIQUE 인덱스가 걸려 같은 chunk 재삽입은 무시된다
    let ignored = db
        .conn()
        .execute(
            "INSERT OR IGNORE INTO turn_vectors(session_id, turn_index, chunk_seq, model, embedded_at, embedding) \
             VALUES ('s', 0, 0, 'm', 't', x'0000803f')",
            [],
        )
        .unwrap();
    assert_eq!(ignored, 0);
}
