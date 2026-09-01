# Task 9 实施报告

## 结论

Task 9 已连接 renderer 的 pet/chat/settings 路由、聊天事件流、探索结果与取消、设置安全语义、pet 状态 reducer，并新增精确 Tauri capability。实现没有进入 Task 10 的托盘、原生通知、开机启动插件、E2E 或发布工作。

- renderer 根据真实 Tauri window label 路由 `pet / chat / settings`；测试仍可通过 query label 覆盖。
- chat/settings/exploration panels 均只调用其窗口所需的最小命令；API Key reload 永远保持空 replacement input。
- renderer 每次请求 bootstrap 都从 SettingsService 重建脱敏状态；保存或替换 protected key 后不需要重启应用即可进入 chat。
- 普通消息由 `submit_user_input` 启动 chat；只有 Task 8 纯 parser 识别的显式外出命令才启动 exploration。
- 聊天使用每请求一次的 `SettingsService::exploration_task_snapshot`，固定 base/model/key，并用 `FixedCredentialStore` 构造该任务的 `OpenAiClient`，不会在流式请求与 summary 间混用并发保存后的设置。
- provider delta 经过跨 chunk 增量脱敏后才发往 `chat://delta`；当前 key、Bearer 与 Authorization 值不会进入 renderer 事件、assistant memory、complete event 或 summary。用户输入中回显的当前 key 同样在落盘前脱敏。
- assistant reply 先持久化再发 `chat://complete`；summary 只在可靠 exact task key 下执行，失败不撤销已持久化回复。
- exploration panel 严格按 task ID 过滤 progress/complete/error；换到模型请求的下一次 outing 后立即注销旧监听，取消只作用于新 task。
- settings 前端和 Rust service 都把空白 replacement key 解释为“保留已有 key”；清除 memory 需要窗口内确认，且不授予 settings 窗口 `clear_api_key`。
- pet reducer 覆盖 chatting/exploring/returned/error/idle 转换，忽略其他 task 的终态事件；`returned` surface 显示短气泡。
- pet surface 只获得 exploration event listen/unlisten 权限；真实 progress/complete/error 驱动 reducer，打开 returned 结果时先等待 chat window 显示成功再 reset idle。

## 精确权限边界

`chat` capability 仅授予：

- `core:event:allow-listen`
- `core:event:allow-unlisten`
- `allow-get-bootstrap-state`
- `allow-open-settings-window`
- `allow-submit-user-input`
- `allow-start-exploration`
- `allow-cancel-exploration`

`settings` capability 仅授予：

- `allow-load-settings`
- `allow-save-settings`
- `allow-test-connection`
- `allow-clear-memory`

lower-level `start_chat` 已注册供 Rust 组合，但未授权任何 renderer。chat 也没有 settings、pet drag 或 position 权限。

## RED → GREEN 证据

1. 新增 `chat_flow` fake 集成测试首次因 `commands::chat` 不存在产生 E0432；实现后 3/3 通过。
2. capability focused 测试首次以 `chat capability must exist` 失败；新增 chat capability、收紧 settings capability 后 exact capability 3/3 和 forbidden permission 1/1 通过。
3. 用户消息含 `sk-secret` 的回归首次显示 SQLite 保存原值；在 task snapshot 后、context/append 前统一脱敏后通过。
4. 空白 replacement key 的 renderer 测试首次收到 `apiKey: "   "`；frontend 归一为 `null` 后通过。Rust service 回归首次把 protected credential 覆盖成空白；service 归一空白为 omitted 后通过。
5. next-outing 测试首次只安装一次 exploration listener；切换到返回的新 task ID、注销旧 listener 后通过，旧 task progress 被忽略且 cancel 指向新 task。
6. returned pet bubble 测试首次找不到 `role=status`；最小气泡实现后通过。
7. summary 成功回归做了受控 mutation：临时跳过保存时测试以 `eligible old memory must be summarized` 失败；恢复实现后 summary exact-key/Authorization 脱敏测试通过。
8. renderer bootstrap refresh 回归首次因 live helper 不存在而产生 E0432；改为从 SettingsService 实时重建后，stale startup state 不再遮蔽新保存的 protected key。
9. pre-commit review 的三项阻断均先复现：Rust exploration event 暴露 `task_id` 而非 `taskId`、pet 未安装真实事件 listener、assistant persistence 失败没有 terminal error；显式 serde 字段契约、pet event/reducer 接线和 chat 统一错误收敛后 focused tests 全绿。另以 cancelled-other-task RED 证明 pet 不会被其他任务终态重置。

所有模型、stream、summary 与 exploration 自动化均使用 fake 或既有本地测试桩；本任务没有调用真实模型、公开搜索或付费接口。

## 最终门禁

| 命令 | 结果 |
|---|---|
| `pnpm typecheck` | exit 0 |
| `pnpm test` | 14 files / 44 tests passed，Type Errors 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 131 tests passed，doc tests 0 failed |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | exit 0 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | exit 0 |
| `pnpm tauri build --debug --no-bundle` | exit 0，生成 debug executable |
| `git diff --check` | exit 0，仅既有 LF→CRLF 提示 |

Windows MSVC 继续显示项目既有的中文 `linker_messages` “正在创建库” warning；Clippy `-D warnings` 自身没有诊断，测试和 Tauri build 均退出 0。

## 风险与边界

- 当前任务只在 Windows debug 构建验证；macOS 编译、签名与公证仍不在本任务范围。
- `src/styles/global.css` 在仓库中不存在，真实单一样式入口是 `src/App.css`；本任务沿用现有入口，没有创建重复全局样式文件。
- Task 10 才负责原生托盘、通知、实际 autostart 集成、E2E 与发布门禁。
