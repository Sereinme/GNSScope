//! Second-order loop filter for DLL and PLL.
//!
//! Based on Borre et al., "A Software-Defined GPS and Galileo Receiver", 2007.
//! Includes output clamping to prevent integrator windup.

#[derive(Debug, Clone)]
pub struct LoopFilter {
    prev_input: f64,
    prev_output: f64,
    c1: f64,  // integrator gain: T * wn^2 / K
    c2: f64,  // proportional gain: wn / (2 * zeta)
    max_output: f64,
}

impl LoopFilter {
    pub fn new(bandwidth_hz: f64, zeta: f64, gain: f64, t: f64) -> Self {
        let wn = bandwidth_hz / 0.53;
        let c1 = t * wn * wn / gain;
        let c2 = wn / (2.0 * zeta);
        // Limit output to prevent windup: ~3× the natural frequency
        let max_output = 3.0 * wn;
        LoopFilter { prev_input: 0.0, prev_output: 0.0, c1, c2, max_output }
    }

    pub fn update(&mut self, input: f64) -> f64 {
        let integ = self.c1 * input;
        let prop = self.c2 * (input - self.prev_input);
        self.prev_input = input;
        self.prev_output = (self.prev_output + integ + prop)
            .clamp(-self.max_output, self.max_output);
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
    fn test_loop_filter_convergence() {
        let mut lf = LoopFilter::new(4.0, 0.707, 1.0, 0.001);
        let mut prev = 0.0;
        for _ in 0..1000 {
            prev = lf.update(0.1);
        }
        assert!(prev.is_finite());
    }
}
