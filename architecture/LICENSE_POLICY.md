# 开源许可证、第三方依赖与商业分发政策

- 文档版本：0.1
- 生效日期：2026-08-13
- 项目拟定许可证：AGPL-3.0
- 适用范围：Rust 核心、原生客户端、CLI、Web/PWA、测试工具、构建产物和随发行包提供的资源
- 复审触发：新增依赖、切换静态/动态链接方式、加入字体/图标/音视频资源、启用新的加密实现或准备商业发布

## 1. 项目许可证与交付范围

项目自有源代码采用 AGPL-3.0（GNU Affero General Public License v3.0）。每个可发布产物必须携带项目许可证、第三方许可证文本和 NOTICE 汇总；WebAssembly、桌面安装包、移动包和 CLI 不因封装方式不同而减少归属或通知义务。

当前仓库仍处于 spike/原型阶段。`wgpu-terminal` 是未来 release-candidate 路径；`terminal-engines`、`ssh-engines` 和 `terminal-contract` 仅用于开发验证，除非它们的依赖重新通过发布扫描，否则不得直接进入商业发行包。

## 2. SPDX 表达式策略

扫描器以 Cargo metadata 的许可证字段为来源，并保留原始表达式。许可证表达式必须能映射到 SPDX 标识或 SPDX 复合表达式；无法解析、缺失或只存在于本地口头说明的许可证进入 `review_required`。

默认允许进入 release-candidate 的许可证族：Apache-2.0、MIT、BSD-2-Clause、BSD-3-Clause、ISC、Zlib、0BSD、Unlicense、CC0-1.0、Unicode-3.0、Unicode-DFS-2016、NCSA，以及这些许可证之间的 `OR`/`AND` 组合，只要通知义务可由发行包履行。

GPL、AGPL、LGPL、WTFPL、专有许可证和未知许可证不能被静默接受。含有 LGPL 但同时提供 MIT/Apache/BSD 等可选路径的表达式必须记录选择依据和动态链接验证；WTFPL-only、GPL-only、AGPL-only 或未声明许可证依赖默认为 `development_only`，不得进入 release-candidate。

## 3. 静态与动态链接义务

| 链接方式 | 允许条件 | 必须交付的证据 |
|---|---|---|
| Rust/C/C++ 静态链接 | 依赖许可证在 allowlist 内，或有明确的可选许可路径；不得把 WTFPL/GPL/AGPL-only 依赖带入产物 | 版本锁定、SPDX 清单、许可证全文、NOTICE、构建命令和产物哈希 |
| 动态链接 | 除上述要求外，必须保存库版本、加载路径、替换/重链接方式和平台打包声明 | 动态库清单、依赖许可证、安装包归属文本、重链接或替换说明 |
| LGPL 依赖 | 只有在动态链接或满足对应 relink/对象文件义务并经过法律复核时才允许 | relink 方案、对象文件/源码提供方式、平台测试和法律复核记录 |
| WebAssembly | 依赖和归属文本随 Web/PWA 发行包可访问；不得因压缩或 bundling 丢失通知 | 第三方通知页、source map/归属策略、bundle 哈希 |

扫描器不会替代法律意见；它只判断仓库证据是否齐全，并在报告中列出需要人工复核的依赖。

## 4. 字体、图标和其他非代码资源

当前仓库没有 `.ttf`、`.otf`、`.woff`、`.woff2`、图标或品牌图片资源，因此当前报告记录为 `not_present`，不是“已获得字体/图标许可”。以后加入资源时必须为每个文件登记：来源 URL、版本、作者、许可证 SPDX 表达式、是否允许商业分发、修改状态、归属文本、文件 SHA-256 和替代资源。

禁止把系统字体、平台图标或第三方品牌资产当作无许可证资源打包。Windows、Apple、Linux、Android 和 Web 的系统字体/图标 API 只能在平台文档允许的方式下调用；随包分发的字体和图标必须进入同一 SPDX/NOTICE 清单。

## 5. 加密与出口边界

当前 spike 只记录 `sha2` 等密码学相关依赖和 SSH 引擎候选，不代表已经完成商业加密出口合规。首个面向美国或其他司法辖区的商业二进制发布前，必须由负责的法务/出口合规人员确认适用的 EAR/当地规则、软件分类、是否适用公开可用/加密例外、申报或报告义务、发行地区限制和客户筛选流程。

扫描器要求报告包含：密码学包列表、使用场景、是否进入 release-candidate、待完成的出口分类/法律复核、负责角色和复审日期。报告不能把“使用开源密码库”推断为自动豁免，也不能把出口合规状态写成 `passed`，除非有外部审查证据。

## 6. 商业分发门禁

商业发行包只有在以下条件同时满足时才可标记为 release-ready：

1. 所有 release-candidate 依赖都有 SPDX 表达式、版本、来源和许可证全文/NOTICE 证据。
2. 所有 development-only 依赖都没有进入产物；WTFPL/GPL/AGPL-only 或未声明依赖被构建门禁拦截。
3. 静态/动态链接方式和 LGPL 等特殊义务已在目标平台验证。
4. 字体、图标和其他资源逐文件完成归属与商业分发确认。
5. 加密出口分类、法律复核和发布地区策略已签字；在此之前报告只能是 `pass_with_restrictions`。
6. 许可证清单、NOTICE、源代码获取说明和产物哈希随构建归档，且扫描结果可由同一命令复现。

## 7. 扫描证据格式

`artifacts/reports/LICENSE_COMPLIANCE.json` 是机器可读事实源，`artifacts/reports/LICENSE_COMPLIANCE.md` 是人工审阅摘要。每个依赖必须包含名称、版本、SPDX/原始许可证、来源、scope、release_eligible、链接方式、证据来源和备注。扫描失败、缺失元数据、未知许可证和过期复核必须显式列出，不能被过滤。

