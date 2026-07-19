# PVT 伪距修复分析

**日期**: 2026-07-18
**状态**: 根因已确认，修复路径已明确

---

## 1. 问题现状

PVT 端到端打通，但定位不准：

| 指标 | 当前结果 | 真值 | 误差 |
|---|---|---|---|
| 纬度 | 41.2750° | 41.2747° | ~33 m |
| 经度 | 1.9377° | 1.9875° | ~5.5 km |
| 高度 | 4057 m | 52 m | ~4 km |

原始 NDO（NCO 累加器）的 `cp/1023` 方法仍是最佳结果。所有试图"修正"码相位的尝试都未改善定位精度。

---

## 2. 根因分析

### 2.1 核心矛盾

`DllPllTracking.code_phase_chips` 是 NCO 累加器，不是真实的信号码相位。每条卫星都有一个**恒定的偏移量**，来自捕获初始化的误差：

```
NCO(t) − SCP(t) = 常数 = acq_cp − (1023 − acq_cp) = 2·acq_cp − 1023
```

DLL 锁定后只调整码频率（`code_freq`），不调整绝对码相位。偏移量永久存在。

### 2.2 为什么纬度准、经度不准

各卫星的 NDO 值**相对差异**（卫星间）恰好部分编码了几何距离差。WLS 利用这个相对关系解算纬度（N-S 方向），但对绝对伪距敏感，导致经度/高度偏差。本质上：
- 相对伪距差异 → 33m 纬度精度 ✓
- 绝对伪距值 → 5.5km 经度偏差 ✗

### 2.3 为什么偏移不是常数

各卫星码多普勒不同（PRN 17 ~9900 Hz vs PRN 11 ~5450 Hz），60 秒内 NCO 累积漂移差可达 ~173 chips。每颗卫星经历了不同的 DLL 调整历程，无法用单一公式批量消除。

---

## 3. 尝试过的方法

| 方法 | lat 误差 | lon 误差 | 结论 |
|---|---|---|---|
| **Baseline（原始 NDO cp/1023）** | 33m | 5.5km | 当前最佳 |
| DLL 校正（true_cp = NDO − dll_error） | 33m | 5.5km | 无效。dll_error ~±0.06 chips，太小 |
| acq_cp 公式（SCP = NDO + 1023 − 2·acq_cp） | 180km | 268km | 更差。公式不正确 |
| 迭代几何校准 | 发散 | 发散 | 反馈回路不稳定，位置在两解间震荡 |
| 残差校正（solve → residuals → subtract → re-solve） | 33m | 5.5km | 同 baseline。等价于拟合到错误位置 |
| pull-in + rem 独立跟踪 | 发散 | 发散 | 需要完整 observables 层（TOW 插值、公共时钟） |

---

## 4. GNSS-SDR 的伪距公式

### 4.1 核心架构

GNSS-SDR 的伪距计算**完全不使用 code_phase_chips**。其数据流为：

```
Tracking → Telemetry Decoder → Observables
```

跟踪层输出：
- `Tracking_sample_counter`（u64）：绝对采样计数
- `Code_phase_samples`（f64）：亚采样码相位残余，始终 < 1 sample
- `fs`（int64_t）：采样频率

### 4.2 伪距公式

```cpp
// 接收时间（精确到亚采样点）
T_rx = (Tracking_sample_counter + Code_phase_samples) / fs;

// 发射时间（TOW 符号间线性插值）
interp_TOW = TOW_1 + (TOW_2 - TOW_1) * (T_rx - T_sample_1) / (T_sample_2 - T_sample_1);

// 伪距
Pseudorange = (d_T_rx_TOW_ms - interp_TOW_ms) * SPEED_OF_LIGHT_M_MS;
```

### 4.3 Pull-in 对齐

GNSS-SDR 在 `start_tracking()` 中跳过输入样本，使码发生器对齐到 PRN 周期起点：

```cpp
// 已流逝样本与捕获码相位的差值
delta = acq_trk_diff_samples - Acq_delay_samples;

// 到下一个 PRN 周期边界的距离
samples_to_skip = T_prn_samples - fmod(delta, T_prn_samples);

consume(samples_to_skip);  // 跳过样本，对齐输入

// 对齐后：
d_rem_code_phase_samples = 0.0;  // 残余为 0
local_code_from_chip_1();        // 本地码从 chip 1 开始
```

### 4.4 rem_code_phase_samples 的闭环维护

```cpp
// 每相关周期更新（不依赖 code NCO 绝对值）
K_blk = T_prn_samples + d_rem - dll_error_secs * fs;
d_correlation_length_samples = round(K_blk);
d_rem = K_blk - d_correlation_length_samples;  // 始终 < 1 sample
```

### 4.5 与 GNSScope 的关键差异

| | GNSS-SDR | GNSScope |
|---|---|---|
| 码相位来源 | `rem_code_phase_samples`（DLL 闭环） | `code_phase_chips`（NCO 累加器） |
| 发射时间 | TOW 符号间插值（亚 20ms） | 子帧锚点 + ms 计数 + 码相位 |
| 码发生器状态 | Pull-in 后从 chip 1 开始 | 从捕获码相位开始 |
| 缓冲长度 | 可变（DLL 调整） | 固定 1ms（4000 样本） |
| Observables 层 | 有（公共接收时钟 + 历史缓冲区 + 跨通道插值） | 无（直接在 pvt_e2e.rs 中计算） |

---

## 5. 当前代码中的可用基础设施

经过本次 session，以下基础设施已就位（保留在代码中）：

### dll_pll.rs
- `sample_counter: u64` — GNSS-SDR `Tracking_sample_counter`
- `last_dll_error: f64` — DLL 鉴别器输出
- `ms_within_bit: u8` — 20ms bit 内 1ms epoch 序号（0-19）
- `set_bit_boundary(usize)` — 外部 bit sync 结果注入
- `new()` 接受 `start_sample_index: u64`
- `TrackingOutput` 包含 `ms_within_bit`、`dll_error`

### pvt_e2e.rs
- `dll_error: Vec<f64>`、`rem_code_phase: Vec<f64>` per-ms 记录
- bit sync 后调用 `ch.trk.set_bit_boundary(b)`

---

## 6. 推荐的下一步：实现轻量 Observables 层

### 6.1 新增 `pvt/observables.rs`

参考 GNSS-SDR `hybrid_observables_gs.cc`，实现：

```rust
/// 伪距观测值构建器
struct PseudorangeBuilder {
    /// 公共接收时刻对应的绝对采样计数器
    rx_sample_clock: u64,
    /// GPS TOW at receiver（毫秒）
    rx_tow_ms: u64,
    /// 接收步长（毫秒）
    rx_step_ms: u64,
}

impl PseudorangeBuilder {
    /// 从公共接收时刻 + 每通道跟踪数据构建伪距
    fn build(
        &self,
        rx_clock: u64,               // 公共接收采样时钟
        tow_history: &[(usize, u64)], // (bit_idx_ms, tow_at_symbol_ms)
        tracking_outputs: &[TrackingOutput],
    ) -> Vec<Pseudorange> {
        // 1. 对每个通道，在 tow_history 中做二分查找
        // 2. 线性插值 TOW 到 rx_clock 时刻
        //    interp_TOW = TOW_1 + (TOW_2 - TOW_1) * (rx_clock - sc_1) / (sc_2 - sc_1)
        // 3. T_rx = (rx_clock + rem_code_phase_samples) / fs
        //    转换为 GPS 时间：rx_tow_ms + frac_ms
        // 4. PR = (T_rx_GPS_ms - interp_TOW_ms) * c
    }
}
```

### 6.2 关键约束

1. **公共接收时钟**：选取一个统一的接收时刻（如文件末尾 ms），所有通道用同一 `rx_clock`
2. **TOW 插值**：利用 NavDecoder 输出的 `tow_at_current_symbol_ms`（20ms 粒度），在两个相邻符号之间做线性插值
3. **不依赖 code_phase_chips**：亚 ms 精度来自 TOW 插值 + `rem_code_phase_samples`
4. **Clock bias 吸收公共偏移**：WLS 估计的 `clk_bias` 吸收 GPS-文件时间偏移

### 6.3 伪距公式（最终形态）

```
对于接收时刻 rx_clock（所有通道相同）：

T_rx_GPS_ms = rx_tow_ms + frac((rx_clock + rem_sv) / fs * 1000)

T_tx_GPS_ms = interp_TOW_ms(从 tow_history 插值得到)

PR_sv = (T_rx_GPS_ms - T_tx_GPS_ms) * 299792.458  // c in m/ms
```

---

## 7. 参考文献

- GNSS-SDR `hybrid_observables_gs.cc` — 公共接收时钟伪距算法
- GNSS-SDR `gps_l1_ca_dll_pll_tracking_gpu_cc.cc` — pull-in + rem 闭环维护
- GNSS-SDR `gps_l1_ca_telemetry_decoder_gs.cc` — `TOW_at_current_symbol_ms` 维护
- GNSS-SDR `gnss_synchro.h` — `Tracking_sample_counter`、`Code_phase_samples` 定义
- IS-GPS-200 Section 20.3.3.3 — 伪距定义 ρ = c·(t_u − t_sv)
- `docs/M2-PROGRESS.md` — 前期伪距根因分析
- `docs/debug-pvt-pseudorange-research.md` — GNSS-SDR/IS-GPS-200/RTKLIB 伪距调研
- `CLAUDE.md` — 项目架构和约定

---

## 8. 提交历史（本次 session）

| Commit | 内容 |
|---|---|
| TBD | feat: add sample_counter + dll_error + ms_within_bit tracking infrastructure |
| TBD | docs: pseudorange fix analysis — root cause + GNSS-SDR investigation + path forward |
