//! GPS L1 C/A ephemeris and ionosphere parsing from decoded subframes.
//!
//! Bit fields per IS-GPS-200M, ported from GNSS-SDR `gps_navigation_message.cc`.
//! Words are stored as in `NavWord::raw`: D1 at bit 29, D30 at bit 0 (MSB-first
//! within the 30-bit word, so bit index 0 of the subframe = D1 = word[0] bit 29).

use crate::nav::decoder::NavWord;
use std::collections::BTreeMap;

pub const SUBFRAME_BITS: usize = 300;

/// GPS ephemeris parameters (one satellite). All angles in radians, distances
/// in meters, times in seconds, unless noted.
#[derive(Debug, Clone, Default)]
pub struct Ephemeris {
    pub prn: u8,
    pub week_number: u16,
    /// Time of ephemeris (GPS seconds of week).
    pub toe: f64,
    /// Time of clock (GPS seconds of week).
    pub toc: f64,
    // Orbital elements
    pub sqrt_a: f64,
    pub eccentricity: f64,
    pub m0: f64,
    pub omega: f64,       // argument of perigee
    pub omega0: f64,      // longitude of ascending node
    pub omega_dot: f64,   // rate of right ascension
    pub i0: f64,          // inclination at reference time
    pub i_dot: f64,       // rate of inclination
    pub delta_n: f64,     // mean motion correction
    // Harmonic corrections
    pub cus: f64,
    pub cuc: f64,
    pub crs: f64,
    pub crc: f64,
    pub cis: f64,
    pub cic: f64,
    // Clock corrections
    pub af0: f64,
    pub af1: f64,
    pub af2: f64,
    pub tgd: f64,
    // Issue of Data
    pub iode: u8,         // Issue of Data, Ephemeris (from SF2 == SF3)
    pub iodc: u16,        // Issue of Data, Clock
    pub sv_accuracy: u8,
    pub sv_health: u8,
    /// TOW of the subframe (count), ×6 s = transmit time of subframe start.
    pub tow: u32,
}

/// Klobuchar ionosphere parameters (8 coefficients from subframe 4, page 18).
#[derive(Debug, Clone, Default)]
pub struct IonoParams {
    pub alpha: [f64; 4],
    pub beta: [f64; 4],
    pub valid: bool,
}

/// Bit reader over the 300-bit subframe. Bit 0 = D1 (MSB of word 0).
struct BitReader {
    bits: [u8; SUBFRAME_BITS],
}

impl BitReader {
    fn new(words: &[NavWord]) -> Self {
        // Flatten the 10 words' raw (D30*-corrected) bits into a 300-bit array.
        // word[i].raw bit 29 = first bit of word (D1 of that word).
        let mut bits = [0u8; SUBFRAME_BITS];
        for (w, word) in words.iter().enumerate() {
            for b in 0..30 {
                // bit (29-b) of raw = D(b+1) of the word
                bits[w * 30 + b] = ((word.raw >> (29 - b)) & 1) as u8;
            }
        }
        BitReader { bits }
    }

    /// Read an unsigned value spanning multiple bit ranges. Each tuple is
    /// (first_bit_1based, length). Bits are read D-first, MSB first, and
    /// concatenated in order. D(N) is at index N-1 of our 0-based `bits`.
    fn read_segments_u(&self, segs: &[(usize, usize)]) -> u64 {
        let mut v: u64 = 0;
        for &(first, len) in segs {
            let start = first - 1; // convert D(1-based) to 0-based index
            for i in 0..len {
                v = (v << 1) | self.bits[start + i] as u64;
            }
        }
        v
    }

    /// Read a signed (two's complement) value spanning multiple bit ranges.
    fn read_segments_i(&self, segs: &[(usize, usize)]) -> i64 {
        let u = self.read_segments_u(segs);
        let total_bits: usize = segs.iter().map(|&(_, l)| l).sum();
        let sign_bit = 1u64 << (total_bits - 1);
        if u & sign_bit != 0 {
            (u as i64) - (1i64 << total_bits)
        } else {
            u as i64
        }
    }

    /// Convenience: single-segment unsigned read, D(first) 1-based.
    fn read_u(&self, first: usize, len: usize) -> u64 {
        self.read_segments_u(&[(first, len)])
    }

    /// Convenience: single-segment signed read, D(first) 1-based.
    fn read_i(&self, first: usize, len: usize) -> i64 {
        self.read_segments_i(&[(first, len)])
    }

    #[allow(dead_code)]
    fn read_bool(&self, start: usize) -> bool {
        self.bits[start] != 0
    }
}

/// Scale factor constants (powers of two / π·2^n) per IS-GPS-200.
const P2_5: f64 = 1.0 / 32.0;
const P2_19: f64 = 1.0 / (1u64 << 19) as f64;
const P2_24: f64 = 1.0 / (1u64 << 24) as f64;
const P2_27: f64 = 1.0 / (1u64 << 27) as f64;
const P2_29: f64 = 1.0 / (1u64 << 29) as f64;
const P2_30: f64 = 1.0 / (1u64 << 30) as f64;
const P2_31: f64 = 1.0 / (1u64 << 31) as f64;
const P2_33: f64 = 1.0 / (1u64 << 33) as f64;
const P2_43: f64 = 1.0 / (1u64 << 43) as f64;
const P2_55: f64 = 1.0 / (1u64 << 55) as f64;
const PI_P2_31: f64 = std::f64::consts::PI * P2_31;
const PI_P2_43: f64 = std::f64::consts::PI * P2_43;

/// Parse subframe 1 (clock + SV ID). Returns None if subframe_id != 1.
pub fn parse_subframe1(words: &[NavWord], prn: u8) -> Option<Ephemeris> {
    let r = BitReader::new(words);
    let sf = r.read_u(50, 3) as u8;
    if sf != 1 { return None; }

    let mut e = Ephemeris::default();
    e.prn = prn;
    e.week_number = r.read_u(61, 10) as u16;
    e.sv_accuracy = r.read_u(73, 4) as u8;
    e.sv_health = r.read_u(77, 6) as u8;
    e.tgd = r.read_i(197, 8) as f64 * P2_31;
    e.toc = r.read_u(219, 16) as f64 * 16.0;
    e.af2 = r.read_i(241, 8) as f64 * P2_55;
    e.af1 = r.read_i(249, 16) as f64 * P2_43;
    e.af0 = r.read_i(271, 22) as f64 * P2_31;
    // IODC = {83,2} MSB + {211,8} LSB (two-segment field).
    e.iodc = ((r.read_segments_u(&[(83, 2), (211, 8)])) as u16);
    e.tow = r.read_u(31, 17) as u32;
    Some(e)
}

/// Merge subframe 2 (orbit part 1) into an existing Ephemeris.
pub fn parse_subframe2(e: &mut Ephemeris, words: &[NavWord]) -> bool {
    let r = BitReader::new(words);
    if r.read_u(50, 3) != 2 { return false; }
    let iode_sf2 = r.read_u(61, 8) as u8;
    e.iode = iode_sf2;
    e.crs = r.read_i(69, 16) as f64 * P2_5;
    e.delta_n = r.read_i(91, 16) as f64 * PI_P2_43;
    e.m0 = r.read_segments_i(&[(107, 8), (121, 24)]) as f64 * PI_P2_31;
    e.cuc = r.read_i(151, 16) as f64 * P2_29;
    e.eccentricity = r.read_segments_u(&[(167, 8), (181, 24)]) as f64 * P2_33;
    e.cus = r.read_i(211, 16) as f64 * P2_29;
    e.sqrt_a = r.read_segments_u(&[(227, 8), (241, 24)]) as f64 * P2_19;
    e.toe = r.read_u(271, 16) as f64 * 16.0;
    e.tow = r.read_u(31, 17) as u32;
    true
}

/// Merge subframe 3 (orbit part 2) into an existing Ephemeris. Returns false
/// if IODE mismatch (SF2 vs SF3) — ephemeris is inconsistent.
pub fn parse_subframe3(e: &mut Ephemeris, words: &[NavWord]) -> bool {
    let r = BitReader::new(words);
    if r.read_u(50, 3) != 3 { return false; }
    let iode_sf3 = r.read_u(271, 8) as u8;
    if iode_sf3 != e.iode { return false; }
    e.cic = r.read_i(61, 16) as f64 * P2_29;
    e.omega0 = r.read_segments_i(&[(77, 8), (91, 24)]) as f64 * PI_P2_31;
    e.cis = r.read_i(121, 16) as f64 * P2_29;
    e.i0 = r.read_segments_i(&[(137, 8), (151, 24)]) as f64 * PI_P2_31;
    e.crc = r.read_i(181, 16) as f64 * P2_5;
    e.omega = r.read_segments_i(&[(197, 8), (211, 24)]) as f64 * PI_P2_31;
    e.omega_dot = r.read_i(241, 24) as f64 * PI_P2_43;
    e.i_dot = r.read_i(279, 14) as f64 * PI_P2_43;
    e.tow = r.read_u(31, 17) as u32;
    // Validate IODC low 8 bits == IODE
    if (e.iodc & 0xFF) as u8 != e.iode { return false; }
    true
}

/// Parse Klobuchar ionosphere params from subframe 4 page 18.
pub fn parse_iono(words: &[NavWord]) -> Option<IonoParams> {
    let r = BitReader::new(words);
    if r.read_u(50, 3) != 4 { return None; }
    let alpha0 = r.read_i(69, 8) as f64 * P2_30;
    let alpha1 = r.read_i(77, 8) as f64 * P2_27;
    let alpha2 = r.read_i(91, 8) as f64 * P2_24;
    let alpha3 = r.read_i(99, 8) as f64 * P2_24;
    let beta0 = r.read_i(107, 8) as f64 * (1u64 << 11) as f64;
    let beta1 = r.read_i(121, 8) as f64 * (1u64 << 14) as f64;
    let beta2 = r.read_i(129, 8) as f64 * (1u64 << 16) as f64;
    let beta3 = r.read_i(137, 8) as f64 * (1u64 << 16) as f64;
    Some(IonoParams {
        alpha: [alpha0, alpha1, alpha2, alpha3],
        beta: [beta0, beta1, beta2, beta3],
        valid: true,
    })
}

/// Ephemeris collection for all tracked satellites: PRN → Ephemeris.
pub type EphemerisMap = BTreeMap<u8, Ephemeris>;

/// A receiver that accumulates subframes per PRN and produces ephemerides once
/// subframes 1, 2, 3 are all collected and consistent.
pub struct EphemerisCollector {
    /// prn → (subframe_id → words). We keep the latest words per subframe id.
    per_prn: BTreeMap<u8, BTreeMap<u8, [NavWord; 10]>>,
}

impl EphemerisCollector {
    pub fn new() -> Self {
        Self { per_prn: BTreeMap::new() }
    }

    /// Feed a decoded subframe. Returns a fully validated Ephemeris when
    /// subframes 1+2+3 are all present and consistent for this PRN.
    pub fn feed(&mut self, prn: u8, words: &[NavWord]) -> Option<Ephemeris> {
        if words.len() != 10 { return None; }
        let r = BitReader::new(words);
        let sf = r.read_u(50, 3) as u8;
        if !(1..=3).contains(&sf) {
            eprintln!("  [eph] PRN {} sf={} (not 1-3, skipping)", prn, sf);
            return None;
        }
        let arr: [NavWord; 10] = [
            words[0], words[1], words[2], words[3], words[4],
            words[5], words[6], words[7], words[8], words[9],
        ];
        let entry = self.per_prn.entry(prn).or_default();
        entry.insert(sf, arr);
        eprintln!("  [eph] PRN {} got sf={}, have {:?}", prn, sf, entry.keys().collect::<Vec<_>>());
        // Need all of 1, 2, 3.
        if !(entry.contains_key(&1) && entry.contains_key(&2) && entry.contains_key(&3)) {
            return None;
        }
        let mut e = match parse_subframe1(&entry[&1], prn) {
            Some(e) => e,
            None => { eprintln!("  [eph] PRN {} parse_subframe1 failed", prn); return None; }
        };
        if !parse_subframe2(&mut e, &entry[&2]) {
            eprintln!("  [eph] PRN {} parse_subframe2 failed", prn); return None;
        }
        if !parse_subframe3(&mut e, &entry[&3]) {
            eprintln!("  [eph] PRN {} parse_subframe3 failed (IODE sf2={} sf3={} iodc={})",
                prn, e.iode, e.iode, e.iodc); return None;
        }
        Some(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: BitReader reads the subframe id and TOW correctly from a
    /// synthetic subframe where we set known bits.
    #[test]
    fn test_bit_reader_subframe_id() {
        // Build a 10-word subframe with subframe_id = 2 in bits 49-51.
        // HOW is word 1 (index 1). Subframe id is in HOW bits 43-45 (D43-D45 of
        // the subframe = bits 49-51 overall since word 0 = bits 0-29).
        // HOW bit layout: D1 of HOW = subframe bit 30. Subframe id D43-D45
        // relative to subframe start = bits 49-51. Within HOW (word 1), that's
        // bits 19-21 (49-30). raw bit for D(30-(19)) = bit 11, etc.
        // We set subframe_id=2 → bits 49,50,51 = 0,1,0.
        let mut words = [NavWord { raw: 0, raw_parity: 0, parity_ok: true }; 10];
        // word 1, set bits 19-21 (within the 30-bit word, MSB-first) to 0,1,0.
        // raw bit (29 - 19) = bit 10 → 0; (29-20)=bit 9 → 1; (29-21)=bit 8 → 0.
        words[1].raw = 1 << 9;
        let r = BitReader::new(&words);
        assert_eq!(r.read_u(50, 3), 2);
    }
}
