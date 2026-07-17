//! GPS L1 C/A navigation message decoding.
//!
//! Preamble detection via soft correlation (GNSS-SDR approach):
//!   corr = Σ sign(symbol) × preamble[i]   → detected when |corr| ≥ threshold
//!
//! Hamming parity check and subframe parsing.

/// GPS L1 C/A navigation message preamble: 10001011 (binary)
pub const PREAMBLE: u8 = 0x8B;
/// Preamble length in bits
pub const PREAMBLE_LEN: usize = 8;
/// Bits per word
pub const WORD_BITS: usize = 30;
/// Words per subframe
pub const WORDS_PER_SUBFRAME: usize = 10;
/// Bits per subframe
pub const SUBFRAME_BITS: usize = WORD_BITS * WORDS_PER_SUBFRAME; // 300

/// Preamble samples: +1 for bit=1, -1 for bit=0
const PREAMBLE_SAMPLES: [i8; PREAMBLE_LEN] = [1, -1, -1, -1, 1, -1, 1, 1];

#[derive(Debug, Clone, Copy)]
pub struct NavWord {
    pub raw: u32,
    pub parity_ok: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct How {
    pub tow: u32,
    pub subframe_id: u8,
    pub anti_spoof: bool,
    pub alert: bool,
}

/// Navigation message decoder.
///
/// Stores recent symbols (floating-point), performs soft preamble correlation.
pub struct NavDecoder {
    /// Ring buffer of recent symbols (20ms accumulations)
    sym_buf: [f64; 16], // 2× PREAMBLE_LEN for wrap
    sym_idx: usize,
    /// Subframe bit buffer
    subframe_bits: [u8; SUBFRAME_BITS],
    subframe_idx: usize,
    /// Frame sync state
    synced: bool,
    /// Number of preamble detections since last decode
    preamble_count: u32,
    /// Previous word's last two parity bits (D29*, D30*)
    prev_d29: u8, prev_d30: u8,
    /// Bit counter for preamble period (300 symbols per subframe)
    bit_counter: u32,
    /// Last preamble index
    preamble_idx: u32,
    /// Corr threshold
    corr_threshold: i32,
    /// Detected 180° phase ambiguity
    pub inverted: bool,
    /// Current TOW (from last valid HOW)
    pub tow: u32,
    /// Current subframe ID
    pub subframe_id: u8,
    /// Words decoded
    pub words: Vec<NavWord>,
}

impl NavDecoder {
    pub fn new() -> Self {
        Self {
            sym_buf: [0.0; 16], sym_idx: 0,
            subframe_bits: [0u8; SUBFRAME_BITS], subframe_idx: 0,
            synced: false, preamble_count: 0,
            prev_d29: 0, prev_d30: 0,
            bit_counter: 0, preamble_idx: 0,
            corr_threshold: 5, // allow weaker matches on real data
            inverted: false,
            tow: 0, subframe_id: 0,
            words: Vec::new(),
        }
    }

    /// Feed one symbol (20ms accumulated I value).
    /// Returns true if a valid subframe was decoded.
    pub fn feed_symbol(&mut self, symbol: f64) -> bool {
        // Store in ring buffer
        self.sym_buf[self.sym_idx % 16] = symbol;
        self.sym_idx += 1;
        self.bit_counter += 1;

        // Check if we have enough stored symbols for preamble correlation
        if self.sym_idx < PREAMBLE_LEN as usize { return false; }

        // Preamble correlation on the last 8 symbols
        let start = (self.sym_idx - PREAMBLE_LEN as usize) % 16;
        let mut corr: i32 = 0;
        for i in 0..PREAMBLE_LEN {
            let s = self.sym_buf[(start + i) % 16];
            let sign = if s > 0.0 { 1i8 } else { -1i8 };
            corr += (sign * PREAMBLE_SAMPLES[i]) as i32;
        }
        // --- Phase 0: searching for preamble ---
        if !self.synced {
            if corr.abs() >= self.corr_threshold {
                // Preamble found!
                self.synced = true;
                self.inverted = corr < 0;
                self.preamble_idx = self.bit_counter - PREAMBLE_LEN as u32;
                self.subframe_idx = 0;
                self.words.clear();

                // Fill subframe bits from stored symbols
                let bit_start = (self.sym_idx - PREAMBLE_LEN as usize) % 16;
                for i in 0..PREAMBLE_LEN {
                    let s = self.sym_buf[(bit_start + i) % 16];
                    let bit = if self.inverted { if s > 0.0 { 0u8 } else { 1u8 } }
                              else { if s > 0.0 { 1u8 } else { 0u8 } };
                    if self.subframe_idx < SUBFRAME_BITS {
                        self.subframe_bits[self.subframe_idx] = bit;
                        self.subframe_idx += 1;
                    }
                }
                // Reset parity for TLM and HOW
                self.prev_d29 = 0;
                self.prev_d30 = 0;
            }
            return false;
        }

        // --- Phase 1: collecting subframe ---
        // Store new symbol bit
        let bit = if self.inverted { if symbol > 0.0 { 0u8 } else { 1u8 } }
                  else { if symbol > 0.0 { 1u8 } else { 0u8 } };
        if self.subframe_idx < SUBFRAME_BITS {
            self.subframe_bits[self.subframe_idx] = bit;
            self.subframe_idx += 1;
        }

        // Parse words every 30 bits
        if self.subframe_idx > 0 && self.subframe_idx % WORD_BITS == 0 {
            let ws = self.subframe_idx - WORD_BITS;
            let raw = self.parse_word(ws);
            self.words.push(raw);
            self.prev_d29 = ((raw.raw >> 1) & 1) as u8;
            self.prev_d30 = (raw.raw & 1) as u8;

            // Parse HOW if this is word 2
            if self.words.len() == 2 {
                if let Some(how) = Self::parse_how(&self.words[1]) {
                    self.tow = how.tow;
                    self.subframe_id = how.subframe_id;
                }
            }
        }

        // Check for next preamble (should appear every 300 symbols)
        if self.bit_counter - self.preamble_idx >= SUBFRAME_BITS as u32 {
            // Next preamble window: correlate and verify
            if self.subframe_idx >= SUBFRAME_BITS {
                let valid = self.words.len() >= 2
                    && self.words[0].parity_ok
                    && self.words[1].parity_ok;
                self.synced = false;
                self.subframe_idx = 0;
                self.preamble_count += 1;
                return valid && self.subframe_id >= 1 && self.subframe_id <= 5;
            } else {
                // Subframe not complete — resync
                self.synced = false;
                self.subframe_idx = 0;
            }
        }
        false
    }

    /// Parse a 30-bit word from subframe_bits
    fn parse_word(&self, offset: usize) -> NavWord {
        let mut raw: u32 = 0;
        for i in 0..WORD_BITS {
            raw = (raw << 1) | (self.subframe_bits[offset + i] as u32 & 1);
        }
        let parity_ok = check_parity(raw, self.prev_d29, self.prev_d30);
        NavWord { raw, parity_ok }
    }

    fn parse_how(word: &NavWord) -> Option<How> {
        let w = word.raw;
        Some(How {
            tow: (w >> 13) & 0x1FFFF,
            alert: ((w >> 12) & 1) != 0,
            anti_spoof: ((w >> 11) & 1) != 0,
            subframe_id: ((w >> 8) & 0x7) as u8,
        })
    }
}

/// GPS extended Hamming (32,26) parity check.
fn check_parity(word: u32, d29_star: u8, d30_star: u8) -> bool {
    let d = |n: u32| -> u8 { ((word >> (30 - n)) & 1) as u8 };
    let d29 = d(29);
    let d30 = d(30);
    let p25 = d29_star ^ d(1)^d(2)^d(3)^d(5)^d(6)^d(10)^d(11)^d(12)^d(13)^d(14)^d(17)^d(18)^d(20)^d(23);
    let p26 = d30_star ^ d(2)^d(3)^d(4)^d(6)^d(7)^d(11)^d(12)^d(13)^d(14)^d(15)^d(18)^d(19)^d(21)^d(24);
    let p27 = d29_star ^ d(1)^d(3)^d(4)^d(5)^d(7)^d(8)^d(12)^d(13)^d(14)^d(15)^d(16)^d(19)^d(20)^d(22);
    let p28 = d30_star ^ d(2)^d(4)^d(5)^d(6)^d(8)^d(9)^d(13)^d(14)^d(15)^d(16)^d(17)^d(20)^d(21)^d(23);
    let p29 = d30_star ^ d(1)^d(3)^d(5)^d(6)^d(7)^d(9)^d(10)^d(14)^d(15)^d(16)^d(17)^d(18)^d(21)^d(22)^d(24);
    let p30 = d29_star ^ d(3)^d(5)^d(6)^d(8)^d(9)^d(10)^d(11)^d(13)^d(15)^d(19)^d(22)^d(23)^d(24);
    p25 == d(25) && p26 == d(26) && p27 == d(27) && p28 == d(28) && p29 == d29 && p30 == d30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preamble_correlation_perfect() {
        let mut dec = NavDecoder::new();
        // Preamble: 10001011 → +1,-1,-1,-1,+1,-1,+1,+1
        let symbols = [1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0];
        for &s in &symbols {
            dec.feed_symbol(s);
        }
        // After 8 symbols, should be synced (corr should be 8)
        assert!(dec.synced);
    }

    #[test]
    fn test_preamble_correlation_inverted() {
        let mut dec = NavDecoder::new();
        // Inverted preamble: -1,+1,+1,+1,-1,+1,-1,-1
        let symbols = [-1.0, 1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0];
        for &s in &symbols {
            dec.feed_symbol(s);
        }
        assert!(dec.synced);
        assert!(dec.inverted);
    }
}
