# AIbb Conversational Outings and Profile Sync Implementation Plan

> For agentic workers: REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax.

**Goal:** Return explicit AIbb outings as personality-rich diary messages in chat, and synchronize a user-chosen AIbb nickname and local avatar across pet, chat, and settings windows.

**Architecture:** Rust remains the only intent router: an explicit outing begins exploration and every other message goes to ChatService. Exploration validates four findings, synthesizes an evidence-based diary, and emits it to the existing chat window. A single profile record lives beside settings; its normalized avatar is a small application-owned file and a versioned profile event refreshes all windows.

**Tech Stack:** Rust, Tauri 2 IPC/events/capabilities, SQLite/rusqlite migrations, React 19, TypeScript, Vitest, Canvas image normalization, existing LLM and safe web clients.

**Spec:** docs/superpowers/specs/2026-09-03-aibb-conversational-profile-design.md

## Global Constraints

- Explicit 去玩 / 去某方向玩 alone starts an outing; ordinary messages remain ordinary chat.
- A successful outing has exactly four discoveries, one diary, safe sources, a persisted round number, and elapsed seconds.
- AIbb never starts a next outing automatically.
- Nickname defaults to AIbb and is not a user identity.
- Imports accept PNG, JPEG, and WebP up to 5 MiB, normalize to 256 by 256 WebP, and never persist an original file path.
- Never put API Keys, source avatar paths, original avatar bytes, or provider diagnostics in profile records, events, or messages.
- Use only per-window Tauri permissions required for profile commands and profile update events.
- Every functional change follows RED then GREEN. Finish only after typecheck, frontend tests, Rust tests, fmt, clippy, debug build, runtime check, and review.

---

## File Structure

| File | Responsibility |
| --- | --- |
| src-tauri/src/domain.rs | AibbProfile, OutingSource, and expanded ExplorationResult contracts. |
| src-tauri/src/storage/migrations.rs | Profile columns and persisted outing report fields. |
| src-tauri/src/storage/database.rs | Profile CRUD, diary persistence/loading, completed outing count. |
| src-tauri/src/settings/profile.rs | Nickname and app-owned avatar validation/lifecycle. |
| src-tauri/src/commands/profile.rs | Scoped profile IPC, update event, native title refresh. |
| src-tauri/src/exploration/diary.rs | Diary envelope parser and synthesis request. |
| src-tauri/src/exploration/orchestrator.rs | Source collection, synthesis, persistence, completion event. |
| src/contracts.ts and src/lib/tauri.ts | Renderer contracts and typed profile bridge. |
| src/components/AibbAvatar.tsx | Built-in or custom avatar renderer. |
| src/features/settings, chat, pet | Profile editor, diary timeline, identity sync. |

## Task 1: Persist and validate the AIbb profile

**Files:**
- Create: src-tauri/src/settings/profile.rs
- Modify: src-tauri/src/domain.rs, src-tauri/src/storage/migrations.rs, src-tauri/src/storage/database.rs, src-tauri/src/settings/mod.rs, src-tauri/src/settings/service.rs
- Test: src-tauri/tests/settings_integration.rs and src-tauri/src/storage/migrations.rs

**Interfaces:**
- Produces AibbProfile { name: String, avatar_data_url: Option<String>, version: i64 }.
- Produces SettingsService::load_aibb_profile and SettingsService::save_aibb_name.

- [ ] **Step 1: Write failing profile migration and service tests**

~~~rust
#[tokio::test]
async fn profile_defaults_to_aibb_and_survives_reopen() {
    let db = TestDatabase::new();
    let service = SettingsService::new(db.handle(), FakeCredentialStore::default());
    assert_eq!(service.load_aibb_profile().await.unwrap().name, "AIbb");
    service.save_aibb_name("小团子".into()).await.unwrap();
    assert_eq!(
        SettingsService::new(db.reopen(), FakeCredentialStore::default())
            .load_aibb_profile().await.unwrap().name,
        "小团子"
    );
}

#[tokio::test]
async fn invalid_name_keeps_the_existing_profile() {
    // Save 小团子; reject whitespace-only and 25-character names; reload 小团子.
}
~~~

- [ ] **Step 2: Run test to verify RED**

Run: cargo test --manifest-path src-tauri/Cargo.toml --test settings_integration profile_

Expected: FAIL because profile schema and APIs do not exist.

- [ ] **Step 3: Write minimal storage implementation**

Add this migration:

~~~sql
ALTER TABLE app_settings ADD COLUMN aibb_name TEXT NOT NULL DEFAULT 'AIbb';
ALTER TABLE app_settings ADD COLUMN avatar_filename TEXT;
ALTER TABLE app_settings ADD COLUMN profile_version INTEGER NOT NULL DEFAULT 0;
~~~

Implement validate_aibb_name: trim, require 1 through 24 Unicode scalar values, reject control characters, and return invalidProfile. Add database load/save functions that increment profile_version in the same update as the name.

- [ ] **Step 4: Run test to verify GREEN**

Run: cargo test --manifest-path src-tauri/Cargo.toml --test settings_integration profile_

Expected: PASS; reload preserves a valid nickname and invalid input cannot overwrite it.

- [ ] **Step 5: Commit**

~~~bash
git add src-tauri/src/domain.rs src-tauri/src/storage src-tauri/src/settings src-tauri/tests/settings_integration.rs
git commit -m "feat: persist AIbb profile"
~~~

## Task 2: Add safe avatar commands and scoped profile events

**Files:**
- Create: src-tauri/src/commands/profile.rs
- Modify: src-tauri/src/settings/profile.rs, src-tauri/src/settings/service.rs, src-tauri/src/lib.rs, src-tauri/capabilities/default.json, src-tauri/capabilities/chat.json, src-tauri/capabilities/settings.json
- Test: src-tauri/tests/settings_integration.rs and src-tauri/src/lib.rs

**Interfaces:**
- Produces commands load_aibb_profile, save_aibb_name, save_aibb_avatar(bytes, mime_type), reset_aibb_avatar.
- Produces profile://updated carrying AibbProfile only.

- [ ] **Step 1: Write failing safety/capability tests**

~~~rust
#[tokio::test]
async fn avatar_import_is_app_owned_and_path_free() {
    let profile = service.save_aibb_avatar(valid_webp_bytes()).await.unwrap();
    assert!(profile.avatar_data_url.unwrap().starts_with("data:image/webp;base64,"));
    assert!(!database_bytes(&db).windows(b"C:\\Users\\face.png".len()).any(|x| x == b"C:\\Users\\face.png"));
}

#[tokio::test]
async fn invalid_avatar_keeps_the_previous_avatar() {
    // Seed valid bytes, attempt text and more than 5 MiB, then reload unchanged data URL.
}
~~~

Add library capability tests: settings may mutate profile; pet/chat may only load profile and listen for profile://updated.

- [ ] **Step 2: Run test to verify RED**

Run: cargo test --manifest-path src-tauri/Cargo.toml profile_

Expected: FAIL because avatar commands, event, and capabilities are absent.

- [ ] **Step 3: Write minimal avatar lifecycle**

Accept MIME image/png, image/jpeg, and image/webp only; reject vectors over 5 MiB before write. Store normalized renderer bytes at app-data/aibb-profile/avatar.webp using a sibling temporary file and atomic rename. SQLite stores only avatar.webp. Load the fixed filename with a 5 MiB cap and return data:image/webp;base64 data. Emit profile://updated after name/avatar/reset and update native pet, chat, settings titles using the saved nickname.

- [ ] **Step 4: Run test to verify GREEN**

Run: cargo test --manifest-path src-tauri/Cargo.toml profile_

Expected: PASS; failed import cannot replace a prior avatar and event payload has no file path.

- [ ] **Step 5: Commit**

~~~bash
git add src-tauri/src/commands/profile.rs src-tauri/src/settings src-tauri/src/lib.rs src-tauri/capabilities src-tauri/tests
git commit -m "feat: add safe AIbb avatar commands"
~~~

## Task 3: Build renderer profile controls and avatar normalization

**Files:**
- Create: src/features/profile/avatarImage.ts, src/features/profile/avatarImage.test.ts, src/components/AibbAvatar.test.tsx
- Modify: src/contracts.ts, src/lib/tauri.ts, src/components/AibbAvatar.tsx, src/features/settings/SettingsPanel.tsx, src/features/settings/SettingsPanel.test.tsx, src/App.css

**Interfaces:**
- Produces normalizeAvatarFile(file): Promise of WebP bytes and MIME.
- Updates AibbAvatar to render a profile data URL or the built-in robot.

- [ ] **Step 1: Write failing renderer tests**

~~~tsx
it("saves a trimmed nickname and previews it", async () => {
  render(<SettingsPanel />);
  fireEvent.change(await screen.findByLabelText("AIbb 昵称"), { target: { value: " 小团子 " } });
  fireEvent.click(screen.getByRole("button", { name: "保存 AIbb 资料" }));
  await waitFor(() => expect(saveAibbName).toHaveBeenCalledWith("小团子"));
});

it("normalizes a local PNG to WebP", async () => {
  const result = await normalizeAvatarFile(new File([pngBytes], "face.png", { type: "image/png" }));
  expect(result.mimeType).toBe("image/webp");
  expect(result.bytes.length).toBeGreaterThan(0);
});
~~~

- [ ] **Step 2: Run test to verify RED**

Run: pnpm vitest run src/features/settings/SettingsPanel.test.tsx src/features/profile/avatarImage.test.ts src/components/AibbAvatar.test.tsx

Expected: FAIL because profile controls, bridge functions, and normalizer do not exist.

- [ ] **Step 3: Write minimal renderer implementation**

Add typed profile invokes/listeners. Add a profile card before API settings: nickname, preview, select image, reset avatar, save profile. Reject unsupported MIME and over-5MiB files before decode. Use createImageBitmap, center crop to Canvas 256 by 256, export WebP, then invoke the backend. Decode/export failure keeps old preview and uses Chinese safe text. Custom image alt text is the nickname; no image means current robot SVG.

- [ ] **Step 4: Run test to verify GREEN**

Run: pnpm vitest run src/features/settings/SettingsPanel.test.tsx src/features/profile/avatarImage.test.ts src/components/AibbAvatar.test.tsx

Expected: PASS; no source path enters renderer state and failed imports retain preview.

- [ ] **Step 5: Commit**

~~~bash
git add src/contracts.ts src/lib/tauri.ts src/components src/features/profile src/features/settings src/App.css
git commit -m "feat: let users personalize AIbb"
~~~

## Task 4: Synchronize profile identity across all windows

**Files:**
- Modify: src/features/pet/PetSurface.tsx, src/features/pet/PetSurface.test.tsx, src/features/chat/ChatPanel.tsx, src/features/chat/ChatPanel.test.tsx, src/features/settings/SettingsPanel.tsx
- Test: the same feature tests

**Interfaces:**
- Consumes loadAibbProfile and listenProfileUpdated.
- Produces current-profile rendering; messages do not duplicate names/images.

- [ ] **Step 1: Write failing sync tests**

~~~tsx
it("updates the pet avatar and accessible name after profile update", async () => {
  render(<PetSurface status="idle" />);
  act(() => profileListener({ name: "小团子", avatarDataUrl: "data:image/webp;base64,AA==", version: 2 }));
  expect(screen.getByRole("button", { name: "小团子" })).toBeVisible();
});

it("renames existing assistant messages after profile update", async () => {
  // Render an assistant reply, update profile, assert author and avatar now use 小团子.
});
~~~

- [ ] **Step 2: Run test to verify RED**

Run: pnpm vitest run src/features/pet/PetSurface.test.tsx src/features/chat/ChatPanel.test.tsx src/features/settings/SettingsPanel.test.tsx

Expected: FAIL because windows hard-code AIbb.

- [ ] **Step 3: Write minimal synchronization**

Every window loads once and installs one unlistener. Pet uses profile name/avatar. Chat uses current profile in header, assistant message author, temporary outing state, diary heading, and old visible assistant messages. Settings accepts only externally newer profile versions. Message content is never rewritten.

- [ ] **Step 4: Run test to verify GREEN**

Run: pnpm vitest run src/features/pet/PetSurface.test.tsx src/features/chat/ChatPanel.test.tsx src/features/settings/SettingsPanel.test.tsx

Expected: PASS; unlisteners clean up and every old visible AIbb message uses updated identity.

- [ ] **Step 5: Commit**

~~~bash
git add src/features/pet src/features/chat src/features/settings
git commit -m "feat: synchronize AIbb identity across windows"
~~~

## Task 5: Generate validated outing diaries with safe sources

**Files:**
- Create: src-tauri/src/exploration/diary.rs
- Modify: src-tauri/src/domain.rs, src-tauri/src/exploration/contract.rs, src-tauri/src/exploration/mod.rs, src-tauri/src/exploration/orchestrator.rs, src-tauri/src/llm/client.rs, src-tauri/src/prompts.rs, src-tauri/src/storage/migrations.rs, src-tauri/src/storage/database.rs
- Test: src-tauri/src/exploration/contract.rs, src-tauri/src/exploration/orchestrator.rs, src-tauri/src/llm/client.rs, src-tauri/tests/settings_integration.rs

**Interfaces:**
- Produces `OutingSource { title: String, url: String }`.
- Expands `ExplorationResult` with `diary`, `sources`, `round_number`, and `elapsed_seconds`.
- Produces `build_outing_diary_request` and `parse_outing_diary`.

- [ ] **Step 1: Write failing diary and persistence tests**

~~~rust
#[test]
fn diary_parser_requires_a_nonempty_json_diary() {
    assert_eq!(
        parse_outing_diary(r#"{"diary":"第二轮回来啦"}"#).unwrap(),
        "第二轮回来啦"
    );
    assert!(parse_outing_diary(r#"{"diary":" "}"#).is_err());
}

#[tokio::test]
async fn successful_outing_persists_a_diary_with_four_items_safe_sources_round_and_elapsed() {
    let result = harness.orchestrator.run(request(Some("海里".into()))).await.unwrap();
    assert_eq!(result.items.len(), 4);
    assert_eq!(result.round_number, 1);
    assert!(result.sources.iter().all(|source| source.url.starts_with("https://")));
    assert!(harness.memory.assistant_messages().await[0].contains(&result.diary));
}
~~~

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml exploration`

Expected: FAIL because neither diary synthesis nor persisted report fields exist.

- [ ] **Step 3: Write minimal diary/sources implementation**

Keep the first exploration stage at exactly four findings. Replace raw web-page strings with records containing a safe title, canonical HTTPS URL, and bounded text. Extend the native web response parser to return its source objects through the same `OutingSource` filter.

Build a second, intentionally light LLM request from the four findings and retrieved evidence. It asks for one natural Chinese diary in a JSON envelope, grounded in those materials, but does not impose a topic, joke, favourite, emotional arc, or next action. Reject an empty or malformed envelope. Use only filtered source URLs collected by the backend, never model-invented URLs.

At completion, calculate `completed_outings + 1` and elapsed whole seconds, persist the report, append the diary (not an automatic next-outing request) to chat memory, and emit the enriched completion event. Keep the existing safe Chinese provider-error mapping.

- [ ] **Step 4: Run test to verify GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml exploration`

Expected: PASS; a report always retains four findings, safe sources, diary, round, and elapsed time.

- [ ] **Step 5: Commit**

~~~bash
git add src-tauri/src/domain.rs src-tauri/src/exploration src-tauri/src/llm src-tauri/src/prompts.rs src-tauri/src/storage src-tauri/tests
git commit -m "feat: return AIbb outings as diary reports"
~~~

## Task 6: Render departures and diaries in one chat timeline

**Files:**
- Modify: src/contracts.ts, src/lib/tauri.ts, src/features/chat/ChatPanel.tsx, src/features/chat/ChatPanel.test.tsx, src/App.css
- Delete: src/features/exploration/ExplorationPanel.tsx, src/features/exploration/ExplorationPanel.test.tsx (only after the replacement tests are green)

**Interfaces:**
- Produces `OutingTimelineMessage` from a typed exploration completion result.
- Reuses the normal assistant-message renderer for departure, progress, diary, sources, and errors.

- [ ] **Step 1: Write failing timeline tests**

~~~tsx
it("replaces departure state with an outing diary in the same timeline", async () => {
  mockSubmit.mockResolvedValue({ kind: "explorationStarted", taskId: "task-1" });
  render(<ChatPanel />);
  fireEvent.change(await screen.findByRole("textbox", { name: "消息" }), {
    target: { value: "去玩" },
  });
  fireEvent.keyDown(screen.getByRole("textbox", { name: "消息" }), { key: "Enter" });
  expect(await screen.findByText(/出发，去玩/)).toBeVisible();
  act(() => explorationCompleteListener({ taskId: "task-1", result: diaryResult }));
  expect(screen.getByText("第二轮回来啦")).toBeVisible();
  expect(screen.queryByRole("region", { name: "探索结果" })).not.toBeInTheDocument();
});

it("keeps an ordinary message as an ordinary chat reply", async () => {
  // Submit a non-outing message and assert no departure/timeline diary is inserted.
});
~~~

- [ ] **Step 2: Run test to verify RED**

Run: `pnpm vitest run src/features/chat/ChatPanel.test.tsx`

Expected: FAIL because exploration is currently rendered as a separate card below the conversation.

- [ ] **Step 3: Write minimal timeline implementation**

On an explicit outing start, append a temporary assistant entry such as “<nickname> 出发，去玩～”; update that entry from progress events. On completion, replace it in place with a diary entry headed “第 {roundNumber} 轮回来啦” and “思考了 {elapsedSeconds} 秒”. Render backend-provided sources as links with `target="_blank"` and `rel="noreferrer"`. On error, replace the temporary entry with the already-safe Chinese error text.

Leave normal ChatService replies on the existing ordinary assistant path. Remove `ExplorationPanel` only once the timeline is fully covered. Do not render a next-outing CTA or make a follow-up request automatically.

- [ ] **Step 4: Run test to verify GREEN**

Run: `pnpm vitest run src/features/chat/ChatPanel.test.tsx`

Expected: PASS; a departure and its result live in chronological chat order, while ordinary chat stays ordinary.

- [ ] **Step 5: Commit**

~~~bash
git add src/contracts.ts src/lib/tauri.ts src/features/chat src/features/exploration src/App.css
git commit -m "feat: show AIbb outings in chat timeline"
~~~

## Task 7: Verify the full feature and runtime behavior

**Files:**
- Modify only for scoped defects found by the checks below.

- [ ] **Step 1: Run complete automated verification**

~~~bash
pnpm typecheck
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
~~~

Expected: all checks pass with no formatting or lint warnings.

- [ ] **Step 2: Build and run a debug smoke test**

~~~bash
pnpm tauri build --debug --no-bundle
~~~

Launch the built debug app. In settings, save a nickname, select a supported local image, confirm pet/chat/settings synchronize, send one ordinary chat message, then send “去玩”. Confirm the diary replaces the departure in the same conversation and source links are present. Do not use an independently captured screenshot as evidence.

- [ ] **Step 3: Inspect the final change set**

~~~bash
git diff --check
git diff 9c7c3d5..HEAD --stat
git diff 9c7c3d5..HEAD -- src-tauri src
~~~

Expected: no whitespace errors, no persisted credentials/source paths, and no automatic-outing control path.

- [ ] **Step 4: Commit only scoped final fixes**

Stage only the concrete files changed by a failing verification command, inspect the staged diff, and commit them as `fix: polish AIbb conversational outings`.
