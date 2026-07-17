# ADR-0002: 信号处理 trait 抽象与多频点预留

**日期**: 2025-07-18  
**状态**: 已接受

## 背景

GNSScope 的首个目标是 GPS L1 C/A，但用户明确要求为后续频点（GPS L1C、L2C、L5 等）预留扩展接口。不同 GNSS 信号的差异体现在：PRN 码长度与速率、导航电文结构、调制方式（BPSK/BOC/TMBOC）、导频/数据通道分离等。

## 决策

`gnss-core` 的信号处理管线采用 **trait-based 抽象**，当前仅实现 GPS L1 C/A：

```rust
// 信号描述 trait
trait SignalSpec {
    fn center_frequency() -> f64;      // 中心频率 (Hz)
    fn code_rate() -> f64;             // 码速率 (chips/s)
    fn code_length() -> usize;         // PRN 码长度 (chips)
    fn prn_count() -> usize;           // 卫星数量
}

// 捕获 trait
trait Acquisition {
    type Signal: SignalSpec;
    fn search(&mut self, samples: &[Complex<f64>]) -> Option<AcquisitionResult>;
}

// 跟踪 trait
trait Tracking {
    type Signal: SignalSpec;
    fn process(&mut self, samples: &[Complex<f64>]) -> TrackingOutput;
}

// 导航电文解码 trait
trait NavDecoder {
    type Signal: SignalSpec;
    fn decode(&mut self, symbols: &[f64]) -> Option<NavMessage>;
}
```

### 当前实现

- `gps_l1ca::GpsL1Ca` 实现 `SignalSpec`
- `gps_l1ca::PcpsAcquisition` 实现 `Acquisition`
- `gps_l1ca::DllPllTracking` 实现 `Tracking`
- `gps_l1ca::L1CaNavDecoder` 实现 `NavDecoder`

### 后续频点

新频点只需添加对应模块实现相同 trait：
```
src/signals/
├── gps_l1ca.rs      ← 当前实现
├── gps_l1c.rs       ← 未来
├── gps_l2c.rs       ← 未来
└── gps_l5.rs        ← 未来
```

## 后果

- **正向**：添加新信号时核心管线（通道调度、PVT）无需改动
- **正向**：单元测试可以针对每个信号独立编写
- **代价**：初期需要多写 trait 定义和模块骨架，但 GPS L1 C/A 的具体实现不受影响
- **注意**：不同信号的 PVT 观测值可能略有差异（如 L2C 的双频无电离层组合），届时需要扩展 observables 模块
