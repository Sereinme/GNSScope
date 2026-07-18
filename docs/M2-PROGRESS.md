# M2 PVT 管线进度总结

**日期**: 2026-07-18
**状态**: PVT 管线端到端打通，纬度精确（33m），经度/高度偏差因伪距亚 ms 不准（已定位根因，待修复）

---

## 已完成

### PVT 核心模块（全部实现 + 单元测试通过）

| 模块 | 文件 | 功能 | 测试 |
|---|---|---|---|
| 星历解析 | `nav/ephemeris.rs` | subframe 1-3 → Ephemeris（IS-GPS-200 位偏移，多段字段，IODC/IODE 校验），subframe 4 → Klobuchar iono | ✓ |
| 坐标转换 | `pvt/coords.rs` | WGS84 ECEF↔LLA（Bowring 迭代） | ✓ CTTC 真值 round-trip |
| 卫星位置 | `pvt/satpos.rs` | Kepler 轨道 + 钟差（含相对论项） | ✓ 收敛性 |
| 大气改正 | `pvt/atmosphere.rs` | Klobuchar 电离层 + Saastamoinen 对流层 | ✓ 典型值范围 |
| 伪距形成 | `pvt/pseudorange.rs` | 绝对伪距 P = c·(T_rx − T_tx) | ✓ 符号 |
| WLS 解算 | `pvt/solver.rs` | 迭代加权最小二乘（nalgebra，C/N0 加权，Sagnac 改正） | ✓ |

### 修正项（全部实现）

| 修正 | 位置 | 公式 | 状态 |
|---|---|---|---|
| 相对论钟差 | satpos.rs:71 | `Δt_rel = -2√(μa)·e·sinE/c²` | ✓ IS-GPS-200 标准 |
| 地球自转（Sagnac） | solver.rs:98-109 | 绕 z 轴旋转 θ = ω_e·τ | ✓ 与 RTKLIB 一阶等价 |
| 广播钟差多项式 | satpos.rs:72 | `af0 + af1·tk + af2·tk²` | ✓ |
| 组延迟 TGD | satpos.rs:72 | `- tgd`（L1 单频） | ✓ |
| 电离层 Klobuchar | solver.rs:128 | 8 参数 α/β | ✓ 实现，但 iono.valid=false（未解码 sf4），实际未应用 |
| 对流层 Saastamoinen | solver.rs:131 | 标准大气 | ✓ |

### 跟踪/解码修复（本次会话）

| 修复 | Commit | 内容 |
|---|---|---|
| PLL 公式 | `160d1b8` | 精确移植 GNSS-SDR Tracking_FLL_PLL_filter |
| DLL 符号 | `160d1b8` | `update(-disc)` → `update(disc)` |
| GPS NAV parity | `45173c4` | p29 多 d3，p30 少 d4（IS-GPS-200） |
| 比特同步 | `aefd827` | 直方图 BitSynchronizer |
| 星历位偏移 | `06e6e28` | 1-based D 编号 + 多段字段 |
| Preamble 阈值 | `06e6e28` | 5→7（消除伪 sync） |
| NavDecoder 重置 | `c8c2c04` | decoded 后重置，搜索下一 preamble |
| NavDecoder TOW | `8ba6caa` | tow_at_current_symbol_ms（GNSS-SDR 模式） |

### PVT 解算结果（CTTC Spain 真实数据，5 颗卫星）

```
纬度  41.274995°  真值 41.2747°  误差 33m ✓
经度  1.937671°   真值 1.9875°  误差 5.5km
高度  4057m        真值 52m      误差 4km
卫星数 5，收敛是（4 次迭代）
```

---

## 当前问题：伪距亚 ms 不准导致经度/高度偏差

### 问题确证

用真值位置反算每卫星残差（PR + clk_sv − range_true − bias）：

| PRN | 伪距 (m) | 真值几何距离 (m) | resid_true (m) |
|---|---|---|---|
| 1 | 23002903 | 20681225 | -2261 |
| 11 | 24247885 | 21809820 | -419 |
| 17 | 24500174 | 22187875 | -5526 |
| 20 | 22483602 | 20186818 | -3705 |
| 32 | 23587481 | 21096872 | -1549 |

**残差不一致**（-419 到 -5526m，差 5107m）。如果伪距正确，所有残差应一致（差一个公共钟差）。**5107m 的不一致直接导致 WLS 解偏——确证伪距不正确。**

### 根因分析

#### 1. code_phase_chips 是 NCO 累加器，非信号码相位

`DllPllTracking.code_phase_chips` 是本地码 NCO 累加器（`self.code_phase_chips += code_freq × t`，mod 1023）。它从 acquisition 的 `code_phase = (1023 − raw_phase) % 1023` 初始化。

DLL 锁定后 NCO 追随信号码相位（SCP），但保持初始偏移：
```
NCO(t) − SCP(t) = 常数 = acq_cp − SCP(0) = acq_cp − (1023 − acq_cp) = 2×acq_cp − 1023
```

验证：NDO 在 60s 内的变化 = 码 Doppler × 60 ≈ 282 chips（各卫星一致）✓。但 NDO 绝对值 ≠ SCP。

#### 2. NDO vs 真实 SCP 对比

| PRN | NDO | SCP（恢复） | 真实 SCP = (travel mod 1ms)×1023 | 匹配？ |
|---|---|---|---|---|
| 1 | 276.8 | 269.6 | 1013 | ✗ |
| 11 | 120.5 | 114.6 | 777 | ✗ |
| 17 | 282.6 | 282.0 | 20 | ✗ |
| 20 | 2.8 | 997.2 | 348 | ✗ |
| 32 | 328.0 | 327.4 | 389 | ✗ |

NDO 和 SCP 都不等于真实信号码相位。SCP 恢复公式 `SCP = (NDO + 1023 − 2×acq_cp) mod 1023` 也不对（可能 acq 在跟踪过程中的码相位漂移导致偏移变化）。

#### 3. 为什么 cp/1023 仍给出最好位置

NDO 的**相对差异**（各卫星间）恰好部分编码了距离差，虽绝对值不对。WLS 用相对关系定位，所以纬度较准（33m）。但 5km 的残差不一致导致经度/高度偏。

#### 4. GNSS-SDR 的方法（对比）

GNSS-SDR **不用 code_phase_chips 计算伪距**。它的架构：

1. **Tracking** 输出 `Tracking_sample_counter`（样本计数器，精确到样本 0.25µs）+ `Code_phase_samples`（亚样本残余码相位，<1 sample）
2. **Telemetry** 维护 `TOW_at_current_symbol_ms`（子帧解码后设为 HOW TOW×1000，每符号 +20ms）
3. **Observables** 用样本计数器在符号间线性插值 TOW（`interp_TOW_ms`），给出亚 ms 精度
4. **伪距 = (T_rx_TOW − interp_TOW) × c**

关键：GNSS-SDR 的 `Code_phase_samples` 是**亚样本残余**（<1 sample），不是 0-1023 的完整码相位。它用于精化样本时间戳，不是直接进伪距公式。

我们的 `code_phase_chips` 是 0-1023 的完整码 NCO 相位，性质不同。

### 尝试过的方法

| 方法 | lat 误差 | lon 误差 | 说明 |
|---|---|---|---|
| **cp/1023（当前最好）** | **33m** | 5.5km | NDO 相对差异部分编码距离差 |
| (1023−cp)/1023 | 187km | 169km | 物理正确但整数 ms 抖动暴露 |
| TOW 插值（20ms 粒度） | 500km | — | 丢失整数 ms 距离差 |
| TOW 插值 + 1ms bit_idx | 4°偏 | 146°偏 | 1ms 粒度仍不够 |

---

## 待修复：跟踪层样本级时间戳

### 需要实现的

在 `DllPllTracking` 添加：
1. `Tracking_sample_counter`（u64）：从跟踪开始的样本计数器，每 ms +1（或按样本）
2. `rem_code_phase_samples`（f64）：亚样本残余码相位（<1 sample），即本 ms 内码相位的亚样本部分

### 伪距计算改为

```
T_rx_s = (Tracking_sample_counter + rem_code_phase_samples) / fs  // 精确接收时刻
T_tx = TOW_at_current_symbol_ms / 1000  // 发射时刻（telemetry 维护）
traveltime = T_rx_s - T_tx  // 含整数 ms + 亚 ms
pseudorange = c × traveltime
```

这绕过 NDO 累加器的偏移问题，用样本计数器直接给亚 ms 精度的接收时刻。

### 关键约束

- `TOW_at_current_symbol_ms` 已在 NavDecoder 实现（commit `8ba6caa`）
- 需要在跟踪层添加样本计数器输出
- 需要保证各通道在同一接收时刻的样本计数器对齐

---

## 未应用/可改进项

1. **电离层未实际应用**：Klobuchar 已实现，但 `iono.valid=false`（未解码 sf4 page 18）。需收集 sf4 并调用 `parse_iono`
2. **`extend_correlation_symbols`**：真·20ms 相干积分，弱信号下提升跟踪稳定性
3. **ADR-0002 trait 抽象**：保持当前具体类型，M2 后再引入
4. **速度解算**：Doppler-based，可后续加
5. **Kalman 滤波平滑**：LS 足够 M2 验证

---

## 提交历史

| Commit | 内容 |
|---|---|
| `160d1b8` | PLL 公式 + DLL 符号修复 |
| `45173c4` | GPS NAV parity 公式修复 |
| `aefd827` | 直方图比特同步，真实子帧解码 |
| `6f8a1b2` | PVT 核心模块（ephemeris/satpos/solver 等） |
| `c8c2c04` | NavDecoder 重置 bug + 多通道测试 |
| `06e6e28` | 星历位偏移修复 + preamble 阈值（5/5 星历） |
| `2bacc93` | 真实数据 PVT 解算成功（lat 33m） |
| `cb631e6` | 伪距精度调查：code_phase 不可直接用于伪距 |
| `8ba6caa` | NavDecoder 逐符号 TOW（GNSS-SDR 模式） |
| `4170a8e` | TOW 符号间插值（亚 20ms 粒度） |
| `27228b7` | 回退到 75ms 方法（最好位置） |
| `e37a251` | code_phase 分析：acq 约定确认 |
