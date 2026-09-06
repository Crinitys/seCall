use thiserror::Error;

#[derive(Error, Debug)]
pub enum SecallError {
    // --- Store ---
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("database not initialized: run `secall init` first")]
    DatabaseNotInitialized,

    // --- Ingest ---
    #[error("parse error for {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("unsupported file format: {0}")]
    UnsupportedFormat(String),

    /// 파일은 정상이지만 user/assistant 대화 턴이 하나도 없는 세션.
    /// 세션을 열기만 하고 대화를 하지 않으면 mode/attachment/system 같은 메타
    /// 이벤트만 기록된 jsonl 이 남는다. 손상이 아니므로 ingest 에서 error 가 아닌
    /// skip 으로 집계해야 한다.
    #[error("session has no conversation turns: {path}")]
    NoTurns { path: String },

    // --- Search ---
    #[error("search error: {0}")]
    Search(String),

    #[error("embedding error: {0}")]
    Embedding(#[source] anyhow::Error),

    // --- Vault ---
    #[error("vault I/O error: {0}")]
    VaultIo(#[from] std::io::Error),

    // --- Not Found ---
    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("turn not found: session={session_id} turn={turn_index}")]
    TurnNotFound { session_id: String, turn_index: u32 },

    // --- Config ---
    #[error("config error: {0}")]
    Config(String),

    // --- General (anyhow fallback) ---
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SecallError>;
