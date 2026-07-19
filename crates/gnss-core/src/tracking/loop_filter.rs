//! Generic loop filter (1st-3rd order) based on GNSS-SDR tracking_loop_filter.cc
//! Uses bilinear transform of standard Wiener filter.

#[derive(Debug, Clone)]
pub struct LoopFilter {
    order: u32,
    t: f64,  // update interval
    bw: f64, // noise bandwidth Hz
    // History
    inputs: [f64; 4],
    outputs: [f64; 4],
    idx: usize,
    // Coefficients
    in_coef: Vec<f64>,
    out_coef: Vec<f64>,
}

impl LoopFilter {
    pub fn new(bw: f64, _zeta: f64, _gain: f64, t: f64) -> Self {
        // Default: 2nd order (backward compatible)
        Self::new_order(bw, t, 2)
    }

    pub fn new_order(bw: f64, t: f64, order: u32) -> Self {
        let mut lf = Self {
            order,
            t,
            bw,
            inputs: [0.0; 4],
            outputs: [0.0; 4],
            idx: 3,
            in_coef: Vec::new(),
            out_coef: Vec::new(),
        };
        lf.update_coefficients();
        lf
    }

    pub fn update(&mut self, input: f64) -> f64 {
        let mut result = 0.0f64;
        // Old outputs
        for (i, &c) in self.out_coef.iter().enumerate() {
            result += c * self.outputs[(self.idx + i) % 4];
        }
        // Update index
        self.idx = (self.idx + 4 - 1) % 4;
        self.inputs[self.idx] = input;
        // New inputs
        for (i, &c) in self.in_coef.iter().enumerate() {
            result += c * self.inputs[(self.idx + i) % 4];
        }
        self.outputs[self.idx] = result;
        result
    }

    pub fn reset(&mut self) {
        self.inputs = [0.0; 4];
        self.outputs = [0.0; 4];
        self.idx = 3;
    }

    pub fn state(&self) -> f64 {
        self.outputs[self.idx]
    }

    pub fn initialize(&mut self, initial_output: f64) {
        self.inputs = [0.0; 4];
        self.outputs = [initial_output; 4];
        self.idx = 3;
    }

    fn update_coefficients(&mut self) {
        let zeta = 1.0_f64 / 2.0_f64.sqrt();
        let t = self.t;
        match self.order {
            2 => {
                let wn = self.bw * 8.0 * zeta / (4.0 * zeta * zeta + 1.0);
                let g1 = wn * wn;
                let g2 = wn * 2.0 * zeta;
                // include_last_integrator = false
                self.in_coef = vec![g1 * t / 2.0 + g2, g1 * t / 2.0 - g2];
                self.out_coef = vec![1.0];
            }
            3 => {
                let wn = self.bw / 0.7845;
                let a3 = 1.1;
                let b3 = 2.4;
                let g1 = wn * wn * wn;
                let g2 = a3 * wn * wn;
                let g3 = b3 * wn;
                // include_last_integrator = false
                self.in_coef = vec![
                    g3 + t / 2.0 * (g2 + t / 2.0 * g1),
                    g1 * t * t / 2.0 - 2.0 * g3,
                    g3 + t / 2.0 * (-g2 + t / 2.0 * g1),
                ];
                self.out_coef = vec![2.0, -1.0];
            }
            _ => {
                // 1st order fallback
                let wn = self.bw * 4.0;
                self.in_coef = vec![wn];
                self.out_coef = vec![];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_3rd_order_finite() {
        let mut lf = LoopFilter::new_order(40.0, 0.001, 3);
        for _ in 0..1000 {
            let o = lf.update(0.1);
            assert!(o.is_finite());
        }
    }
}
