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
    /// True when `week_number` came from the 10-bit GPS navigation field.
    pub week_number_is_truncated: bool,
    /// Time of ephemeris (GPS seconds of week).
    pub toe: f64,
    /// Time of clock (GPS seconds of week).
    pub toc: f64,
    // Orbital elements
    pub sqrt_a: f64,
    pub eccentricity: f64,
    pub m0: f64,
    pub omega: f64,     // argument of perigee
    pub omega0: f64,    // longitude of ascending node
    pub omega_dot: f64, // rate of right ascension
    pub i0: f64,        // inclination at reference time
    pub i_dot: f64,     // rate of inclination
    pub delta_n: f64,   // mean motion correction
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
    pub iode: u8,  // Issue of Data, Ephemeris (from SF2 == SF3)
    pub iodc: u16, // Issue of Data, Clock
    pub sv_accuracy: u8,
    pub sv_health: u8,
    /// TOW of the subframe (count), ×6 s = transmit time of subframe start.
    pub tow: u32,
}

/// Parse Klobuchar ionosphere parameters from a RINEX navigation header.
///
/// Supports RINEX 2 `ION ALPHA`/`ION BETA` records and RINEX 3
/// `GPSA`/`GPSB` records. Exponents may use either `D` or `E` notation.
pub fn parse_rinex_ionosphere_params(contents: &str) -> Option<IonoParams> {
    let mut alpha = None;
    let mut beta = None;

    for line in contents.lines() {
        let label = line.get(60..).unwrap_or(line).trim();
        let prefix = line.split_whitespace().next().unwrap_or_default();

        if label.contains("ION ALPHA") || line.contains("ION ALPHA") {
            alpha = parse_first_four_floats(line);
        } else if label.contains("ION BETA") || line.contains("ION BETA") {
            beta = parse_first_four_floats(line);
        } else if prefix == "GPSA" {
            alpha = parse_first_four_floats(line);
        } else if prefix == "GPSB" {
            beta = parse_first_four_floats(line);
        }

        if label == "END OF HEADER" {
            break;
        }
    }

    Some(IonoParams {
        alpha: alpha?,
        beta: beta?,
        valid: true,
    })
}

fn parse_first_four_floats(line: &str) -> Option<[f64; 4]> {
    let mut values = Vec::with_capacity(4);
    for token in line.split_whitespace() {
        let normalized = token.replace(['D', 'd'], "E");
        if let Ok(value) = normalized.parse::<f64>() {
            if !value.is_finite() {
                return None;
            }
            values.push(value);
            if values.len() == 4 {
                return Some([values[0], values[1], values[2], values[3]]);
            }
        }
    }
    None
}

/// Parse GPS broadcast ephemerides from a RINEX navigation file.
///
/// Supports GPS RINEX 2 `.n` records and GPS records in RINEX 3 navigation
/// files. The result preserves multiple records per PRN for epoch-time based
/// selection.
pub fn parse_rinex_ephemeris_catalog(contents: &str) -> EphemerisCatalog {
    let mut lines = contents.lines();
    let version = lines
        .next()
        .and_then(|line| line.get(0..9))
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(2.0);
    for line in lines.by_ref() {
        let label = line.get(60..).unwrap_or(line).trim();
        if label == "END OF HEADER" || line.contains("END OF HEADER") {
            break;
        }
    }

    let mut catalog = EphemerisCatalog::new();
    while let Some(first_line) = lines.next() {
        if first_line.trim().is_empty() {
            continue;
        }
        let continuation: Vec<&str> = lines.by_ref().take(7).collect();
        if continuation.len() != 7 {
            break;
        }
        if let Some(eph) = parse_rinex_ephemeris_record(first_line, &continuation, version) {
            catalog.entry(eph.prn).or_default().push(eph);
        }
    }

    catalog
}

fn parse_rinex_ephemeris_record(
    first_line: &str,
    continuation: &[&str],
    version: f64,
) -> Option<Ephemeris> {
    let first = parse_rinex_first_line(first_line, version)?;
    let row1 = parse_rinex_float_fields(continuation[0], 3, 4)?;
    let row2 = parse_rinex_float_fields(continuation[1], 3, 4)?;
    let row3 = parse_rinex_float_fields(continuation[2], 3, 4)?;
    let row4 = parse_rinex_float_fields(continuation[3], 3, 4)?;
    let row5 = parse_rinex_float_fields(continuation[4], 3, 4)?;
    let row6 = parse_rinex_float_fields(continuation[5], 3, 4)?;
    let row7 = parse_rinex_float_fields(continuation[6], 3, 4)?;

    Some(Ephemeris {
        prn: first.prn,
        week_number: row5[2].round().clamp(0.0, u16::MAX as f64) as u16,
        week_number_is_truncated: false,
        toe: row3[0],
        toc: first.toc_sow,
        sqrt_a: row2[3],
        eccentricity: row2[1],
        m0: row1[3],
        omega: row4[2],
        omega0: row3[2],
        omega_dot: row4[3],
        i0: row4[0],
        i_dot: row5[0],
        delta_n: row1[2],
        cus: row2[2],
        cuc: row2[0],
        crs: row1[1],
        crc: row4[1],
        cis: row3[3],
        cic: row3[1],
        af0: first.af0,
        af1: first.af1,
        af2: first.af2,
        tgd: row6[2],
        iode: row1[0].round().clamp(0.0, u8::MAX as f64) as u8,
        iodc: row6[3].round().clamp(0.0, u16::MAX as f64) as u16,
        sv_accuracy: row6[0].round().clamp(0.0, u8::MAX as f64) as u8,
        sv_health: row6[1].round().clamp(0.0, u8::MAX as f64) as u8,
        tow: (row7[0] / 6.0).round().clamp(0.0, u32::MAX as f64) as u32,
    })
}

#[derive(Debug, Clone, Copy)]
struct RinexFirstLine {
    prn: u8,
    toc_sow: f64,
    af0: f64,
    af1: f64,
    af2: f64,
}

fn parse_rinex_first_line(line: &str, version: f64) -> Option<RinexFirstLine> {
    if version >= 3.0 {
        if line.get(0..1)? != "G" {
            return None;
        }
        let prn = parse_u8_field(line, 1, 3)?;
        let year = parse_i32_field(line, 4, 8)?;
        let month = parse_u32_field(line, 9, 11)?;
        let day = parse_u32_field(line, 12, 14)?;
        let hour = parse_u32_field(line, 15, 17)?;
        let minute = parse_u32_field(line, 18, 20)?;
        let second = parse_f64_field(line, 21, 23)?;
        let clock = parse_rinex_float_fields(line, 23, 3)?;
        Some(RinexFirstLine {
            prn,
            toc_sow: gps_sow_from_ymdhms(year, month, day, hour, minute, second)?,
            af0: clock[0],
            af1: clock[1],
            af2: clock[2],
        })
    } else {
        let prn = parse_u8_field(line, 0, 2)?;
        let year = expand_rinex2_year(parse_i32_field(line, 3, 5)?);
        let month = parse_u32_field(line, 6, 8)?;
        let day = parse_u32_field(line, 9, 11)?;
        let hour = parse_u32_field(line, 12, 14)?;
        let minute = parse_u32_field(line, 15, 17)?;
        let second = parse_f64_field(line, 18, 22)?;
        let clock = parse_rinex_float_fields(line, 22, 3)?;
        Some(RinexFirstLine {
            prn,
            toc_sow: gps_sow_from_ymdhms(year, month, day, hour, minute, second)?,
            af0: clock[0],
            af1: clock[1],
            af2: clock[2],
        })
    }
}

fn parse_rinex_float_fields(line: &str, start: usize, count: usize) -> Option<Vec<f64>> {
    (0..count)
        .map(|idx| {
            let begin = start + idx * 19;
            parse_f64_field(line, begin, begin + 19)
        })
        .collect()
}

fn parse_f64_field(line: &str, start: usize, end: usize) -> Option<f64> {
    line.get(start..end.min(line.len()))?
        .trim()
        .replace(['D', 'd'], "E")
        .parse::<f64>()
        .ok()
}

fn parse_i32_field(line: &str, start: usize, end: usize) -> Option<i32> {
    line.get(start..end)?.trim().parse::<i32>().ok()
}

fn parse_u32_field(line: &str, start: usize, end: usize) -> Option<u32> {
    line.get(start..end)?.trim().parse::<u32>().ok()
}

fn parse_u8_field(line: &str, start: usize, end: usize) -> Option<u8> {
    line.get(start..end)?.trim().parse::<u8>().ok()
}

fn expand_rinex2_year(year: i32) -> i32 {
    if year >= 80 {
        1900 + year
    } else {
        2000 + year
    }
}

fn gps_sow_from_ymdhms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: f64,
) -> Option<f64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let gps_epoch_days = days_from_civil(1980, 1, 6);
    let gps_days = days - gps_epoch_days;
    if gps_days < 0 {
        return None;
    }
    let week_day = gps_days.rem_euclid(7) as f64;
    Some(week_day * 86_400.0 + hour as f64 * 3600.0 + minute as f64 * 60.0 + second)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
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
    if sf != 1 {
        return None;
    }

    Some(Ephemeris {
        prn,
        week_number: r.read_u(61, 10) as u16,
        week_number_is_truncated: true,
        sv_accuracy: r.read_u(73, 4) as u8,
        sv_health: r.read_u(77, 6) as u8,
        tgd: r.read_i(197, 8) as f64 * P2_31,
        toc: r.read_u(219, 16) as f64 * 16.0,
        af2: r.read_i(241, 8) as f64 * P2_55,
        af1: r.read_i(249, 16) as f64 * P2_43,
        af0: r.read_i(271, 22) as f64 * P2_31,
        // IODC = {83,2} MSB + {211,8} LSB (two-segment field).
        iodc: r.read_segments_u(&[(83, 2), (211, 8)]) as u16,
        tow: r.read_u(31, 17) as u32,
        ..Default::default()
    })
}

/// Merge subframe 2 (orbit part 1) into an existing Ephemeris.
pub fn parse_subframe2(e: &mut Ephemeris, words: &[NavWord]) -> bool {
    let r = BitReader::new(words);
    if r.read_u(50, 3) != 2 {
        return false;
    }
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
    if r.read_u(50, 3) != 3 {
        return false;
    }
    let iode_sf3 = r.read_u(271, 8) as u8;
    if iode_sf3 != e.iode {
        return false;
    }
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
    if (e.iodc & 0xFF) as u8 != e.iode {
        return false;
    }
    true
}

/// Parse Klobuchar ionosphere params from subframe 4 page 18.
pub fn parse_iono(words: &[NavWord]) -> Option<IonoParams> {
    let r = BitReader::new(words);
    if r.read_u(50, 3) != 4 {
        return None;
    }
    // Subframes 4/5 carry a page ID in word 3. Ionospheric alpha/beta
    // coefficients are only on subframe 4, page 18 (SV ID 56), data ID 1.
    if r.read_u(61, 2) != 1 || r.read_u(63, 6) != 56 {
        return None;
    }
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

/// Ephemeris collection that can hold multiple broadcast records per PRN.
pub type EphemerisCatalog = BTreeMap<u8, Vec<Ephemeris>>;

#[derive(Debug, Clone)]
pub struct EphemerisSelectionConfig {
    pub exclude_unhealthy_sv: bool,
    pub max_toe_age_s: Option<f64>,
    pub max_toc_age_s: Option<f64>,
}

impl Default for EphemerisSelectionConfig {
    fn default() -> Self {
        Self {
            exclude_unhealthy_sv: true,
            max_toe_age_s: Some(4.0 * 3600.0),
            max_toc_age_s: Some(4.0 * 3600.0),
        }
    }
}

pub fn select_ephemeris<'a>(
    catalog: &'a EphemerisCatalog,
    prn: u8,
    transmit_time_sow: f64,
    config: &EphemerisSelectionConfig,
) -> Option<&'a Ephemeris> {
    select_ephemeris_from_candidates(catalog.get(&prn)?, transmit_time_sow, config)
}

pub fn select_ephemeris_from_candidates<'a>(
    candidates: &'a [Ephemeris],
    transmit_time_sow: f64,
    config: &EphemerisSelectionConfig,
) -> Option<&'a Ephemeris> {
    candidates
        .iter()
        .filter(|eph| ephemeris_candidate_is_usable(eph, transmit_time_sow, config))
        .min_by(|a, b| {
            let a_toe_age = gps_time_abs_diff(transmit_time_sow, a.toe);
            let b_toe_age = gps_time_abs_diff(transmit_time_sow, b.toe);
            let a_toc_age = gps_time_abs_diff(transmit_time_sow, a.toc);
            let b_toc_age = gps_time_abs_diff(transmit_time_sow, b.toc);
            a_toe_age
                .total_cmp(&b_toe_age)
                .then_with(|| a_toc_age.total_cmp(&b_toc_age))
                .then_with(|| b.iode.cmp(&a.iode))
        })
}

pub fn ephemeris_map_from_catalog_at_time(
    catalog: &EphemerisCatalog,
    transmit_times_by_prn: &[(u8, f64)],
    config: &EphemerisSelectionConfig,
) -> EphemerisMap {
    transmit_times_by_prn
        .iter()
        .filter_map(|(prn, transmit_time_sow)| {
            select_ephemeris(catalog, *prn, *transmit_time_sow, config)
                .cloned()
                .map(|eph| (*prn, eph))
        })
        .collect()
}

pub fn ephemeris_catalog_from_map(map: &EphemerisMap) -> EphemerisCatalog {
    map.iter()
        .map(|(prn, eph)| (*prn, vec![eph.clone()]))
        .collect()
}

fn ephemeris_candidate_is_usable(
    eph: &Ephemeris,
    transmit_time_sow: f64,
    config: &EphemerisSelectionConfig,
) -> bool {
    if !eph.sqrt_a.is_finite()
        || eph.sqrt_a <= 0.0
        || !eph.eccentricity.is_finite()
        || !eph.toe.is_finite()
        || !eph.toc.is_finite()
    {
        return false;
    }
    if config.exclude_unhealthy_sv && eph.sv_health != 0 {
        return false;
    }
    if config
        .max_toe_age_s
        .is_some_and(|max_age| gps_time_abs_diff(transmit_time_sow, eph.toe) > max_age)
    {
        return false;
    }
    if config
        .max_toc_age_s
        .is_some_and(|max_age| gps_time_abs_diff(transmit_time_sow, eph.toc) > max_age)
    {
        return false;
    }
    true
}

fn gps_time_abs_diff(t: f64, reference: f64) -> f64 {
    let mut dt = t - reference;
    if dt > 302_400.0 {
        dt -= 604_800.0;
    } else if dt < -302_400.0 {
        dt += 604_800.0;
    }
    dt.abs()
}

/// A receiver that accumulates subframes per PRN and produces ephemerides once
/// subframes 1, 2, 3 are all collected and consistent.
pub struct EphemerisCollector {
    /// prn → (subframe_id → words). We keep the latest words per subframe id.
    per_prn: BTreeMap<u8, BTreeMap<u8, [NavWord; 10]>>,
}

impl EphemerisCollector {
    pub fn new() -> Self {
        Self {
            per_prn: BTreeMap::new(),
        }
    }

    /// Feed a decoded subframe. Returns a fully validated Ephemeris when
    /// subframes 1+2+3 are all present and consistent for this PRN.
    pub fn feed(&mut self, prn: u8, words: &[NavWord]) -> Option<Ephemeris> {
        if words.len() != 10 {
            return None;
        }
        let r = BitReader::new(words);
        let sf = r.read_u(50, 3) as u8;
        if !(1..=3).contains(&sf) {
            return None;
        }
        let arr: [NavWord; 10] = [
            words[0], words[1], words[2], words[3], words[4], words[5], words[6], words[7],
            words[8], words[9],
        ];
        let entry = self.per_prn.entry(prn).or_default();
        entry.insert(sf, arr);
        // Need all of 1, 2, 3.
        if !(entry.contains_key(&1) && entry.contains_key(&2) && entry.contains_key(&3)) {
            return None;
        }
        let mut e = parse_subframe1(&entry[&1], prn)?;
        if !parse_subframe2(&mut e, &entry[&2]) {
            return None;
        }
        if !parse_subframe3(&mut e, &entry[&3]) {
            return None;
        }
        Some(e)
    }
}

impl Default for EphemerisCollector {
    fn default() -> Self {
        Self::new()
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
        let mut words = [NavWord {
            raw: 0,
            raw_parity: 0,
            parity_ok: true,
        }; 10];
        // word 1, set bits 19-21 (within the 30-bit word, MSB-first) to 0,1,0.
        // raw bit (29 - 19) = bit 10 → 0; (29-20)=bit 9 → 1; (29-21)=bit 8 → 0.
        words[1].raw = 1 << 9;
        let r = BitReader::new(&words);
        assert_eq!(r.read_u(50, 3), 2);
    }

    fn set_u(words: &mut [NavWord; 10], first: usize, len: usize, value: u64) {
        for i in 0..len {
            let bit = first - 1 + i;
            let word = bit / 30;
            let bit_in_word = bit % 30;
            let raw_bit = 29 - bit_in_word;
            let v = (value >> (len - 1 - i)) & 1;
            if v != 0 {
                words[word].raw |= 1 << raw_bit;
            } else {
                words[word].raw &= !(1 << raw_bit);
            }
        }
    }

    #[test]
    fn test_parse_iono_requires_subframe4_page18() {
        let mut words = [NavWord {
            raw: 0,
            raw_parity: 0,
            parity_ok: true,
        }; 10];
        set_u(&mut words, 50, 3, 4);
        set_u(&mut words, 61, 2, 1);
        set_u(&mut words, 63, 6, 56);
        set_u(&mut words, 69, 8, 1);
        set_u(&mut words, 77, 8, 2);
        set_u(&mut words, 91, 8, 3);
        set_u(&mut words, 99, 8, 4);
        set_u(&mut words, 107, 8, 5);
        set_u(&mut words, 121, 8, 6);
        set_u(&mut words, 129, 8, 7);
        set_u(&mut words, 137, 8, 8);

        let iono = parse_iono(&words).expect("subframe 4 page 18 should parse");
        assert!(iono.valid);
        assert_eq!(iono.alpha[0], P2_30);
        assert_eq!(iono.alpha[1], 2.0 * P2_27);
        assert_eq!(iono.beta[0], 5.0 * (1u64 << 11) as f64);
        assert_eq!(iono.beta[3], 8.0 * (1u64 << 16) as f64);

        set_u(&mut words, 63, 6, 55);
        assert!(
            parse_iono(&words).is_none(),
            "non-page-18 subframe 4 must not parse as iono"
        );
    }

    #[test]
    fn test_parse_rinex2_ionosphere_header() {
        let rinex = "\
     2.10           N: GPS NAV DATA                         RINEX VERSION / TYPE
 0.1212D-07  0.7451D-08 -0.5960D-07 -0.1192D-06          ION ALPHA
 0.9626D+05  0.0000D+00 -0.1966D+06 -0.6554D+05          ION BETA
                                                            END OF HEADER
";

        let iono = parse_rinex_ionosphere_params(rinex).expect("RINEX 2 iono should parse");

        assert!(iono.valid);
        assert!((iono.alpha[0] - 0.1212e-7).abs() < 1e-18);
        assert!((iono.alpha[3] + 0.1192e-6).abs() < 1e-18);
        assert!((iono.beta[0] - 0.9626e5).abs() < 1e-9);
        assert!((iono.beta[2] + 0.1966e6).abs() < 1e-9);
    }

    #[test]
    fn test_parse_rinex3_gpsa_gpsb_header() {
        let rinex = "\
     3.04           N: GNSS NAV DATA    M                   RINEX VERSION / TYPE
GPSA   1.1176E-08  7.4506E-09 -5.9605E-08 -1.1921E-07    IONOSPHERIC CORR
GPSB   9.6256E+04  0.0000E+00 -1.9661E+05 -6.5536E+04    IONOSPHERIC CORR
                                                            END OF HEADER
";

        let iono = parse_rinex_ionosphere_params(rinex).expect("RINEX 3 iono should parse");

        assert!(iono.valid);
        assert!((iono.alpha[0] - 1.1176e-8).abs() < 1e-18);
        assert!((iono.beta[3] + 6.5536e4).abs() < 1e-9);
    }

    #[test]
    fn test_parse_rinex_ionosphere_requires_alpha_and_beta() {
        let missing_beta = "\
GPSA   1.1176E-08  7.4506E-09 -5.9605E-08 -1.1921E-07    IONOSPHERIC CORR
                                                            END OF HEADER
";
        assert!(parse_rinex_ionosphere_params(missing_beta).is_none());
    }

    #[test]
    fn test_parse_rinex2_ephemeris_catalog_preserves_broadcast_fields() {
        let catalog = parse_rinex_ephemeris_catalog(rinex2_single_ephemeris_fixture());
        let prn1 = catalog.get(&1).expect("PRN 1 ephemeris should parse");
        assert_eq!(prn1.len(), 1);

        let eph = &prn1[0];
        assert_eq!(eph.prn, 1);
        assert_eq!(eph.week_number, 1734);
        assert!((eph.toe - 345_600.0).abs() < 1e-9);
        assert!((eph.toc - 345_600.0).abs() < 1e-9);
        assert_eq!(eph.iode, 66);
        assert_eq!(eph.iodc, 66);
        assert_eq!(eph.sv_accuracy, 2);
        assert_eq!(eph.sv_health, 0);
        assert_eq!(eph.tow, 56_403);
        assert!((eph.af0 - 0.130245462060e-4).abs() < 1e-18);
        assert!((eph.af1 - 0.329691829393e-11).abs() < 1e-22);
        assert!((eph.sqrt_a - 5153.70147133).abs() < 1e-8);
        assert!((eph.eccentricity - 0.170189398341e-2).abs() < 1e-15);
        assert!((eph.omega0 + 0.462984461481).abs() < 1e-12);
        assert!((eph.tgd - 0.838190317154e-8).abs() < 1e-18);
    }

    #[test]
    fn test_parse_rinex2_ephemeris_catalog_handles_glued_negative_fields() {
        let catalog = parse_rinex_ephemeris_catalog(rinex2_single_ephemeris_fixture());
        let eph = &catalog[&1][0];

        assert!((eph.cic + 0.931322574616e-8).abs() < 1e-20);
        assert!((eph.cis + 0.931322574615e-7).abs() < 1e-19);
        assert!((eph.omega_dot + 0.795818863336e-8).abs() < 1e-20);
    }

    #[test]
    fn test_gps_sow_from_ymdhms_for_cttc_nav_epoch() {
        assert_eq!(
            gps_sow_from_ymdhms(2013, 4, 4, 0, 0, 0.0).expect("valid GPS epoch"),
            345_600.0
        );
    }

    #[test]
    fn test_select_ephemeris_prefers_closest_toe() {
        let candidates = vec![
            ephemeris_for_selection(1, 9_000.0, 9_000.0, 1, 0),
            ephemeris_for_selection(1, 10_900.0, 10_800.0, 2, 0),
            ephemeris_for_selection(1, 7_000.0, 7_000.0, 3, 0),
        ];

        let selected = select_ephemeris_from_candidates(&candidates, 11_000.0, &Default::default())
            .expect("nearest TOE ephemeris should be selected");

        assert_eq!(selected.iode, 2);
    }

    #[test]
    fn test_select_ephemeris_filters_unhealthy_and_stale_records() {
        let candidates = vec![
            ephemeris_for_selection(1, 10_900.0, 10_900.0, 1, 1),
            ephemeris_for_selection(1, 1_000.0, 1_000.0, 2, 0),
            ephemeris_for_selection(1, 10_500.0, 10_500.0, 3, 0),
        ];
        let config = EphemerisSelectionConfig {
            max_toe_age_s: Some(1_000.0),
            max_toc_age_s: Some(1_000.0),
            ..Default::default()
        };

        let selected = select_ephemeris_from_candidates(&candidates, 11_000.0, &config)
            .expect("healthy non-stale ephemeris should be selected");

        assert_eq!(selected.iode, 3);
    }

    #[test]
    fn test_ephemeris_map_from_catalog_selects_per_prn_time() {
        let mut catalog = EphemerisCatalog::new();
        catalog.insert(
            1,
            vec![
                ephemeris_for_selection(1, 1_000.0, 1_000.0, 1, 0),
                ephemeris_for_selection(1, 2_000.0, 2_000.0, 2, 0),
            ],
        );
        catalog.insert(2, vec![ephemeris_for_selection(2, 5_000.0, 5_000.0, 7, 0)]);

        let selected = ephemeris_map_from_catalog_at_time(
            &catalog,
            &[(1, 2_100.0), (2, 5_100.0), (3, 5_100.0)],
            &Default::default(),
        );

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[&1].iode, 2);
        assert_eq!(selected[&2].iode, 7);
    }

    fn ephemeris_for_selection(prn: u8, toe: f64, toc: f64, iode: u8, sv_health: u8) -> Ephemeris {
        Ephemeris {
            prn,
            toe,
            toc,
            iode,
            sv_health,
            sqrt_a: 26_559_710.0_f64.sqrt(),
            eccentricity: 0.01,
            ..Default::default()
        }
    }

    fn rinex2_single_ephemeris_fixture() -> &'static str {
        "\
     2              NAVIGATION DATA                         RINEX VERSION / TYPE
                                                            END OF HEADER
 1 13  4  4  0  0  0.0 0.130245462060D-04 0.329691829393D-11 0.000000000000D+00
    0.660000000000D+02 0.110312500000D+02 0.445589989183D-08-0.129411133012D+01
    0.603497028351D-06 0.170189398341D-02 0.977143645287D-05 0.515370147133D+04
    0.345600000000D+06-0.931322574616D-08-0.462984461481D+00-0.931322574615D-07
    0.960438103656D+00 0.193500000000D+03 0.315473100675D+00-0.795818863336D-08
    0.174292974288D-09 0.100000000000D+01 0.173400000000D+04 0.000000000000D+00
    0.200000000000D+01 0.000000000000D+00 0.838190317154D-08 0.660000000000D+02
    0.338418000000D+06 0.400000000000D+01 0.000000000000D+00 0.000000000000D+00
"
    }
}
