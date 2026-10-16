pub mod classify;
pub mod config;
pub mod embed;
pub mod get;
pub mod graph;
pub mod ingest;
pub mod init;
pub mod lint;
pub mod log;
pub mod mcp;
pub mod migrate;
pub mod model;
pub mod recall;
pub mod reindex;
pub mod serve;
pub mod status;
pub mod sync;
pub mod wiki;

/// 무거운 작업(sync/ingest/embed/reindex/wiki/graph)의 프로세스 사이 단일 실행 잠금.
///
/// 진입점(`main.rs` 명령 분기, `serve.rs` REST 어댑터)에서만 잡는다. sync 안에서
/// 호출되는 `ingest_sessions` / `wiki::run_update` 가 다시 잡으면 같은 프로세스의
/// 다른 핸들끼리 막혀 자기 자신을 기다린다.
#[derive(Clone, Copy)]
pub(crate) enum LockMode {
    /// 잡혀 있으면 건너뛴다 — 훅(SessionStart sync, SessionEnd ingest --auto)용.
    Skip,
    /// 잡혀 있으면 풀릴 때까지 기다린다 — 사용자가 직접 실행한 명령용.
    /// bin(`main.rs`)에서만 쓰여 lib 타깃에서는 미사용이다.
    #[allow(dead_code)]
    Wait,
}

pub(crate) const JOB_LOCK_BUSY: &str = "다른 secall 작업이 실행 중이라 건너뜁니다";

/// DB 폴더의 `job.lock` 을 잡는다. `Skip` 에서 잡혀 있으면 `None`.
/// 잠금은 반환된 `File` 이 drop 될 때(프로세스 비정상 종료 포함) OS 가 푼다.
pub(crate) fn acquire_job_lock(mode: LockMode) -> anyhow::Result<Option<std::fs::File>> {
    let path = secall_core::store::get_default_db_path().with_file_name("job.lock");
    acquire_job_lock_at(&path, mode)
}

/// REST 작업용 — 잡혀 있으면 작업을 에러로 끝낸다. 기다리면 P36 작업 취소가
/// 동작하지 않는다.
pub(crate) fn require_job_lock() -> anyhow::Result<std::fs::File> {
    acquire_job_lock(LockMode::Skip)?.ok_or_else(|| anyhow::anyhow!(JOB_LOCK_BUSY))
}

fn acquire_job_lock_at(
    path: &std::path::Path,
    mode: LockMode,
) -> anyhow::Result<Option<std::fs::File>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| anyhow::anyhow!("job lock open failed ({}): {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => match mode {
            LockMode::Skip => {
                eprintln!("{JOB_LOCK_BUSY}");
                Ok(None)
            }
            LockMode::Wait => {
                eprintln!("다른 secall 작업이 끝날 때까지 기다립니다...");
                file.lock()
                    .map_err(|e| anyhow::anyhow!("job lock failed ({}): {e}", path.display()))?;
                Ok(Some(file))
            }
        },
        Err(std::fs::TryLockError::Error(e)) => {
            Err(anyhow::anyhow!("job lock failed ({}): {e}", path.display()))
        }
    }
}

/// CLI 경로에서 `run_with_progress`를 호출할 때 사용하는 비활성 sink.
///
/// 기존 CLI 출력은 `eprintln!` 그대로 유지되며, sink 메서드는 모두 no-op이다.
/// 이로써 P33 Task 02의 phase 분리 도입 후에도 CLI 회귀가 발생하지 않는다.
pub(crate) struct NoopSink;

#[async_trait::async_trait]
impl secall_core::jobs::ProgressSink for NoopSink {
    async fn phase_start(&self, _phase: &str) {}
    async fn message(&self, _text: &str) {}
    async fn progress(&self, _ratio: f32) {}
    async fn phase_complete(&self, _phase: &str, _result: Option<serde_json::Value>) {}

    // P36 Task 01 — CLI 컨텍스트는 외부 cancel 채널이 없으므로 항상 false.
    // (default 구현이 동일하지만 의도를 명시적으로 표기.)
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod job_lock_tests {
    use super::*;

    /// 잡혀 있는 동안 Skip 은 None, 해제 후엔 다시 잡힌다.
    /// (Windows LockFileEx / Unix flock 모두 같은 프로세스의 다른 핸들도 막는다.)
    #[test]
    fn skip_returns_none_while_held_and_some_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("job.lock");

        let first = acquire_job_lock_at(&path, LockMode::Skip).unwrap();
        assert!(first.is_some());
        assert!(acquire_job_lock_at(&path, LockMode::Skip)
            .unwrap()
            .is_none());

        drop(first);
        assert!(acquire_job_lock_at(&path, LockMode::Skip)
            .unwrap()
            .is_some());
    }

    /// Wait 는 다른 핸들이 잠금을 풀 때까지 기다렸다가 잡는다.
    #[test]
    fn wait_blocks_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("job.lock");

        let held = acquire_job_lock_at(&path, LockMode::Skip).unwrap().unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            drop(held);
        });

        let start = std::time::Instant::now();
        let got = acquire_job_lock_at(&path, LockMode::Wait).unwrap();
        assert!(got.is_some());
        assert!(start.elapsed() >= std::time::Duration::from_millis(150));
        releaser.join().unwrap();
    }
}
