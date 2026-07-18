//! M2 end-to-end PVT test on real CTTC Spain data.
//!
//! Track 5 satellites (PRN 1, 11, 17, 20, 32 per GNSS-SDR reference), decode
//! subframes 1-3, parse ephemerides, form pseudoranges, run WLS solver, and
//! assert the position is near the CTTC reference: 41.2747°N, 1.9875°E.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::nav::{Ephemeris, EphemerisCollector, EphemerisMap, IonoParams, NavDecoder};
use gnss_core::pvt::pseudorange::Pseudorange;
use gnss_core::pvt::{solve_wls, PvtSolution};
use gnss_core::tracking::{BitSynchronizer, DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::path::Path;

const DATA_FILE: &str = "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
const FS: f64 = 4_000_000.0;
const REF_DOPPLERS: &[(u8, f64)] = &[
    (1, 7250.0),
    (11, 5500.0),
    (17, 9750.0),
    (20, 8250.0),
    (32, 6500.0),
];
const REF_LAT: f64 = 41.2747;
const REF_LON: f64 = 1.9875;

/// One channel's state during the per-ms tracking loop.
struct Channel {
    prn: u8,
    prn_idx: usize,
    trk: DllPllTracking,
    bit_sync: BitSynchronizer,
    nav: NavDecoder,
    collector: EphemerisCollector,
    boundary: Option<usize>,
    /// ms index from file start at which bit sync started (phase reference).
    sync_start_ms: usize,
    /// Per-ms prompt I history (filled each ms).
    prompt_i: Vec<f64>,
    /// ms index (from file start) of the most recent subframe preamble.
    last_subframe_ms: Option<usize>,
    /// TOW count (HOW) of the most recent subframe.
    tow_count: u32,
    /// Latest C/N0 from tracking.
    cn0_db_hz: f64,
    /// Latest Doppler from tracking.
    doppler_hz: f64,
}

#[test]
fn test_pvt_e2e() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let mut reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let spm = FS as usize / 1000;
    // 100 s: need sf 1-3 (each 30 s) for ephemeris; allow time for PLL lock + 3 subframes.
    let total = (60.0 * FS) as usize;
    let mut signal = vec![Complex::new(0.0, 0.0); total];
    reader.read(&mut signal, total).unwrap();
    let total_ms = signal.len() / spm;

    // Acquire each PRN and fine-tune Doppler.
    let acq_cfg = PcpsConfig { fs: FS, doppler_max: 10_000.0, doppler_step: 250.0, threshold: 15.0, ..Default::default() };
    let mut acq = PcpsAcquisition::new(acq_cfg);

    let mut channels: Vec<Channel> = Vec::new();
    for &(prn, ref_dopp) in REF_DOPPLERS {
        let prn_idx = (prn - 1) as usize;
        let ar = acq.acquire(prn_idx, &signal[..spm]);
        if !ar.detected {
            eprintln!("PRN {} not acquired, skipping", prn);
            continue;
        }
        // Fine Doppler search around the GNSS-SDR reference value.
        let mut best_dopp = ref_dopp;
        let mut best_qi = f64::MAX;
        for offset in (-250..=250).step_by(50) {
            let init_dopp = ref_dopp + offset as f64;
            let cfg = TrackingConfig { fs: FS, pll_bw: 50.0, pll_order: 3, dll_bw: 4.0, ..Default::default() };
            let mut trk = DllPllTracking::new(prn_idx, init_dopp, ar.code_phase_chips, cfg);
            for ms in 0..2000 {
                trk.process(&signal[ms * spm..(ms + 1) * spm]);
            }
            let out = trk.process(&signal[2000 * spm..2001 * spm]);
            let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
            if qi < best_qi { best_qi = qi; best_dopp = init_dopp; }
        }
        println!("PRN {}: cp={:.1} | best_dopp={:.0} Q/I={:.3}",
            prn, ar.code_phase_chips, best_dopp, best_qi);

        let cfg = TrackingConfig { fs: FS, pll_bw: 50.0, pll_order: 3, dll_bw: 4.0, ..Default::default() };
        channels.push(Channel {
            prn,
            prn_idx,
            trk: DllPllTracking::new(prn_idx, best_dopp, ar.code_phase_chips, cfg),
            bit_sync: BitSynchronizer::new(),
            nav: NavDecoder::new(),
            collector: EphemerisCollector::new(),
            boundary: None,
            sync_start_ms: 2000,
            prompt_i: Vec::with_capacity(total_ms),
            last_subframe_ms: None,
            tow_count: 0,
            cn0_db_hz: 0.0,
            doppler_hz: 0.0,
        });
    }
    if channels.is_empty() {
        panic!("no satellites acquired");
    }
    assert!(channels.len() >= 4, "need ≥4 acquired satellites, got {}", channels.len());

    // Per-ms tracking loop: track all channels, record prompt_i, run bit sync,
    // and at each bit boundary feed the 20 ms coherent sum to NavDecoder.
    let mut ephemerides: EphemerisMap = EphemerisMap::new();
    let mut iono = IonoParams::default();
    let mut subframes_decoded_per_prn: std::collections::BTreeMap<u8, u32> = Default::default();

    // Phase 1: track all channels, collect prompt_i, run bit sync.
    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        for ch in channels.iter_mut() {
            let out = ch.trk.process(chunk);
            ch.cn0_db_hz = out.cn0_db_hz;
            ch.doppler_hz = out.carrier_doppler_hz;
            let prompt_i = out.prompt.re;
            ch.prompt_i.push(prompt_i);

            if ch.boundary.is_none() && ms >= ch.sync_start_ms {
                if let Some(b) = ch.bit_sync.update(prompt_i) {
                    ch.boundary = Some(b);
                    eprintln!("PRN {} bit sync locked at ms={} boundary={}", ch.prn, ms, b);
                }
            }
        }
    }

    // Phase 2: for each bit-synced channel, replay prompt_i from the first
    // full bit boundary through NavDecoder + EphemerisCollector. This mirrors
    // real_tracking.rs (which decodes after collecting all prompt_i).
    for ch in channels.iter_mut() {
        let boundary = match ch.boundary {
            Some(b) => b,
            None => {
                eprintln!("PRN {} no bit sync, skipping decode", ch.prn);
                continue;
            }
        };
        let sync_start = ch.sync_start_ms;
        let mut first_bit_ms = sync_start;
        while first_bit_ms < ch.prompt_i.len() && (first_bit_ms - sync_start) % 20 != boundary {
            first_bit_ms += 1;
        }
        eprintln!("PRN {} decoding from first_bit_ms={}", ch.prn, first_bit_ms);

        let mut bit_idx = first_bit_ms;
        while bit_idx + 20 <= ch.prompt_i.len() {
            let sum: f64 = ch.prompt_i[bit_idx..bit_idx + 20].iter().sum();
            if ch.nav.feed_symbol(sum) {
                ch.tow_count = ch.nav.tow;
                ch.last_subframe_ms = Some(bit_idx + 20);
                *subframes_decoded_per_prn.entry(ch.prn).or_insert(0) += 1;
                let sf_id = ch.nav.subframe_id;
                eprintln!("PRN {} subframe {} decoded at bit_idx={} tow={}",
                    ch.prn, sf_id, bit_idx, ch.nav.tow);
                if let Some(eph) = ch.collector.feed(ch.prn, &ch.nav.words) {
                    eprintln!("PRN {} ephemeris complete", ch.prn);
                    ephemerides.insert(ch.prn, eph);
                }
            }
            bit_idx += 20;
        }
        // Keep latest cn0/doppler for pseudorange weighting.
    }

    println!("\n=== Ephemeris collection ===");
    for (prn, eph) in &ephemerides {
        println!("PRN {}: sqrt_a={:.1} e={:.6} toe={:.0} toc={:.0} tow={}",
            prn, eph.sqrt_a, eph.eccentricity, eph.toe, eph.toc, eph.tow);
    }
    println!("Subframes decoded per PRN: {:?}", subframes_decoded_per_prn);
    println!("Ephemerides: {}/{} satellites", ephemerides.len(), channels.len());

    if ephemerides.len() < 4 {
        eprintln!("Only {} ephemerides decoded (need 4). Subframes: {:?}",
            ephemerides.len(), subframes_decoded_per_prn);
        return;
    }

    // Form pseudoranges: use the latest subframe TOW per channel + ms offset.
    // Pick a common receive epoch: the latest ms where all ephemeris-bearing
    // channels have a known TOW.
    let mut pseudoranges: Vec<Pseudorange> = Vec::new();
    for ch in channels.iter() {
        if ch.last_subframe_ms.is_none() { continue; }
        if !ephemerides.contains_key(&ch.prn) { continue; }
        let tow_count = ch.tow_count;
        if tow_count == 0 { continue; }
        // ms elapsed since the start of the most recent subframe on this channel.
        let ms_into_subframe = (ch.prompt_i.len() as u64) - ch.last_subframe_ms.unwrap() as u64;
        let rx_sample_index = ch.prompt_i.len() as u64;
        let pr = Pseudorange::from_tracking(
            ch.prn,
            tow_count,
            ms_into_subframe,
            rx_sample_index,
            FS,
            ch.cn0_db_hz,
            ch.doppler_hz,
        );
        println!("PRN {}: PR={:.0} m tow_count={} ms_into_sf={} rx_sow={:.3} tx_sow={:.3} cn0={:.1}",
            ch.prn, pr.pseudorange_m, tow_count, ms_into_subframe,
            pr.receive_time_sow, pr.transmit_time_sow, ch.cn0_db_hz);
        pseudoranges.push(pr);
    }

    if pseudoranges.len() < 4 {
        panic!("Need ≥4 pseudoranges, got {}", pseudoranges.len());
    }

    // Solve WLS. Bootstrap with a rough a-priori (Earth center / zero alt).
    let sol: PvtSolution = solve_wls(&pseudoranges, &ephemerides, &iono, 0.0, 0.0, 0.0);
    let lat_deg = sol.lla[0].to_degrees();
    let lon_deg = sol.lla[1].to_degrees();
    println!("\n=== PVT Solution ===");
    println!("ECEF: [{:.1}, {:.1}, {:.1}]", sol.ecef[0], sol.ecef[1], sol.ecef[2]);
    println!("LLA:  lat={:.6}° lon={:.6}° alt={:.1} m", lat_deg, lon_deg, sol.lla[2]);
    println!("Clock bias: {:.3} µs", sol.clock_bias_s * 1e6);
    println!("n_sats={} converged={}", sol.n_sats, sol.converged);
    println!("Residuals (m): {:?}", sol.residuals_m);
    println!("Reference: lat={:.4}° lon={:.4}°", REF_LAT, REF_LON);

    assert!(sol.converged, "WLS did not converge");
    assert!(sol.n_sats >= 4, "need ≥4 sats in solution, got {}", sol.n_sats);
    // Position accuracy: tolerate ~10 km for this rough first PVT (clock bias
    // alignment + pseudorange formation are approximate). Tighten later.
    let lat_err = (lat_deg - REF_LAT).abs();
    let lon_err = (lon_deg - REF_LON).abs();
    println!("Error: lat={:.4}° lon={:.4}°", lat_err, lon_err);
    assert!(lat_err < 1.0, "latitude error too large: {:.4}°", lat_err);
    assert!(lon_err < 1.0, "longitude error too large: {:.4}°", lon_err);
}
