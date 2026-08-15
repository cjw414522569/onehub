# PC 客户端 UI（照抄自 mXterm）

本目录按用户指示，从 `C:\work\ssh\mxterm` **全量照抄** mXterm 前端（UI 页面与前端源码），用于 PC 客户端界面。

## 照抄来源与许可

- 来源：mXterm（https://github.com/syscryer/mxterm），版本 0.1.17
- 许可：MIT（见 `LICENSE.mxterm`，版权归 mXterm contributors）
- 拷贝内容：`src/`（React 前端 111 个文件）、`index.html`、`package.json`、`package-lock.json`、`vite.config.ts`、`tsconfig.json`、`tsconfig.node.json`
- 未拷贝：`src-tauri/`（Tauri Rust 后端）——按「UI 照抄 + 核心桥接」方案，功能走本项目现有 Rust 核心（abi-c）

## 集成状态

- T004 起：npm 安装 + Tauri shim + vite build，产出 `dist/`
- T005-T006 起：Win32 壳用 WebView2 承载本 UI
- T007 起：JS↔Rust 桥（window.sshHost / abi-c）接通会话、连接、终端等
- 凭据（密码/私钥）一律不在此层持久化