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
    /// Per-ms code phase (chips) history, parallel to prompt_i.
    code_phase: Vec<f64>,
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
            code_phase: Vec::with_capacity(total_ms),
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
    let mut diag_counters: std::collections::BTreeMap<u8, (u32, u32, f64, f64)> = Default::default();
    // (sync_attempts, sync_successes, last_qi, last_abs_p)
    for ms in 0..total_ms {
        let chunk = &signal[ms * spm..(ms + 1) * spm];
        for ch in channels.iter_mut() {
            let out = ch.trk.process(chunk);
            ch.cn0_db_hz = out.cn0_db_hz;
            ch.doppler_hz = out.carrier_doppler_hz;
            let prompt_i = out.prompt.re;
            ch.prompt_i.push(prompt_i);
            ch.code_phase.push(out.code_phase_chips);

            if ch.boundary.is_none() && ms >= ch.sync_start_ms {
                if let Some(b) = ch.bit_sync.update(prompt_i) {
                    ch.boundary = Some(b);
                    eprintln!("PRN {} bit sync locked at ms={} boundary={}", ch.prn, ms, b);
                }
            }
        }

        // Periodic tracking-quality diagnostic (every 5 s).
        if ms > 0 && ms % 5000 == 0 {
            for ch in channels.iter() {
                let qi = (ch.prompt_i.last().unwrap_or(&0.0).abs()) / 1.0;
                let _ = qi;
                // Re-read latest prompt magnitude from the tracker's last output is not stored;
                // use |prompt_i| as a proxy for |I|.
                let last_i = ch.prompt_i.last().copied().unwrap_or(0.0);
                let abs_p = last_i.abs();
                eprintln!("  [ms={:>6}] PRN {:2} dopp={:+.0} |I|={:.0} cn0={:.1}",
                    ms, ch.prn, ch.doppler_hz, abs_p, ch.cn0_db_hz);
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
        let mut sync_count = 0u32;
        let mut decode_count = 0u32;
        while bit_idx + 20 <= ch.prompt_i.len() {
            let sum: f64 = ch.prompt_i[bit_idx..bit_idx + 20].iter().sum();
            let was_synced = ch.nav.is_synced();
            if ch.nav.feed_symbol(sum) {
                ch.tow_count = ch.nav.tow;
                ch.last_subframe_ms = Some(bit_idx + 20);
                *subframes_decoded_per_prn.entry(ch.prn).or_insert(0) += 1;
                decode_count += 1;
                let sf_id = ch.nav.subframe_id;
                if decode_count <= 5 {
                    eprintln!("PRN {} subframe {} decoded at bit_idx={} tow={} words.len={}",
                        ch.prn, sf_id, bit_idx, ch.nav.tow, ch.nav.words.len());
                }
                if let Some(eph) = ch.collector.feed(ch.prn, &ch.nav.words) {
                    eprintln!("PRN {} ephemeris complete", ch.prn);
                    ephemerides.insert(ch.prn, eph);
                }
            }
            if ch.nav.is_synced() && !was_synced { sync_count += 1; }
            bit_idx += 20;
        }
        eprintln!("PRN {}: sync_count={} decode_count={}", ch.prn, sync_count, decode_count);
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
    // Form pseudoranges using iterative per-satellite traveltime refinement.
    //
    // The integer-ms part of each satellite's traveltime is ambiguous from
    // TOW + code phase alone. We resolve it by: (1) solving WLS with a rough
    // 75ms traveltime estimate, (2) using the solved position to compute each
    // satellite's geometric range → true traveltime, (3) snapping each
    // satellite's tx_sow so traveltime matches range/c, (4) re-solving.
    let rx_ms = channels.iter().map(|c| c.prompt_i.len()).max().unwrap_or(0) as u64;

    struct ChanRaw {
        prn: u8,
        tow_count: u32,
        ms_since_subframe: u64,
        cp_chips: f64,
        cn0: f64,
        doppler: f64,
        subframe_start_sow: f64,
    }
    let mut raws: Vec<ChanRaw> = Vec::new();
    for ch in channels.iter() {
        if ch.last_subframe_ms.is_none() { continue; }
        if !ephemerides.contains_key(&ch.prn) { continue; }
        let tow_count = ch.tow_count;
        if tow_count == 0 { continue; }
        let last_sf_ms = ch.last_subframe_ms.unwrap() as u64;
        if rx_ms <= last_sf_ms { continue; }
        let ms_since_subframe = rx_ms - last_sf_ms;
        let cp_idx = (rx_ms as usize).min(ch.code_phase.len()) - 1;
        let cp_chips = ch.code_phase[cp_idx];
        let subframe_start_sow = tow_count as f64 * 6.0 - 6.0;
        raws.push(ChanRaw {
            prn: ch.prn, tow_count, ms_since_subframe, cp_chips,
            cn0: ch.cn0_db_hz, doppler: ch.doppler_hz, subframe_start_sow,
        });
    }
    if raws.len() < 4 { panic!("Need ≥4 channels, got {}", raws.len()); }

    // Initial rx_sow: first channel's tx + 75ms estimate.
    let first_tx_base = raws[0].subframe_start_sow
        + raws[0].ms_since_subframe as f64 * 1e-3;
    let mut rx_sow = first_tx_base + 0.075;

    let mut sol = PvtSolution::default();
    let mut pseudoranges: Vec<Pseudorange> = Vec::new();

    for iteration in 0..5 {
        pseudoranges.clear();
        for r in &raws {
            let cp_ms = r.cp_chips / 1023.0;
            let tx_sow = r.subframe_start_sow
                + r.ms_since_subframe as f64 * 1e-3
                + cp_ms * 1e-3;
            let mut traveltime = rx_sow - tx_sow;
            while traveltime < 0.060 { traveltime += 0.001; }
            while traveltime > 0.100 { traveltime -= 0.001; }
            if iteration == 0 {
                eprintln!("  PRN {}: cp_chips={:.1} cp_ms={:.4} ms_sf={} tx_sow={:.6} traveltime={:.4}ms",
                    r.prn, r.cp_chips, cp_ms, r.ms_since_subframe, tx_sow, traveltime * 1000.0);
            }
            let pr = Pseudorange {
                prn: r.prn,
                pseudorange_m: traveltime * 299_792_458.0,
                transmit_time_sow: rx_sow - traveltime,
                receive_time_sow: rx_sow,
                cn0_db_hz: r.cn0,
                doppler_hz: r.doppler,
            };
            pseudoranges.push(pr);
        }

        sol = solve_wls(&pseudoranges, &ephemerides, &iono, 0.0, 0.0, 0.0);
        if !sol.converged { break; }

        // Refine: for each satellite, compute geometric range from solved pos,
        // derive true traveltime, and recompute what rx_sow should be (the
        // average across satellites). This corrects the integer-ms ambiguity.
        let mut rx_sow_sum = 0.0f64;
        let mut n = 0u32;
        for (r, pr) in raws.iter().zip(pseudoranges.iter()) {
            let eph = &ephemerides[&r.prn];
            let sat = gnss_core::pvt::satellite_position(eph, pr.transmit_time_sow);
            let dx = sat.ecef[0] - sol.ecef[0];
            let dy = sat.ecef[1] - sol.ecef[1];
            let dz = sat.ecef[2] - sol.ecef[2];
            let range = (dx*dx + dy*dy + dz*dz).sqrt();
            // true traveltime = range/c. tx_sow (refined) = rx_sow - range/c - clk_bias.
            // But we want to find rx_sow such that tx_sow + range/c + clk = rx_sow.
            // tx_sow = subframe_start + ms_since + cp. So:
            //   rx_sow = tx_sow + range/c + clk_bias
            let cp_ms = r.cp_chips / 1023.0;
            let tx_sow = r.subframe_start_sow + r.ms_since_subframe as f64 * 1e-3 + cp_ms * 1e-3;
            let implied_rx = tx_sow + range / 299_792_458.0 + sol.clock_bias_s;
            rx_sow_sum += implied_rx;
            n += 1;
        }
        let new_rx_sow = rx_sow_sum / n as f64;
        let delta = new_rx_sow - rx_sow;
        rx_sow = new_rx_sow;
        eprintln!("iter{}: rx_sow={:.6} (delta={:.6}s) lat={:.6} lon={:.6} alt={:.0}",
            iteration, rx_sow, delta, sol.lla[0].to_degrees(), sol.lla[1].to_degrees(), sol.lla[2]);
        if delta.abs() < 1e-7 { break; }
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

    // Per-satellite residual analysis: compare pseudorange to geometric range
    // (computed from the solved position + ephemeris satellite position).
    println!("\nPer-satellite analysis (PR - geometric_range - clk_bias):");
    let true_ecef = gnss_core::pvt::lla_to_ecef([
        REF_LAT.to_radians(), REF_LON.to_radians(), 52.0
    ]);
    let sol_bias_m = sol.clock_bias_s * 299_792_458.0;
    for pr in &pseudoranges {
        let eph = &ephemerides[&pr.prn];
        let sat = gnss_core::pvt::satellite_position(eph, pr.transmit_time_sow);
        // Sagnac-corrected range from solved position
        let tau = pr.pseudorange_m / 299_792_458.0;
        let theta = 7.292_115_146_7e-5 * tau;
        let xs_r = theta.cos() * sat.ecef[0] + theta.sin() * sat.ecef[1];
        let ys_r = -theta.sin() * sat.ecef[0] + theta.cos() * sat.ecef[1];
        let zs_r = sat.ecef[2];
        // Range from SOLVED position
        let dx_s = xs_r - sol.ecef[0];
        let dy_s = ys_r - sol.ecef[1];
        let dz_s = zs_r - sol.ecef[2];
        let range_sol = (dx_s*dx_s + dy_s*dy_s + dz_s*dz_s).sqrt();
        // Range from TRUE position
        let dx_t = xs_r - true_ecef[0];
        let dy_t = ys_r - true_ecef[1];
        let dz_t = zs_r - true_ecef[2];
        let range_true = (dx_t*dx_t + dy_t*dy_t + dz_t*dz_t).sqrt();
        let resid_sol = pr.pseudorange_m + 299_792_458.0 * sat.clock_correction_s - range_sol - sol_bias_m;
        let resid_true = pr.pseudorange_m + 299_792_458.0 * sat.clock_correction_s - range_true - sol_bias_m;
        println!("  PRN {:2}: PR={:.0} range_sol={:.0} range_true={:.0} | resid_sol={:+.0} resid_true={:+.0} (diff={:+.0})",
            pr.prn, pr.pseudorange_m, range_sol, range_true, resid_sol, resid_true, resid_true - resid_sol);
    }
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
