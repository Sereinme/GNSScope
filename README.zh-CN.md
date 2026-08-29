# GNSScope

[English](README.md) | [简体中文](README.zh-CN.md)

GNSScope 是一个使用 Rust 编写的离线 GPS L1 C/A 软件接收机与桌面分析应用。它将已录制的交错 IQ 文件依次处理为捕获、跟踪、导航电文、观测量、位置/速度/时间（PVT）以及可持久化和回放的可视化结果。

> GNSScope 当前只处理已录制文件。Live SDR 输入、实时截止期和多星座接收尚未实现。

## 主要能力

- 读取交错有符号 `Int8` 和 `Int16` IQ 文件，可配置采样率与中频。
- 使用基于 FFT 的并行码相位搜索（PCPS）完成 GPS L1 C/A 捕获。
- 通过 DLL 与 Costas PLL 进行多通道跟踪，并保持各通道 NCO 连续性。
- 完成位同步、GPS NAV 子帧解码、广播星历选择，并支持显式配置的 RINEX 导航数据回退。
- 输出伪距、WLS 位置、多普勒速度、Hatch 平滑与 Kalman 候选解，以及 GPS 时间；具备绝对周参考时可输出 UTC。
- 支持确定性的有界并行捕获/跟踪、通道准入、失锁检测与重捕获。
- 提供 GPUI 桌面工作流，包括可编辑接收机配置、解算队列、SQLite 历史、图表回放、天空图/C/N₀ 诊断和 Raw/Smoothed/Kalman 候选解比较。

## 项目状态

最初定义的 M0–M2 主链路已经实现：录制 IQ 可完整经过捕获、跟踪、导航解码、连续 PVT、结果持久化和 GUI 历史回放。

当前验证覆盖静态 CTTC 与 LabSat 录制数据，这些数据集不随仓库分发。当前优先级是先结构化重构 App UI，再扩展 GPS L1C、GPS L2C、GPS L5 和北斗 B1I 等多系统、多频点信号处理。动态/高采样率验证和进一步精度评估暂时后移。

优先级请查看[项目路线图](docs/roadmap.md)，实测证据和已知精度边界请查看[接收机就绪审计](docs/receiver-readiness-audit.md)。

## 接收机流水线

```text
录制 IQ
    → 流式读取
    → PCPS 捕获
    → DLL / Costas PLL 跟踪
    → 位同步与 GPS NAV 解码
    → 观测量
    → WLS / Hatch / Kalman PVT
    → GPUI worker、SQLite 历史与图表
```

流水线采用有界、单遍文件处理。捕获和活动通道跟踪可以并行运行，但接收机状态按 PRN 确定性合并，PVT epoch 按时间顺序串行推进。

## 工作区结构

| Crate | 职责 |
| --- | --- |
| `gnss-data` | 构建时生成的 GPS L1 C/A PRN 表，无运行时依赖 |
| `gnss-core` | IQ I/O、捕获、跟踪、NAV 解码、观测量、PVT、通道管理和流式编排 |
| `gnss-gui` | GPUI 桌面界面、后台处理、任务队列、SQLite 持久化、历史回放、图表和候选解比较 |

## 下载安装

预构建安装包发布在 [GitHub Releases](https://github.com/Sereinme/GNSScope/releases) 页面：

- macOS Apple Silicon 与 Intel 磁盘映像（`.dmg`）
- Windows x64 安装程序（`.exe`）与便携压缩包（`.zip`）
- Debian/Ubuntu x64 软件包（`.deb`）与 Linux 便携压缩包（`.tar.gz`）

请使用附件中的 `SHA256SUMS` 校验下载文件。macOS 应用仅带 ad-hoc 签名、未经公证，Windows 安装程序也尚未进行代码签名，因此首次启动时系统可能要求用户显式确认。

## 快速开始

安装稳定版 Rust 工具链，然后在仓库根目录运行：

```bash
cargo run -p gnss-gui
```

在应用中选择 IQ 文件，并配置数据格式、采样率、中频、捕获网格、跟踪环路、导航数据策略和 PVT 参数。已完成的解算记录不可修改；重新处理会创建新的解算记录。

## 验证

运行常规工作区检查：

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

真实数据回归依赖 `data/` 下由 Git 忽略的数据集。部分完整文件和性能回归按设计标记为 ignored，必须在 release 构建中显式选择。缺少这些数据时，默认测试通过并不等同于完整接收机验证。

## macOS 应用包

在 macOS 上可生成带本地 ad-hoc 签名的应用包：

```bash
./scripts/package-macos.sh
```

输出位于 `target/release/bundle/osx/GNSScope.app`。该本地开发签名不能替代 Developer ID 签名和公证。详见 [macOS 打包说明](docs/macos-packaging.md)。

## 项目文档

- [项目路线图](docs/roadmap.md)：权威优先级与工作顺序
- [UI 架构](docs/ui-architecture.md)：应用流程、导航与响应式交互约定
- [接收机就绪审计](docs/receiver-readiness-audit.md)：真实数据、运行时间、内存与精度证据
- [领域术语表](CONTEXT.md)：接收机和产品的统一术语
- [架构决策记录](docs/adr/)：流水线、持久化、GUI 和延期的信号扩展设计

## 当前限制

- 仅实现 GPS L1 C/A；延期的信号 trait 设计不代表已经支持多信号或多星座。
- 仅处理录制文件；不支持 Live SDR 设备输入和丢样恢复。
- 当前静态数据集不能证明高动态性能。
- 缺少测量级天线坐标或独立参考接收机时，不宣称绝对定位精度。
- 当导航电文只提供截断的 GPS 周时，原始 IQ 若没有 RINEX 或配置的参考周，就无法确定 UTC 日期。
