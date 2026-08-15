# OneHub

OneHub 是一个 **Windows-first 的 PC 客户端**，以 [mXterm](https://github.com/syscryer/mxterm) 为交互原型，逐步接入真实后端能力：SQLite 持久化、加密保险库、真实 SSH/SFTP、隧道、定时任务、网络诊断、本地终端、Docker、主机监控、AI 助手、MCP、RDP、VNC、WebDAV 同步等。

## 目录

- `clients/windows/` — PC 客户端（Rust + WebView2，二进制 `onehub.exe`；前端为照抄 mXterm 的 React UI）
- `clients/windows/ui/` — 前端 UI（`index.html` → `OneHub`，构建产物 `dist/`）
- `crates/` — 核心 crate（abi-c 桥、core-domain、forwarding、proxy、secret、transfer 等）
- `docs/` — 项目文档与控制账本（`docs/PROJECT_CONTROL.mxterm.md`：T001–T019 逐行推进记录）
- `scripts/` — 验证/门禁脚本（`test-pc-gui.mjs`、`validate-control.ps1`、`validate-workspace.mjs` 等）

## 启动

```bat
start-pc.bat
```

脚本会按需构建前端（`clients/windows/ui`）与原生二进制（`target\debug\onehub.exe`）后启动。

## 开发

```bash
cargo build -p clients-windows --locked     # 编译原生客户端
cargo test -p clients-windows --locked      # 单测 + 集成
node scripts/test-pc-gui.mjs                # PC GUI 契约门禁
```

Windows 优先；其他平台仅保留已批准的接口边界。

## 许可

前端照抄自 mXterm（MIT，见 `clients/windows/ui/LICENSE.mxterm`，版权归 mXterm contributors）；本项目其余部分见仓库内许可声明。
