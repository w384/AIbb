# AIbb — macOS 版构建与测试

AIbb 是 Tauri 2 + Rust + React 的跨平台桌宠。macOS 版与 Windows 版共享同一份代码；
Windows 专属逻辑（桌面快捷方式图标、NSIS/WiX 安装包、盘符检查）都通过 `cfg(windows)`
隔离，不影响 macOS 构建。

## 环境准备（Mac 上只需一次）

```bash
# 1. Xcode 命令行工具（含 Rust 链接器需要的 clang）
xcode-select --install

# 2. Rust（如未装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 3. Node 与 pnpm
brew install node          # 或官方安装包
corepack enable && corepack prepare pnpm@9 --activate
```

## 本地开发运行

```bash
git clone git@github.com:w384/AIbb.git
git checkout feature/aibb-desktop-pet
pnpm install
pnpm tauri dev             # 热更新开发模式
```

## 打包 dmg

```bash
pnpm tauri build
# 产物：src-tauri/target/release/bundle/dmg/AIbb_0.1.0_aarch64.dmg
```

## 运行未签名 dmg（Gatekeeper 绕过）

当前**没有 Apple Developer 证书**，构建产物未签名。第一次打开时：

1. 双击 dmg 拖入「应用程序」，或直接打开 .app
2. 若提示"无法打开，因为无法验证开发者"：
   - **右键** .app → **打开** → 再次确认打开（每次版本都要一次）
   - 或终端执行：`xattr -cr /Applications/AIbb.app`

## 已做的 macOS 适配

- `macOSPrivateApi: true`：透明桌宠窗口必需（否则 pet 窗口显示为白块）
- `ActivationPolicy::Accessory`：不在 Dock 显示图标，只在菜单栏留托盘图标
- 托盘左键 = 开关聊天窗（`show_menu_on_left_click(false)`），右键 = 菜单
- pet 窗口 `visibleOnAllWorkspaces`：在所有"桌面空间"可见
- 手动拖动（`cursor_position`）与边缘吸附逻辑跨平台，任何 DPI 精确

## CI（GitHub Actions）

- 推送 `feature/aibb-desktop-pet` → 自动跑 macOS 编译 + Rust 测试 + 前端测试
- 打 `v*` tag 或手动触发 → 额外打包 dmg 并上传 artifact；tag 还会发布 GitHub Release

## 需要实测并反馈的点

- 透明置顶窗口在全屏应用 / 台前调度下的层级
- 聊天框中文输入法（IME）
- 拖动跟手、边缘吸附、文件拖放归档
- 菜单栏图标显示与左键行为
