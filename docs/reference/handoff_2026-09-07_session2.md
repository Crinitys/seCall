---
type: handoff
status: done
updated_at: 2026-09-07
---

# Handoff 2026-09-07 (session 2) — 세션 프리즈 원인 규명: Orca codex 세션 백필

> 이전 세션 핸드오프: [handoff_2026-09-07.md](./handoff_2026-09-07.md)

## 요약

Claude Code 세션이 작업 도중 "session restore" 상태에서 멈추고, `secall` 프로세스를 강제 종료해야 풀리는 현상이 반복됐다. 원인은 seCall 이 아니라 **Orca 가 `~/.codex/sessions` 로 하드링크해 둔 141개의 유령 Codex 세션**이었다. 사용자는 Codex CLI 를 제거하고 `~/.codex` 를 삭제했지만, Orca 가 자체 격리 홈에서 세션을 다시 링크해 두었고, SessionStart 훅의 `secall sync` 가 매번 그 141개를 임베딩하느라 세션 진입을 막았다.

유령 세션을 제거한 뒤 `secall sync --local-only` 는 즉시 종료한다.

## 증상

- 세션 시작/재개 시 "session restore" 에서 프롬프트 진입 불가
- 사용자가 세션을 새로 열거나 종료하지 않았는데도 작업 도중 발생
- `secall` 프로세스를 죽이면 즉시 정상화
- MCP 를 비활성화해도 재발

## 조사 경과

### 1. 훅 특정

`~/.claude/settings.json` 의 SessionStart 훅:

```json
{ "matcher": "startup|resume",
  "hooks": [{ "type": "command", "command": "secall sync --local-only --no-semantic" }] }
```

`timeout` 이 없어 프롬프트 진입 전 블로킹으로 실행된다. `resume` 는 사용자가 세션을 새로 만들지 않아도 하네스가 세션을 이어붙일 때(auto-compact, 재연결) 발사된다. 실제로 이번 대화 중에도 SessionStart 컨텍스트 블록이 중간에 재주입되는 것이 관측됐다.

### 2. 소요 시간 실측

```
$ time timeout 100 secall sync --local-only --no-semantic
Embedding 121 session(s)...
  [1/121] ...
  [5/121] 01a069f8 (216 turns)
real 1m40.071s   ← 타임아웃까지 121개 중 5개
```

`--no-semantic` 은 그래프 추출만 끈다. **임베딩은 끄지 않는다** (`--no-embed` 가 별도 플래그). 121세션(최대 3756턴) 을 Ollama 로 임베딩하는 동안 세션이 잠긴다.

### 3. 대상 세션의 정체

전부 Codex 세션이었다. vault 산출물 frontmatter:

```yaml
agent: codex
project: llama.cpp
cwd: D:\Store\Git\repo\llama.cpp
session_id: 01a069f8-ce63-7f41-a339-846077eae603
```

소스는 `~/.codex/sessions/2026/09/04/rollout-*.jsonl` 141개. 사용자는 Codex CLI 를 제거했고 `which codex` 결과도 없었다.

### 4. 파일 타임스탬프 모순

```
~/.codex           Birth: 2026-09-07 03:38:01
rollout-*.jsonl    Birth: 2026-09-04 10:11:46   ← 원본 생성시각 보존
                   Change: 2026-09-07 03:38:01  ← 오늘 자리 이동
```

새로 쓴 파일이면 Birth 가 오늘이어야 한다. 원본 시각이 보존된 채 ctime 만 갱신된 것은 **복원 또는 하드링크**를 뜻한다. 휴지통·OneDrive·예약작업·디스크 내 다른 사본은 모두 음성이었다.

### 5. 범인 확정 — Orca

`%APPDATA%\orca\codex-session-backfill\backfill-complete.json`:

```json
{ "version": 4,
  "systemSessionsRoot": "C:\\Users\\Thurion\\.codex\\sessions",
  "coverage": "full" }
```

같은 폴더 `audit.jsonl` 마지막 레코드 — 시각이 정확히 일치한다:

```json
{"at":"2026-09-06T18:38:01.209Z","action":"run-summary","stopped":false,
 "scannedFiles":141,"linkedFiles":141,"copiedFiles":0}
```

UTC 18:38:01 = KST 09-07 03:38:01, `~/.codex` Birth 와 동일. 개별 레코드는 하드링크 동작을 명시한다:

```json
{"action":"hardlink",
 "source":"...\\orca\\codex-runtime-home\\home\\sessions\\2026\\08\\26\\rollout-....jsonl",
 "target":"C:\\Users\\Thurion\\.codex\\sessions\\2026\\08\\26\\rollout-....jsonl"}
```

`copiedFiles: 0, linkedFiles: 141` — 복사가 아닌 하드링크라 원본 Birth/mtime 이 그대로 따라온다. 4번의 타임스탬프 모순이 이것으로 설명된다.

Orca 앱 번들(`app.asar`) 확인 결과 이 동작은 앱 시작 시 자동 1회 실행된다:

```js
Q.codexSessionMigration = vgi({
  isEligible: () => Q.codexRuntimeHome?.isHostSystemDefaultSessionMigrationEligible() === true,
  resolveSystemCodexHomePathOverride: () => S2(e.getSettings()),
  startBackfill: JYr, startIndexHeal: Bgi })
Q.codexSessionMigration.scheduleInitialRun()
```

제어 스위치는 환경변수가 아니라 설정 UI 항목이다:

```
codexSessionSource        : "Codex home to import from"
codexSessionSourceTooltip : "Orca runs Codex in an isolated home. Point this at your
                             existing Codex home to import that session history.
                             Empty uses ~/.codex."
```

비워두면 기본값이 `~/.codex` 이므로 백필이 계속 동작한다.

## 조치

### 삭제 (사용자 승인 후 실행)

```bash
rm -rf ~/AppData/Roaming/orca/codex-runtime-home/home/sessions   # 141개 / 58MB, 원본
rm -rf ~/.codex/sessions                                          # 하드링크 (이미 삭제된 상태였음)
rm -f  ~/AppData/Roaming/orca/codex-session-backfill/backfill-complete.json
```

`audit.jsonl` / `index-heal-*.json` 은 이력 로그이므로 증거 보존을 위해 남겼다. Orca 설정은 사용자 판단으로 변경하지 않았다 — 향후 Codex 를 서브에이전트로 정상 사용하면 그 세션은 오히려 기록되어야 하기 때문.

### 검증

`secall sync --local-only` 즉시 종료 확인.

### 코드 변경

`3754f94 feat(ingest): 세션 인제스트 출력에 소스 파일 경로 추가`

ingest 결과 블록이 vault 산출물 경로만 보여줘 원본 세션 파일을 추적할 수 없었다. `File:` 아래 `Source:` 라인을 추가한다.

```
  File:    C:\Users\Thurion\obsidian-vault\seCall\raw\.sessions\2026-09-04\codex_....md
  Source:  C:/Users/Thurion/.codex/sessions/2026/09/04/rollout-....jsonl
  BM25:    1 turns indexed
```

- `crates/secall/src/output.rs` — `print_ingest_result` 에 `source_path` 파라미터
- `crates/secall/src/commands/ingest.rs` — `ingest_single_session` 으로 전달 (호출부에 이미 있던 `session_path` 재사용)

## 확인된 인제스트 탐색 경로

| 진입점 | 코드 | codex 루트 |
| --- | --- | --- |
| `secall sync` | `sync.rs:553` | `~/.codex/sessions` |
| `secall ingest --auto` (cwd 없음) | `ingest.rs:1253` | `~/.codex/sessions` |
| `secall ingest <디렉터리>` | `ingest.rs:1263` | 해당 디렉터리 하위 `.jsonl` 전부 |
| `secall ingest --auto --cwd <path>` | `detect.rs:270` | **codex 미탐색** (claude projects 만) |

SessionEnd 훅은 마지막 경로를 쓰므로 codex 를 끌어오지 않는다. 문제를 일으킨 것은 앞의 두 경로다.

## 알려진 제약

- **`find_codex_sessions` 는 `exclude_patterns` 를 적용하지 않는다** (`crates/secall-core/src/ingest/detect.rs:222` 가 `is_pruned_dir(e, &[])` 로 하드코딩). `config.ingest.exclude_patterns` 로 codex 경로를 제외할 수단이 없다. claude 경로만 `find_claude_sessions_excluding` 으로 적용된다.
  - 이번에는 패치하지 않기로 결정했다. Codex 를 정상 사용하게 되면 그 세션은 인제스트되어야 하며, 이번 문제는 "쓰지도 않은 세션이 유령으로 링크된 것"이 원인이었기 때문.
  - 같은 이유로 `codex_observer-sessions_*` (claude-mem 옵저버가 codex 로 돌던 시절 산출물) 도 `exclude_patterns = ["claude-mem"]` 를 우회한다.

## 운용 메모

- 오래 걸리는 sync 는 `--no-embed` 로 분리한다. `--no-semantic` 은 임베딩을 끄지 않는다.

  ```bash
  secall sync --local-only --no-embed --no-semantic
  secall embed --all
  ```

- 진행 상황 표시와 취소는 **Web UI 에 이미 있다**. CLI 로 sync 를 돌리면 둘 다 없다.

  ```bash
  secall serve   # http://localhost:8080 → Commands 탭
  ```

  `web/src/components/JobBanner.tsx` 가 phase + 진행률 + 취소 버튼을 제공하고, API 는 `POST /api/commands/sync` → `GET /api/jobs/{id}/stream` (SSE) → `POST /api/jobs/{id}/cancel` (`crates/secall-core/src/mcp/rest.rs:161-172`).

- `D:\Store\Git\repo` 트리 탐색 결과 codex 세션 데이터 없음. `.codex/` 디렉터리 3개(`AdvancedPTCGPTool`, `PTCGPB_Crinity_Rust`, `PTCGPB_Crinity_v2`)는 `config.toml` / `hooks.json` 만 있고 `sessions/` 가 없어 인제스트 대상이 아니다.

## 남은 작업

- 로컬 `main` 이 `fork/main` 보다 13 커밋 뒤처져 있음 (`git branch -f main fork/main` 으로 정리)
