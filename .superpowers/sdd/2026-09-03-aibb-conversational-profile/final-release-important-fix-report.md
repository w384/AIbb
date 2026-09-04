# AIbb conversational profile — final release Important fixes

Date: 2026-09-05
Worktree: `D:\AI\Codex\Projects\Clink AI\.worktrees\aibb-implementation`
Commit subject: `fix: close final release review findings`

## Outcome

Both final release Important findings are closed with RED → GREEN regressions. The outing parser now distinguishes explicit direction text from common semantic questions without rejecting directions that happen to contain `值得`. Ordinary chat completion persistence and terminal delivery are linearized against memory clearing, closing the remaining post-append TOCTOU window.

## Changes

1. **Outing grammar precision**
   - Preserved explicit outings: `去玩`, `去公园玩`, `往北玩`, and `去值得探索的海洋玩`.
   - Preserved existing chat cases: `去公园是否值得玩`, `去公园值不值得玩`, and `去公园真的好玩`.
   - Added chat cases for `是不是`, `能不能`, `可不可以`, and `适不适合` question structures.
   - Removed the broad `direction.contains("值得")` rejection. Only an actual `值得` predicate ending remains rejected, while explicit A-not-A / semantic-question markers are matched precisely.
2. **Completion/clear lifecycle boundary**
   - `MemoryRepository` now shares a completion-boundary mutex across clones.
   - `ChatService` holds that boundary only across the generation-checked assistant append and `Complete` event delivery, then releases it before optional summarization.
   - `clear_memory` acquires the same boundary before its existing atomic delete transaction. A clear request therefore cannot complete between a persisted assistant reply and its terminal event; if clear wins before completion begins, the existing generation barrier still rejects the stale append and emits no `Complete`.
   - No event payload or frontend type changed, so existing early-event and `ChatPanel` request-ID filtering semantics remain intact.

## RED → GREEN evidence

- The parser regression first failed because `去值得探索的海洋玩` was classified as chat. The focused table passes after replacing the broad blacklist with precise grammatical markers and a predicate-ending check.
- The real asynchronous terminal race first demonstrated that `clear_memory` completed while the `Complete` sink was deliberately paused after verifying the assistant row existed. With the shared lifecycle boundary, the same clear remains pending until terminal delivery is released, then clears memory successfully.

## Final verification

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — passed.
- `pnpm typecheck` — passed.
- `pnpm test` — 18 files, 122 tests passed; no type errors.
- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets` — 174 tests passed (42 unit + 6 chat + 30 exploration + 27 LLM + 7 memory + 28 settings + 34 web).
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings -A linker_messages` — passed. `linker_messages` remains excluded only for MSVC's normal import-library creation notice.
- `git diff --check` — passed.

## Workspace note

The pre-existing untracked cache directories `.cargo-home-task2/` and `.cargo-target-sdd/` were not touched and remain excluded from the commit.
