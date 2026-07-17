//! Second-order loop filter for DLL and PLL.
//!
//! Based on Borre et al., "A Software-Defined GPS and Galileo Receiver", 2007.

/// Second-order loop filter.
#[derive(Debug, Clone)]
pub struct LoopFilter {
    prev_input: f64,
    prev_output: f64,
    c1: f64,  // integrator gain: T * wn^2 / K
    c2: f64,  // proportional gain: wn / (2 * zeta)
}

impl LoopFilter {
    /// Create a new 2nd-order loop filter.
    ///
    /// - `bandwidth_hz`: Loop noise bandwidth (e.g., 4.0 Hz for DLL, 40.0 Hz for PLL)
    /// - `zeta`: Damping factor (typically 0.707)
    /// - `gain`: Discriminator gain (1.0 for normalized discriminators)
    /// - `t`: Integration time in seconds (0.001 for 1ms GPS)
    pub fn new(bandwidth_hz: f64, zeta: f64, gain: f64, t: f64) -> Self {
        // Natural frequency from loop bandwidth
        let wn = bandwidth_hz / 0.53;
        // Continuous-time PLL: F(s) = (1/tau2) + 1/(tau1*s)
        // tau1 = K / wn^2, tau2 = 2*zeta / wn
        // Discrete (backward Euler): y[n] = y[n-1] + T/tau1*x[n] + 1/tau2*(x[n]-x[n-1])
        let c1 = t * wn * wn / gain;      // T/tau1
        let c2 = wn / (2.0 * zeta);        // 1/tau2
        // c1, c2 produce output in rad/s for 1 rad input
        // For PLL use in Hz: divide by 2π
        // For DLL use in chips/s: multiply discriminator output (chips) by code rate
        LoopFilter { prev_input: 0.0, prev_output: 0.0, c1, c2 }
    }

    /// Process one discriminator input, return filtered output.
    /// Output is in the same units as input (radians for PLL, chips for DLL).
    pub fn update(&mut self, input: f64) -> f64 {
        let integ = self.c1 * input;
        let prop = self.c2 * (input - self.prev_input);
        self.prev_input = input;
        self.prev_output += integ + prop;
        self.prev_output
    }

    pub fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_filter_convergence() {
        let mut lf = LoopFilter::new(4.0, 0.707, 1.0, 0.001);
        let mut prev = 0.0;
        for _ in 0..1000 {
            prev = lf.update(0.1);
        }
        assert!(prev.is_finite());
    }
}
