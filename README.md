# AIbb — 本地优先的 AI 桌宠

> 一只会聊天、会自己出去「玩」、还会帮你整理文件的桌面 AI 伙伴。

<p align="center">
  <a href="https://github.com/w384/AIbb/actions/workflows/build-windows.yml">
    <img src="https://github.com/w384/AIbb/actions/workflows/build-windows.yml/badge.svg" alt="Windows build" />
  </a>
  <a href="https://github.com/w384/AIbb/actions/workflows/build-mac.yml">
    <img src="https://github.com/w384/AIbb/actions/workflows/build-mac.yml/badge.svg" alt="macOS build" />
  </a>
</p>

AIbb 是一只常驻 Windows 桌面的 AI 宠物。它不只是聊天机器人：你说「去玩」，它会自己去中文科学论坛、前沿资讯里探索一圈，回来给你写一篇见闻日记；你把文件拖到它身上，它会按你的规则自动归档整理。所有对话、记忆、归档记录都只保存在这台电脑上。

## 为什么是 AIbb

| 你在意的事 | AIbb 的做法 |
| --- | --- |
| 🔒 隐私 | 完全本地运行，API Key 存于系统凭据（Windows Credential Manager），对话与记忆不上传任何服务器 |
| 💰 成本 | 自带 API Key（BYOK）模式，用多少付多少，无订阅捆绑 |
| 🐾 陪伴感 | 有昵称、头像、性格的桌宠，说话生动带表情，不是冷冰冰的对话框 |
| 🧭 会探索 | 「去玩」自动联网检索中文科学论坛与前沿资讯（可随时要求外网内容），并写成日记；聊天时也会偷偷溜出去玩，带回链接和图片 |
| 📁 会整理 | 把文件拖到宠物身上即按规则归档（支持周/分类层级、关键词规则、结构库模板） |
| 🖥 常驻 | 系统托盘常驻、始终置顶、开机自启可选 |

## 核心功能

1. **陪伴聊天** — 基于 DeepSeek 等兼容 OpenAI 的大模型，支持流式回复与记忆摘要；回复口语化、有温度，语气词与情绪处会自动配上贴切的表情包；聊天里出现的链接左键用系统浏览器打开，右键只有「复制链接」。
2. **出去玩（探索）** — 说「去玩」或「去 XX 方向玩」，AIbb 自动联网搜索、阅读、整理，带回一篇 4 个部分的日记（每个部分有醒目小标题、角度各不相同，其中至少一部分是看到图片后的感想，结尾总结找共同点），附来源链接与有趣的配图；你没指定方向时，它会优先挑「某个领域最新进展、考古学奇闻、天文学、海洋生物、前沿科技」这类冷门新奇的角度，避开烂大街的话题。
3. **自发探索** — 聊天中问到「最近有什么好玩的」「为什么…」这类话题时，AIbb 会自动在后台出去逛一圈，回来后把发现的链接和图片贴在聊天里，邀请你一起参与；搜索过程全程直播：搜索词、逐页阅读、日记逐字写出都能实时看到。
4. **历史回放** — 对话与出游记录都保存在本地；重开聊天窗口会自动回放最近的对话，并把之前的出游日记还原成带图片和来源链接的卡片，随时翻看过去的游玩信息。
5. **出游计数** — 每次出去玩都会记一次数；聊天窗口顶部有「🐾 ×N」徽章，出游结束后自动刷新。
6. **性格定制** — 设置里可以给 AIbb 选一套人设（活泼元气 / 温柔治愈 / 毒舌傲娇 / 冷静理性 / 话痨热闹 / 神秘高冷），或手写你自己的性格设定；聊天与出游日记都会按这套性格说话。
7. **拖拽归档** — 把任意文件拖到宠物身上，自动按规则分类归档到项目目录（原文件保留不动）；支持自动识别归档区已有结构。
8. **个性装扮** — 自定义昵称与头像，AIbb 只属于你。

## 快速开始（3 步）

1. 在 [platform.deepseek.com](https://platform.deepseek.com) 注册并创建 API Key（也支持其他兼容 OpenAI 的服务）。
2. 打开 AIbb 设置，粘贴 API Key —— API 地址与模型名称会自动填好。
3. 点「保存并测试」，提示连接成功后就可以聊天，或对它说「去玩」。

> 密钥只保存在本机系统凭据里；对话框与归档面板会提示每一步该做什么。

## 安装

- **正式安装包**：从 Release 下载 `AIbb_x64-setup.exe`（NSIS，简体中文向导）或 `AIbb_x64_zh-CN.msi` 安装。打 `v*` 标签后由 CI 自动构建并发布。
- **macOS**：从 Release 下载 `.dmg`（Apple Silicon，当前由 GitHub Actions 在 tag 时自动构建）。
- **从源码构建**（需要 Rust 1.80+ 与 pnpm）：

```bash
pnpm install
pnpm tauri build        # 产物在 src-tauri/target/release/bundle/
```

> 没有正式 Release 时，也可以从仓库 **Actions → build-windows → Artifacts** 下载最新构建的安装包。
> 安装后首次打开如遇 SmartScreen「未知发布者」提示，点「更多信息 → 仍要运行」即可（正式发布会补代码签名）。

## 构建状态

| 平台 | 工作流 | 内容 |
| --- | --- | --- |
| Windows | [build-windows](.github/workflows/build-windows.yml) | 前端/Rust 测试 + NSIS 安装包 |
| macOS | [build-mac](.github/workflows/build-mac.yml) | 前端/Rust 测试 + dmg |

每次推送到 `feature/aibb-desktop-pet` 都会自动跑两个平台的测试与编译；打 `v*` 标签时额外打包安装包并发布到 Release。

## 开发

```bash
pnpm dev                # tauri dev，热更新
pnpm typecheck          # tsc --noEmit
pnpm test               # vitest（前端）
cargo test --manifest-path src-tauri/Cargo.toml   # Rust 全量测试
```

技术栈：Tauri 2（Rust + WebView2 / WKWebView）+ React 19 + rusqlite（本地存储）+ reqwest（联网检索，rustls）。

## 悬浮窗操作

- **点击**：打开对话窗
- **长按或拖动**：移动悬浮窗，靠近屏幕边缘会自动吸附，且不会被拖出屏幕
- **右键**：打开设置
- **拖文件到宠物身上**：按规则归档

## 自动化构建

仓库内置 `.github/workflows/build-windows.yml`（Windows：测试 + NSIS 安装包）与 `.github/workflows/build-mac.yml`（macOS：测试 + dmg）：手动触发，或在推送 `v*` tag 时自动构建并发布安装包。

```bash
git tag v0.1.1 && git push origin v0.1.1   # 触发 Windows + macOS 自动构建 + Release
```

## 隐私与数据

- API Key：仅保存在系统凭据库（Windows Credential Manager），应用内不落盘明文。
- 对话、记忆、探索记录、归档台账：全部存在本地 SQLite（`com.clink.aibb` 配置目录）。
- 联网检索仅发生在你主动触发「去玩」或原生联网探索时，且只读取公开网页内容。
- 无遥测、无账号、无云同步（当前版本）。

## 路线图（建议）

- v0.2：探索订阅源（RSS）、归档统计与全文检索、备份/导出
- v0.3：多宠物形象与主题、更多模型供应商预设、自动更新
- 商业化：免费核心 + 可选 Pro（内置 AI 用量 / 高级归档规则），详见 `docs/市场与盈利分析.md`

## 支持与反馈

- 项目仓库：本仓库（AIbb desktop pet）
- 遇到问题请附带：操作系统版本、AIbb 版本、复现步骤与报错文案。

---

© Clink AI. 本地优先，快乐陪伴。
