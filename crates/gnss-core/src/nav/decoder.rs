//! GPS L1 C/A navigation message decoding.
//! Exact port of GNSS-SDR gps_l1_ca_telemetry_decoder_gs.cc

pub const PREAMBLE: u8 = 0x8B;
pub const PREAMBLE_LEN: usize = 8;
pub const WORD_BITS: usize = 30;
pub const WORDS_PER_SUBFRAME: usize = 10;
pub const SUBFRAME_BITS: usize = WORD_BITS * WORDS_PER_SUBFRAME;

const PREAMBLE_SAMPLES: [i8; PREAMBLE_LEN] = [1, -1, -1, -1, 1, -1, 1, 1];

#[derive(Debug, Clone, Copy)]
pub struct NavWord {
    pub raw: u32,       // data bits (after D30* inversion)
    pub raw_parity: u32, // raw 30-bit word (for D29*/D30* extraction)
    pub parity_ok: bool,
}

pub struct NavDecoder {
    sym_buf: [f64; 16],
    sym_idx: usize,
    subframe_bits: [u8; SUBFRAME_BITS],
    subframe_idx: usize,
    synced: bool,
    prev_word: u32,  // previous 30-bit word (for D29*/D30*)
    bit_counter: u32,
    preamble_idx: u32,
    corr_threshold: i32,
    pub inverted: bool,
    pub tow: u32,
    pub subframe_id: u8,
    pub words: Vec<NavWord>,
}

impl NavDecoder {
    pub fn new() -> Self {
        Self {
            sym_buf: [0.0; 16], sym_idx: 0,
            subframe_bits: [0u8; SUBFRAME_BITS], subframe_idx: 0,
            synced: false, prev_word: 0,
            bit_counter: 0, preamble_idx: 0,
            corr_threshold: 5,
            inverted: false,
            tow: 0, subframe_id: 0,
            words: Vec::new(),
        }
    }

    pub fn feed_symbol(&mut self, symbol: f64) -> bool {
        self.sym_buf[self.sym_idx % 16] = symbol;
        self.sym_idx += 1;
        self.bit_counter += 1;

        if self.sym_idx < PREAMBLE_LEN as usize { return false; }

        // Preamble correlation
        let start = (self.sym_idx - PREAMBLE_LEN as usize) % 16;
        let mut corr: i32 = 0;
        for i in 0..PREAMBLE_LEN {
            let s = self.sym_buf[(start + i) % 16];
            let sign = if s > 0.0 { 1i8 } else { -1i8 };
            corr += (sign * PREAMBLE_SAMPLES[i]) as i32;
        }

        if !self.synced {
            if corr.abs() >= self.corr_threshold {
                self.synced = true;
                self.inverted = corr < 0;
                self.preamble_idx = self.bit_counter - PREAMBLE_LEN as u32;
                self.subframe_idx = 0;
                self.words.clear();
                self.prev_word = 0;

                // Fill preamble bits
                let bit_start = (self.sym_idx - PREAMBLE_LEN as usize) % 16;
                for i in 0..PREAMBLE_LEN {
                    let s = self.sym_buf[(bit_start + i) % 16];
                    let bit = if self.inverted { if s < 0.0 { 1u8 } else { 0u8 } }
                              else { if s > 0.0 { 1u8 } else { 0u8 } };
                    if self.subframe_idx < SUBFRAME_BITS {
                        self.subframe_bits[self.subframe_idx] = bit;
                        self.subframe_idx += 1;
                    }
                }
            }
            return false;
        }

        // Collect bits
        let bit = if self.inverted { if symbol < 0.0 { 1u8 } else { 0u8 } }
                  else { if symbol > 0.0 { 1u8 } else { 0u8 } };
        if self.subframe_idx < SUBFRAME_BITS {
            self.subframe_bits[self.subframe_idx] = bit;
            self.subframe_idx += 1;
        }

        // Parse words every 30 bits
        if self.subframe_idx > 0 && self.subframe_idx % WORD_BITS == 0 {
            let ws = self.subframe_idx - WORD_BITS;
            let raw = self.parse_word_gnss_sdr(ws);
            self.words.push(raw);
            self.prev_word = raw.raw_parity;  // Raw bits for D29*/D30*

            if self.words.len() == 2 {
                if let Some(how) = Self::parse_how(&self.words[1]) {
                    self.tow = how.tow;
                    self.subframe_id = how.subframe_id;
                }
            }
        }

        // Check subframe complete
        if self.bit_counter - self.preamble_idx >= SUBFRAME_BITS as u32 {
            if self.subframe_idx >= SUBFRAME_BITS {
                let n_ok = self.words.iter().filter(|w| w.parity_ok).count();
                let valid = n_ok >= 2 && self.words[0].parity_ok && self.words[1].parity_ok;
                let tlm_ok = self.words.get(0).map(|w| w.parity_ok).unwrap_or(false);
                let how_ok = self.words.get(1).map(|w| w.parity_ok).unwrap_or(false);
                let tlm_d30 = self.words.get(0).map(|w| w.raw_parity & 1).unwrap_or(0);
                if tlm_ok && !how_ok {
                    eprintln!("TLM ok(D30={}) HOW fail", tlm_d30);
                }
                if tlm_ok && how_ok {
                    eprintln!("BOTH PASS!");
                    return true;
                }
                self.synced = false;
                self.subframe_idx = 0;
                return valid && self.subframe_id >= 1 && self.subframe_id <= 5;
            } else {
                self.synced = false;
                self.subframe_idx = 0;
            }
        }
        false
    }

    /// GNSS-SDR parity check
    fn parse_word_gnss_sdr(&self, offset: usize) -> NavWord {
        // Build 30-bit word matching GNSS-SDR format:
        // bit 0 = D30 (last received), bit 29 = D1 (first received)
        let mut gpsword: u32 = 0;
        for i in 0..WORD_BITS {
            // D1 is first (offset+0), D30 is last (offset+29)
            // GNSS-SDR: D1 at bit 29, D30 at bit 0
            gpsword |= (self.subframe_bits[offset + i] as u32 & 1) << (WORD_BITS - 1 - i);
        }

        // D29* = D29 of previous word = bit 1 of prev_word
        // D30* = D30 of previous word = bit 0 of prev_word
        let d29_star = (self.prev_word >> 1) & 1;
        let d30_star = self.prev_word & 1;

        // GNSS-SDR: D30* at bit 30, D29* at bit 31
        let mut ext_word = gpsword;
        if d30_star == 1 { ext_word |= 1 << 30; }
        if d29_star == 1 { ext_word |= 1 << 31; }

        // If D30* is 1, invert data bits (D1-D24 = bits 6-29)
        if ext_word & (1 << 30) != 0 {
            ext_word ^= 0x3FFFFFC0;
        }

        let parity_ok = Self::gps_parity_check(ext_word);

        let data_word = if ext_word & (1u32 << 30) != 0 {
            ext_word & 0x3FFFFFFF
        } else {
            gpsword
        };

        NavWord { raw: data_word, raw_parity: gpsword, parity_ok }
    }

    /// GPS parity check — exact port of GNSS-SDR gps_word_parityCheck
    fn gps_parity_check(gpsword: u32) -> bool {
        let d1 = gpsword & 0xFBFFBF00;
        let d2 = gpsword.rotate_left(1) & 0x07FFBF01;
        let d3 = gpsword.rotate_left(2) & 0xFC0F8100;
        let d4 = gpsword.rotate_left(3) & 0xF81FFE02;
        let d5 = gpsword.rotate_left(4) & 0xFC00000E;
        let d6 = gpsword.rotate_left(5) & 0x07F00001;
        let d7 = gpsword.rotate_left(6) & 0x00003000;
        let t = d1 ^ d2 ^ d3 ^ d4 ^ d5 ^ d6 ^ d7;
        let parity = t ^ t.rotate_left(6) ^ t.rotate_left(12) ^ t.rotate_left(18) ^ t.rotate_left(24);
        let parity = parity & 0x3F;
        parity == (gpsword & 0x3F)
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

#[derive(Debug, Clone, Copy, Default)]
pub struct How {
    pub tow: u32,
    pub subframe_id: u8,
    pub anti_spoof: bool,
    pub alert: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preamble_perfect() {
        let mut dec = NavDecoder::new();
        let symbols = [1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0];
        for &s in &symbols { dec.feed_symbol(s); }
        assert!(dec.synced);
    }

    #[test]
    fn test_preamble_inverted() {
        let mut dec = NavDecoder::new();
        let symbols = [-1.0, 1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0];
        for &s in &symbols { dec.feed_symbol(s); }
        assert!(dec.synced);
        assert!(dec.inverted);
    }
}
