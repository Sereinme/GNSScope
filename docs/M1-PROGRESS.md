# M1 进度总结

**日期**: 2025-07-18  
**状态**: 核心管线完成，PLL 跟踪质量限制子帧解码

---

## 已完成

### M0: 捕获 ✅
- PRN 码生成：32 个 GPS L1 C/A Gold 码，IS-GPS-200 验证通过
- PCPS 捕获：基于 GNSS-SDR 算法，RustFFT
- 真实数据验证：6 颗卫星检测（PRN 1,11,17,20,27,32），匹配 GNSS-SDR 参考

### M1: 跟踪 ✅ (部分)
- **DLL**: 二阶环路滤波器，carrier aiding，码相位跟踪正确
- **PLL**: 移植了 GNSS-SDR 的三种滤波器实现：
  - `tracking_2nd_PLL_filter`（二阶 Euler 积分）
  - `Tracking_FLL_PLL_filter`（三阶 FLL+PLL 级联）
  - `tracking_loop_filter`（双线性变换，1-3 阶）
- **FLL**: `fll_diff_atan` 精确移植（phase_unwrap ±π/2）
- **C/N₀**: I²/Q² 比值公式
- **真实数据跟踪**: PLL 锁定 60s+，Doppler 稳定 6620→6550 Hz

### M1: 导航电文解码 ✅ (部分)
- **Preamble 检测**: 软相关（Σ sign(symbol) × preamble[i]），corr=8
- **Hamming 码校验**: 精确移植 GNSS-SDR `gps_word_parityCheck`
  - D29*/D30* 来自前一个字
  - D30* = 1 时反转数据位
- **TLM**: 65% 通过率（10s 窗口 + majority vote）
- **HOW**: 19% 通过率（majority vote），但与 TLM 不同时通过

### 工程基础
- Cargo workspace: `gnss-core` / `gnss-data` / `gnss-gui`
- IQ 文件读取器: int8/int16 交错格式
- 4→2Msps 重采样器
- 诊断测试: PLL/FLL 鉴相器值、Doppler、Q/I 全程监控
- GNSS-SDR 源码克隆到 `reference/gnss-sdr/` 作为参考

---

## 未完成

### PLL 跟踪质量 ⚠️
- **核心问题**: 1ms 相干积分下 Costas PLL 无法完全锁相
- Q/I = 0.019-0.028（接近锁定但存在残余相位旋转）
- 1ms prompt I 值在 20ms 窗口内正负波动（相位未完全锁定）
- **根因分析**:
  - 正确的 `tracking_2nd_PLL_filter` 公式（k1=530）放大鉴相器噪声
  - 旧的错误公式（k1≈0）意外作为纯积分器工作，Q/I=0.019
  - GNSS-SDR 使用相同公式但成功——差异可能在相关器输出缩放
- **GNSS-SDR 配置**:
  - CTTC Spain: `GPS_L1_CA_DLL_PLL_Tracking`, PLL=50Hz, DLL=4Hz, order=3
  - 现代: `dll_pll_veml_tracking`, PLL=40Hz, DLL=4Hz, order=3, no FLL
  - `extend_correlation_symbols` 默认 1（1ms）

### 子帧解码 ❌
- TLM 和 HOW 从未在同一子帧同时通过奇偶校验
- **原因 1**: BER ~3%（1ms 积分），30 位字的通过率 ~6%
- **原因 2**: 码相位漂移导致 20ms 窗口在 100s 内偏移 ~0.4 比特
- **原因 3**: majority vote 改善了 TLM（65%）但 HOW 仍为 0
- **可能的 D30* 反转 bug**: TLM 和 HOW 在不同边界通过（b14 vs b12）

---

## 关键技术发现

1. **Acq Doppler 精度**: acq 给出 7000 Hz，真实值 ~6620 Hz，误差 380 Hz
   - 超过 Costas PLL 1ms 拉入范围（±250 Hz）
   - 需要精细搜索（20 Hz 步长）才能找到可锁定频率

2. **PLL 公式陷阱**: `tracking_2nd_PLL_filter` 的 `k1 = tau2/tau1` 值很大（530）
   - 正比例项放大 1ms 鉴相器噪声
   - GNSS-SDR 可能依赖 VOLK 优化的相关器输出幅值

3. **码相位漂移**: 100s 内 4.3 chips/s 的码 Doppler 导致 20ms 窗口偏移
   - 需要用跟踪的码相位动态确定比特边界

4. **Majority vote vs 相干求和**: 
   - 相干求和（20×1ms I 值求和）受相位旋转影响
   - Majority vote 对相位旋转更鲁棒，TLM 通过率 6%→65%

---

## 下一步建议

1. **修复 PLL 锁定**: 
   - 对比 GNSS-SDR 相关器输出幅值，确认是否需要缩放
   - 或使用旧的纯积分器公式（k1≈0）配合更窄带宽（5-10 Hz）

2. **实现 `extend_correlation_symbols`**:
   - 先做比特同步（直方图法），确定 20ms 边界
   - 然后在跟踪内部做 5-20ms 相干积分
   - 这是 GNSS-SDR 在弱信号下成功的关键

3. **动态比特边界**:
   - 用跟踪的码相位（code_phase_chips）确定当前比特位置
   - 每 20ms 重新对齐，而非固定 `bit_idx += 20`

4. **进入 M2**: 
   - PLL 稳定性可在 M2 中迭代优化
   - PVT 解算和 GUI 不依赖完美解码
   - 可用合成信号验证完整管线
