use anyhow::Result;
use secall_core::{
    mcp::{start_mcp_http_server, start_mcp_server},
    search::tokenizer::create_tokenizer,
    search::vector::create_vector_indexer,
    search::{Bm25Indexer, SearchEngine},
    store::get_default_db_path,
    store::Database,
    vault::Config,
};

pub async fn run(http: Option<String>) -> Result<()> {
    let db_path = get_default_db_path();
    let db = Database::open(&db_path)?;

    let config = Config::load_or_default();
    let tok = create_tokenizer(&config.search.tokenizer)
        .map_err(|e| anyhow::anyhow!("tokenizer init failed: {e}"))?;
    let bm25 = Bm25Indexer::new(tok);
    let vector = create_vector_indexer(&config).await;
    let search = SearchEngine::new(bm25, vector);

    let vault_path = config.vault.path.clone();

    // web-ui feature로 컴파일된 바이너리에서만 REST/Web UI를 background로 자동 기동.
    // 포트 충돌 등으로 실패해도 MCP 서버 자체는 계속 동작해야 하므로 에러는 warn만.
    #[cfg(feature = "web-ui")]
    if config.web.auto_start {
        let port = config.web.port;
        tokio::spawn(async move {
            if let Err(e) = crate::commands::serve::run(port, false).await {
                tracing::warn!(
                    error = %e,
                    port,
                    "Web UI 자동 기동 실패 (포트 충돌 등) — MCP 서버는 정상 동작합니다"
                );
            }
        });
    }

    match http {
        Some(addr) => start_mcp_http_server(db, search, vault_path, &addr).await,
        None => start_mcp_server(db, search, vault_path).await,
    }
}
