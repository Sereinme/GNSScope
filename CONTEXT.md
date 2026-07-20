# GNSScope — Domain Glossary

本文档是 GNSScope 项目的一体语言（ubiquitous language）。所有代码、文档、Issue 必须使用这里定义的术语，不得使用同义词。

---

## 信号处理流程

- **IQ 文件**（IQ file）：SDR 前端采集后存储的本地文件。先支持 `int8` 实部/虚部交错格式。测试数据集使用 GNSS-SDR 的 `2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN`（100s GPS L1 C/A, 4 Msps, 零中频, 16-bit interleaved）。
- **采样率**（sample rate, fs）：IQ 文件的采样频率，单位 Msps，用户在 GUI 中配置。
- **中频**（intermediate frequency, IF）：ADC 采样后的残余载波频率，零中频时为 0 Hz。
- **环路带宽**（loop bandwidth）：跟踪环路（DLL/PLL）的噪声带宽，单位 Hz。
- **捕获**（acquisition）：在频率-码相位二维搜索空间中检测 GPS L1 C/A 信号的存在，输出粗略的多普勒频率和码相位。采用**并行码相位搜索**（Parallel Code Phase Search, PCPS）——通过 FFT 将本地 PRN 码与输入信号的循环互相关一次性完成，逐多普勒频点扫描。FFT 使用 **RustFFT** 库。
- **跟踪**（tracking）：对捕获到的信号进行精密跟踪，持续输出解扩后的符号、载噪比（C/N₀）和码相位/载波相位。默认采用**二阶 DLL + 二阶 Costas PLL**（两象限反正切鉴别器），可选**FLL+PLL 级联**用于弱信号/高动态场景。
- **DLL**（Delay Lock Loop）：码跟踪环，采用非相干早减迟归一化鉴别器（`dll_nc_e_minus_l_normalized`），E/L 间距可配。
- **PLL**（Phase Lock Loop）：载波相位跟踪环，默认 Costas PLL，对导航电文 180° 相位翻转免疫。
- **FLL**（Frequency Lock Loop）：载波频率跟踪环，引入频率锁定辅助 PLL，适用于弱信号初始锁定阶段。
- **导航电文解码**（nav message decoding）：从跟踪输出的符号流中提取定位必需信息——星历（ephemeris）、电离层参数、UTC 参数、卫星健康状态。**不实现完整历书（almanac）解码**（离线场景已知大致时间，历书无必要）。
- **电离层延迟**（ionospheric delay）：单频接收机使用 GPS 广播的 **Klobuchar 模型**（8 参数 α₀-α₃, β₀-β₃），从导航电文第 4 子帧提取。
- **对流层延迟**（tropospheric delay）：采用 **Saastamoinen 模型**，气象参数先硬编码标准值（P=1013.25 hPa, T=20°C, RH=50%），后续考虑 GUI 可配。
- **伪距**（pseudorange）：利用卫星发射时间和本地接收时间计算的粗略距离，含钟差。M1 采用**相对伪距**（以 IQ 文件第一帧为时间零点），M2 解码 TOW 后转为绝对伪距。
- **PRN 码**（PRN code）：GPS L1 C/A 的 Gold 码序列，32 颗卫星各 1023 码片。采用**硬编码查找表**（`const PRN_CODES: [[i8; 1023]; 32]`），编译时生成。
- **多普勒频率**（doppler frequency）：卫星与接收机相对运动造成的载波频率偏移，典型范围 ±10 kHz。
- **捕获检测**（acquisition detection）：PCPS 结果中峰值/均值比超过设定阈值则判定信号存在，阈值由期望虚警率 PFA 推导。
- **PVT 解算**（PVT solution）：利用至少 4 颗卫星的伪距和多普勒测量，通过**加权最小二乘**（Weighted Least Squares, WLS）求解接收机位置（Position）、速度（Velocity）和时间（Time）。权重基于各卫星跟踪通道输出的 **C/N₀**（载噪比越高，权重越大）。
- **解算任务记录**（processing run）：一次针对指定 IQ 文件、使用固定配置执行的完整接收机处理记录。记录具有独立身份和不可变配置快照，可处于排队、处理中、已完成、失败、已取消或已中断状态；已完成记录包含可跨应用重启查看的 PVT 历史与通道诊断结果。重复处理同一 IQ 文件仍产生新的解算任务记录。
- **已中断任务**（interrupted processing run）：应用或系统在任务正常终止前退出而留下的解算任务记录。已中断任务保留退出前产生的结果，但不包含可恢复的接收机内部状态；重新处理会创建新任务并从 IQ 文件开头开始。
- **解算任务队列**（processing run queue）：等待成为活动解算任务的有序任务集合。同一时刻最多存在一个活动解算任务；活动任务终止后，队首任务自动开始。排队任务可以取消或调整顺序，但不会并行占用接收机处理资源。
- **解算任务库**（processing run library）：本机持久保存解算任务记录的集合，为 GUI 侧边栏提供历史浏览、状态恢复和结果重放。解算任务库不复制或拥有原始 IQ 文件。
- **天空图**（sky plot）：以接收机为中心、天顶为圆心、地平线为外圈的卫星方位角/高度角极坐标视图。天空图按解算 epoch 展示卫星几何位置，并以有限的视觉编码表达通道状态、C/N₀ 和 PVT 使用状态；它不是卫星轨道或星座结构示意图。
- **活动解算任务**（active processing run）：当前由后台接收机持续处理的解算任务记录。活动解算任务独立于 GUI 当前查看内容，在用户浏览其他历史记录时仍继续处理并更新自身状态。
- **选中解算任务**（selected processing run）：GUI 主工作区当前展示的解算任务记录，可以是活动任务或任意历史任务。选中任务只控制查看焦点，不控制后台处理生命周期。

## MVP 分阶段路线

| 阶段 | 交付物 | 验收标准 |
|---|---|---|
| **M0** | `gnss-core`：PRN 码生成 + PCPS 捕获（单通道、固定参数） | 给定已知的 IQ 测试文件，能正确检测到 ≥4 颗卫星，输出多普勒和码相位 |
| **M1** | + 跟踪 + 导航电文解码 | 能输出子帧数据（TLM+HOW），校验通过 |
| **M2** | + PVT + GUI | 完整端到端：选择文件 → 处理 → 地图上显示定位结果 + 图表 |

## 技术选型

- **线性代数**：**nalgebra**（`SMatrix` 栈上定长矩阵，适合 PVT 的 4×4~8×8 矩阵运算）
- **FFT**：**RustFFT**（纯 Rust，SIMD 加速，捕获用）
- **大文件 IO**：**memmap2**（内存映射，OS 页缓存优化顺序读取）
- **并行化**：**Rayon**（每 chunk 内 32 通道相关器并行计算）
- **图表**：**gpui Canvas 手绘**（C/N₀ 折线图、相关峰曲线图、2D 定位散点图，约 150 行）
- **坐标转换**：**自己实现** WGS84 ECEF ↔ LLA（迭代算法，约 30 行）
- **PRN 码**：硬编码查找表 `const PRN_CODES: [[i8; 1023]; 32]`

## 系统架构

- **离线接收机**（offline receiver）：从本地 IQ 文件读取数据进行后处理，不做实时流式处理。采用**分段流式流水线**架构（见 ADR-0001）——IQ 文件按固定大小 chunk 分段读取，单遍遍历，每 chunk 内所有通道并行处理。
- **多通道并行处理**（multi-channel parallel）：所有 32 颗 GPS L1 C/A 卫星（PRN 1-32）同时捕获和跟踪。每个活跃通道维护独立的跟踪状态。
- **通道**（channel）：一个通道负责一颗卫星从捕获到导航电文输出的完整处理流水线。通道状态可以是：空闲（idle）、捕获中（acquiring）、跟踪中（tracking）、已锁定（locked）。内部通过 **trait 抽象**（`SignalSpec`、`Acquisition`、`Tracking`、`NavDecoder`）解耦具体信号制式（见 ADR-0002），当前仅实现 GPS L1 C/A，后续可扩展 GPS L1C/L2C/L5 等。
- **chunk**：IQ 文件的分段读取单元，固定为 **1 ms**（一个 C/A 码周期）。在 2.048 Msps 下约 4 KB，内存极小。捕获通过跨 chunk 的**非相干累积**（non-coherent integration）达到所需的积分时间（典型 10-20 ms）。跟踪每 1 ms 更新一次环路。
- **GUI**：用户界面，基于 **gpui**（Zed 编辑器同款 GPU 加速框架），提供文件选择、参数配置、结果展示（定位、C/N₀、捕获/跟踪状态图表），每 chunk 刷新一次。
- **线程模型**：信号处理跑在独立工作线程上，通过 channel（`crossbeam` 或 `std::sync::mpsc`）向 gpui 主线程发送状态更新。每 chunk 内用 **Rayon** 并行化 32 通道的相关器运算，保证 GUI 流畅和信号处理吞吐。
