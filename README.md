# Wuwa Tappo — macOS 移植版（仅个人使用）

把 Windows 版 Wuwa Tappo 1.0.1 的前端（Vue）原样拿出来，配上一个重写的 macOS 版 Tauri 后端。
联网、登录、图鉴、背包、房间等功能全部是原版前端代码，直接连库洛的官方服务器。

> 素材和前端代码版权归库洛所有。仅供自己使用，请勿分发。

## 在 Mac 上构建

把整个 `wuwatappo-mac` 文件夹拷到 Mac（不需要 `src-tauri/target`），然后：

```bash
chmod +x build-mac.sh
./build-mac.sh
```

脚本会按需安装 Xcode 命令行工具、Rust 和 tauri-cli。第一次构建大约 5–10 分钟。
产物在 `src-tauri/target/release/bundle/macos/Wuwa Tappo.app`，拖进「应用程序」即可。

## 首次运行需要的权限

- **输入监控**（系统设置 → 隐私与安全性 → 输入监控）：用来统计全局键盘敲击数。
  首次启动会弹窗请求；授权后需要**退出并重新打开**应用。
- 鼠标点击统计不需要额外权限。
- 每次重新构建后签名会变，macOS 可能要求重新授权。

## 和 Windows 版的差异

| 功能 | macOS 实现 |
|---|---|
| 全局键鼠监听（rdev / 钩子） | CGEventTap（`input_macos.rs`） |
| 桌宠鼠标穿透 / 命中区域 | 按光标位置动态切换 `set_ignore_cursor_events`（`hit.rs`） |
| 托盘菜单 / 桌宠右键菜单 | 菜单栏图标 + 同样的菜单项（`menu.rs`） |
| 开机自启（计划任务提权） | LaunchAgent |
| "显示任务栏图标" | 显示/隐藏 Dock 图标 |
| 自动更新 | 不支持（官方只出 Windows 包），始终视为"已是最新" |
| 安装 / 卸载器页面 | 不需要 |

热键：F5（重新加载，仅开发版生效）/ F7 / F8（切换显示器）/ F9。Mac 键盘上可能需要按住 Fn。

## 目录结构

- `dist/`：从 `WuwaTappo.exe` 提取的前端资源（已打补丁，见下）
- `src-tauri/`：macOS 后端
- `tools/`：提取 / 分析 / 打补丁用的脚本

## 官方版本更新后怎么同步

1. 在 Windows 上安装新版官方客户端。
2. 重新提取并打补丁：
   ```
   python -I tools/extract_assets.py "C:\Program Files\WuwaTappo\WuwaTappo.exe" dist
   python -I tools/patch_frontend.py dist
   ```
3. 先把新版提取到另一个目录，用 `tools/api_signature.py <exe> <dist目录>` 分别对旧、新前端各跑一次再对比，
   就能看到新增、删除的后端命令，参数变化和事件名变化也会列出来。有变化的话在 `src-tauri/src/` 里补上。
4. 把 `src-tauri/tauri.conf.json` 和 `Cargo.toml` 里的版本号改成新版本号。

前端补丁（`tools/patch_frontend.py`）只有一处：登录 SDK 的地址 `//pro-cdn-sdk...` 改成 `https://`，
因为 Mac 上页面的 origin 是 `tauri://localhost`，协议相对地址会解析错。

## 排查问题

- 日志：`~/Library/Logs/com.kuro.wuwatappo.cn/wuwatappo.log`
- 点数存档：`~/Library/Application Support/com.kuro.wuwatappo.cn/counters.json`
- 调试前端：用 `cd src-tauri && cargo tauri dev` 运行，在窗口里右键 → 检查元素。
  如果某个功能没反应，先看控制台里有没有 `xxx failed` 的报错，一般对应某个后端命令还需要调整。
