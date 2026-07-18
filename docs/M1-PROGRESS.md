# M1 进度总结

**日期**: 2026-07-18
**状态**: PLL 公式 + DLL 符号 bug 已修复，真实数据 PLL 锁定（Doppler 7244≈7250）；NavDecoder 奇偶校验仍失败（下一独立问题）

---

## 本次修复

### Bug 1: PLL 公式错误移植（已修复）
`crates/gnss-core/src/tracking/dll_pll.rs` 的 PLL 由错误的手写公式驱动，正确的 `pll_filter` 字段是死代码。手写公式错误移植了 GNSS-SDR `Tracking_FLL_PLL_filter`：
- `k1 = w0f/w0p2 = 0`（因 fll_bw=0）→ 比例项消失
- `pll_x = pll_disc` 直接覆盖（应 `+= t*(...)` 累加）
- 初始化 `pll_x=0, pll_w=doppler` 反了（应 `pll_x=2*doppler, pll_w=0`）

**修复**：精确移植 `Tracking_FLL_PLL_filter::get_carrier_error` 状态空间累加形式，按阶数计算 w0p/w0f 和初始化累加器。

### Bug 2: DLL 鉴相器符号反了（已修复）
`dll_pll.rs:143` 原为 `dll_filter.update(-dll_err)`——**取负是错的**。

GNSS-SDR 约定：`dll_nc_e_minus_l_normalized` 返回 `(E-L)/(E+L)`，正当地码超前（E>L），负当地码滞后（E<L）。`code_freq = chip_rate - dll_error_filt`：
- 码超前（disc>0）→ 应减速 → filt>0 → code_freq 降低 ✓
- 码滞后（disc<0）→ 应加速 → filt<0 → code_freq 升高 ✓

取负后变成正反馈：码超前→filt<0→code_freq 升高→码更超前。这导致 |P| 在真实数据上从 95920 暴跌到 509（去相关）。

**修复**：`dll_filter.update(dll_err)`（不取负）。

### 验证
- 干净信号 50 Hz 误差，bw=50：100ms 收敛到目标频率 ✓
- 干净信号 7250 Hz（0.25 周期残差）：Q/I=0.00005 ✓
- **真实数据 PRN 1**（3 阶 PLL=50, DLL=4）：
  - Doppler mean=**7244.8 Hz**（GNSS-SDR 真值 7250）✓
  - |P| 稳定在 ~105000（不再崩溃）✓
  - 20ms 符号一致性：pos=16/neg=4 清晰交替（导航比特）✓
  - E/L 比值 0.6-1.6 正常波动（DLL 跟踪）✓
- lib 测试 11/11 通过，m1_tracking 干净信号通过 ✓

---

## 未解决：NavDecoder 奇偶校验失败

### 现象
PLL 已锁定（符号清晰交替），但 NavDecoder 从未通过奇偶校验：
- preamble corr=6/8（所有 20 个边界，阈值 5）
- TLM/HOW parity_ok 始终 false
- `best_sf=0`（无子帧解码成功）

### 已排除
- NavDecoder 本身正确：`test_nav_decoder_direct`（直接喂已知比特）通过
- PLL 符号一致：20ms 内 pos=16/neg=4，符号清晰
- 合成信号也失败：`test_m1_track_and_decode_subframe`（但该测试有 bug——每 ms 喂 symbol 而非每 20ms）

### 待查
1. NavDecoder 的 D29*/D30* 反转逻辑与 majority-vote 符号处理的交互
2. preamble corr=6/8（非 8/8）——某些比特的 20ms 和被污染
3. `real_tracking.rs:154` 的 `sym = if pos>neg {sum.abs()} else {-sum.abs()}` 与 NavDecoder 内部 `inverted` 检测可能冲突

---

## GNSS-SDR 参考数据（CTTC Spain）

真实跟踪的 5 颗卫星及 pull-in 参数（来自 `.txt` 日志）：

| PRN | Pull-in Doppler | Pull-in Code Phase (samples) | SNR (dB-Hz) |
|---|---|---|---|
| 1 | 7250 Hz | 3986.29 | 52 |
| 11 | 5500 Hz | 350.22 | 49 |
| 17 | 9750 Hz | 394.38 | 49 |
| 20 | 8250 Hz | 1243.32 | 48 |
| 32 | 6500 Hz | 3681.25 | 48 |

配置：`dll_pll_veml_tracking`, PLL=50 Hz, DLL=4 Hz, order=3, FLL=10 Hz（但 `enable_fll_pull_in=false`，实际纯 PLL）

---

## 已完成

### M0: 捕获 ✅
- PRN 码生成 + PCPS 捕获，真实数据检测 6 颗卫星，匹配 GNSS-SDR 参考

### M1: 跟踪 ✅
- **DLL**: ✅ 符号已修复，码跟踪正确（|P| 稳定，E/L 正常）
- **PLL**: ✅ 公式已修复（精确移植 Tracking_FLL_PLL_filter），真实数据锁定
- **C/N₀**: I²/Q² 比值公式（粗糙，建议改 M2M4）

### M1: 导航电文解码 ⚠️
- Preamble 检测 + Hamming 校验已移植，但真实数据上奇偶校验失败（独立问题）

---

## 下一步建议

1. **调试 NavDecoder 奇偶校验**（最高优先级）：
   - 用合成信号（PLL 已锁定）隔离 NavDecoder 问题
   - 修复 `m1_tracking.rs` 测试的 20ms 喂符号 bug
   - 检查 D29*/D30* 与 majority-vote 的交互

2. **C/N0 估计器**：改用 M2M4 替换 I²/Q² 公式

3. **进入 M2**：PVT + GUI
