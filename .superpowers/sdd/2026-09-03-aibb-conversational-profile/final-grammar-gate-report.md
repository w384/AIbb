# AIbb conversational profile — final outing grammar gate

Date: 2026-09-05
Worktree: `D:\AI\Codex\Projects\Clink AI\.worktrees\aibb-implementation`
Commit subject: `fix: recognize outing question grammar`

## Outcome

The final Important gate is closed without extending the previous scattered direction blacklist. Explicit outing shells still accept high-freedom direction phrases, while recognizable Chinese question and evaluative clause structures conservatively remain ordinary chat.

## Root cause and grammar model

After stripping the explicit outer shell `去/往 + ... + 玩`, the previous implementation searched arbitrary substrings inside the remaining candidate. That conflated an attributive phrase such as `值得探索的海洋` with a clause-final predicate such as `公园是否值得`.

The parser now separates direction safety from clause interpretation:

1. `is_explicit_outing_direction` checks only safety, the empty `方向` placeholder, and the centralized clause classifier.
2. `is_question_or_evaluative_outing_clause` recognizes three structural positions:
   - interrogative pronouns at the candidate boundary, such as `哪里` and `什么`;
   - polar-question structures, including `是否`, `有没有`, `能否`, and a generic Unicode-safe A-not-A ending detector;
   - clause-final evaluative predicates ending in `值得` or `好`.
3. Interior natural-language terms are not rejected merely for appearing in a direction. Thus `去值得探索的海洋玩` remains an explicit outing.

## Conservative ambiguity boundary

Pure rules cannot determine every context-free Chinese ellipsis: the same bare surface `去X玩` can be read as a command or as part of an omitted-context statement. The product's explicit-command contract resolves that irreducible ambiguity by accepting a complete safe `去/往 + X + 玩` shell unless the candidate carries a recognizable interrogative boundary or clause-final question/evaluation predicate. Recognized ordinary questions therefore fail closed to Chat; the parser does not guess from arbitrary interior words.

## RED → GREEN evidence

The focused parser table first failed on `去公园要不要玩`, returning `Explore { direction: "公园要不要" }`. After the centralized grammar classifier and generic A-not-A detector were introduced, all required new negatives, earlier question/evaluation negatives, and explicit positive directions pass together.

New Chat regressions cover:

- `去公园要不要玩`
- `去公园该不该玩`
- `去公园应不应该玩`
- `去公园能否玩`
- `去公园有没有必要玩`
- `去公园不好玩`

Existing positive regressions retain `去玩`, `去公园玩`, `往北玩`, and `去值得探索的海洋玩`.

## Final verification

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — passed.
- `pnpm typecheck` — passed.
- `pnpm test` — 18 files, 122 tests passed; no type errors.
- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets` — 174 tests passed (42 unit + 6 chat + 30 exploration + 27 LLM + 7 memory + 28 settings + 34 web).
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings -A linker_messages` — passed. `linker_messages` remains excluded only for MSVC's normal import-library creation notice.
- `git diff --check` — passed.

## Workspace note

The pre-existing untracked cache directories `.cargo-home-task2/` and `.cargo-target-sdd/` were not touched and remain excluded from the commit.
