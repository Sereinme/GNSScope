//! Second-order loop filter matching GNSS-SDR Tracking_2nd_PLL_filter.
//!
//! Reference: Borre et al., "A Software-Defined GPS and Galileo Receiver", 2007.
//! Trapezoidal integration, gain K=0.25 for PLL, K=1.0 for DLL.

#[derive(Debug, Clone)]
pub struct LoopFilter {
    prev_input: f64,
    prev_output: f64,
    /// tau2/tau1 (proportional-differential gain)
    k1: f64,
    /// T/(2*tau1) (trapezoidal integration gain)
    k2: f64,
}

impl LoopFilter {
    /// Create a 2nd-order loop filter.
    ///
    /// - `bandwidth_hz`: Loop noise bandwidth
    /// - `zeta`: Damping factor (0.7 for GPS)
    /// - `gain_k`: Discriminator gain (0.25 for PLL, 1.0 for DLL per GNSS-SDR)
    /// - `t`: Integration time in seconds
    pub fn new(bandwidth_hz: f64, zeta: f64, gain_k: f64, t: f64) -> Self {
        // Natural frequency: GNSS-SDR formula
        let wn = bandwidth_hz * 8.0 * zeta / (4.0 * zeta * zeta + 1.0);
        // Time constants
        let tau1 = gain_k / (wn * wn);
        let tau2 = 2.0 * zeta / wn;
        // Filter gains
        let k1 = tau2 / tau1;                    // differential gain
        let k2 = t / (2.0 * tau1);               // trapezoidal integration gain
        LoopFilter { prev_input: 0.0, prev_output: 0.0, k1, k2 }
    }

    /// Process input, return filtered output.
    ///
    /// GNSS-SDR formula:
    ///   y[n] = y[n-1] + k1*(x[n] - x[n-1]) + k2*(x[n] + x[n-1])
    ///
    /// PLL: input in Hz, output in Hz/s.
    /// DLL: input in chips, output in chips/s.
    pub fn update(&mut self, input: f64) -> f64 {
        let diff = self.k1 * (input - self.prev_input);
        let integ = self.k2 * (input + self.prev_input);
        self.prev_input = input;
        self.prev_output += diff + integ;
        self.prev_output
    }

    pub fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }

    pub fn state(&self) -> f64 { self.prev_output }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_finite() {
        let mut lf = LoopFilter::new(25.0, 0.7, 0.25, 0.001);
        for _ in 0..1000 {
            let o = lf.update(0.1);
            assert!(o.is_finite());
        }
    }
}
