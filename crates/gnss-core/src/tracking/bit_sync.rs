//! Histogram-based bit-edge synchronizer for GPS L1 C/A.
//!
//! GPS nav bits last 20 ms; the 1 ms prompt correlator output changes sign at
//! bit boundaries. We histogram the phase (0..20) at which sign transitions
//! occur; the phase with the most transitions is the bit boundary.
//!
//! Port of GNSS-SDR's `HistogramBitSynchronizer` (simplified: sign-detector
//! branch only, no tracking-quality gating).

const BITS_PER_SYMBOL: usize = 20; // 20 ms per nav bit at 1 ms prompt rate

/// Histogram-based bit synchronizer. Feed 1 ms prompt samples; once locked,
/// `bit_boundary()` reports the phase (0..20) at which nav bits start.
pub struct BitSynchronizer {
    hist: [u32; BITS_PER_SYMBOL],
    epoch: usize,      // total 1 ms samples seen
    total_events: u32, // sign transitions counted
    last_sign: i8,     // sign of previous prompt (±1)
    has_last: bool,
    locked: bool,
    edge_phase: usize,
    // Stability: the best bin must stay the same for several evaluations
    last_best_bin: usize,
    stable_best_count: u32,
}

impl Default for BitSynchronizer {
    fn default() -> Self {
        Self::new()
    }
}

impl BitSynchronizer {
    pub fn new() -> Self {
        Self {
            hist: [0; BITS_PER_SYMBOL],
            epoch: 0,
            total_events: 0,
            last_sign: 1,
            has_last: false,
            locked: false,
            edge_phase: 0,
            last_best_bin: 0,
            stable_best_count: 0,
        }
    }

    /// Feed one 1 ms prompt sample (I component). Returns Some(boundary) when
    /// lock is first acquired, else None.
    pub fn update(&mut self, prompt_i: f64) -> Option<usize> {
        let phase = self.epoch % BITS_PER_SYMBOL;
        self.epoch += 1;

        let sign = if prompt_i >= 0.0 { 1i8 } else { -1i8 };
        let mut edge_event = false;
        if self.has_last && sign != self.last_sign {
            edge_event = true;
        }
        self.last_sign = sign;
        self.has_last = true;

        if edge_event {
            self.hist[phase] += 1;
            self.total_events += 1;
        }

        // Evaluate lock once we have enough transitions.
        // Rule of thumb: ~1 transition per 2 bits → in 100 bits ~50 transitions.
        const MIN_EVENTS: u32 = 30;
        const DOMINANCE_RATIO: f64 = 0.4; // best bin must hold ≥40% of transitions
        const STABLE_BEST_REQUIRED: u32 = 3; // best bin must be stable 3 evaluations

        if !self.locked && self.total_events >= MIN_EVENTS {
            let (best_bin, best_count) = self.best_bin();
            let ratio = best_count as f64 / self.total_events as f64;
            if best_bin != self.last_best_bin {
                self.last_best_bin = best_bin;
                self.stable_best_count = 1;
            } else {
                self.stable_best_count += 1;
            }
            if ratio >= DOMINANCE_RATIO && self.stable_best_count >= STABLE_BEST_REQUIRED {
                self.locked = true;
                self.edge_phase = best_bin;
                return Some(best_bin);
            }
        }
        None
    }

    pub fn locked(&self) -> bool {
        self.locked
    }
    pub fn bit_boundary(&self) -> usize {
        self.edge_phase
    }

    /// Phase of the current 1 ms sample within the 20 ms bit (0..20).
    pub fn current_phase(&self) -> usize {
        self.epoch % BITS_PER_SYMBOL
    }

    /// True if the current 1 ms sample is the first of a new bit.
    pub fn is_bit_edge(&self) -> bool {
        self.locked && self.current_phase() == self.edge_phase
    }

    fn best_bin(&self) -> (usize, u32) {
        let mut best = 0;
        let mut best_count = self.hist[0];
        for i in 1..BITS_PER_SYMBOL {
            if self.hist[i] > best_count {
                best_count = self.hist[i];
                best = i;
            }
        }
        (best, best_count)
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic: nav bits of 20 ms each with known boundary at phase 0.
    /// Sign transitions only at bit edges → histogram peaks at 0.
    #[test]
    fn test_detects_boundary() {
        let mut bs = BitSynchronizer::new();
        // 50 bits, alternating sign, boundary at phase 0
        let mut prompts: Vec<f64> = Vec::new();
        for bit in 0..50 {
            let amp = if bit % 2 == 0 { 100.0 } else { -100.0 };
            for _ in 0..20 {
                prompts.push(amp);
            }
        }
        let mut locked_at = None;
        for (i, &p) in prompts.iter().enumerate() {
            if let Some(b) = bs.update(p) {
                locked_at = Some((i, b));
                break;
            }
        }
        let (_, boundary) = locked_at.expect("should lock");
        assert_eq!(boundary, 0, "boundary should be 0, got {}", boundary);
        assert!(bs.locked());
    }

    /// Boundary at phase 7: first 7 samples of bit 0 are positive, then bit
    /// changes at sample 7. Histogram should peak at 7.
    #[test]
    fn test_offset_boundary() {
        let mut bs = BitSynchronizer::new();
        // Build a stream where bit edges fall at phase 7 (mod 20).
        // Bit 0: samples 0..7 positive, bit 1: 7..27 negative, bit 2: 27..47 positive, ...
        let mut prompts: Vec<f64> = Vec::new();
        let mut sign = 100.0;
        let mut next_edge = 7;
        for s in 0..1000 {
            if s == next_edge {
                sign = -sign;
                next_edge += 20;
            }
            prompts.push(sign);
        }
        let mut boundary = None;
        for p in prompts {
            if let Some(b) = bs.update(p) {
                boundary = Some(b);
                break;
            }
        }
        assert_eq!(boundary.unwrap(), 7);
    }
}
