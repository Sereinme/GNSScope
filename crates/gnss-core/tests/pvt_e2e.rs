//! M2 end-to-end PVT test on real CTTC Spain data.
//!
//! Track 5 satellites (PRN 1, 11, 17, 20, 32 per GNSS-SDR reference), decode
//! subframes 1-3, parse ephemerides, form pseudoranges, run WLS solver, and
//! assert the position is near the CTTC reference: 41.2747°N, 1.9875°E.

use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::io::{IqFormat, IqReader};
use gnss_core::nav::{
    load_optional_rinex_iono, parse_iono, EphemerisCollector, EphemerisMap, NavDecoder,
    RinexNavConfig,
};
use gnss_core::pvt::{
    from_common_receive_epoch, from_tow_history_common_receive_epoch,
    from_tracking_timestamp_common_receive_epoch, pseudorange_delta_stats, solve_wls,
    ContinuousPvtConfig, ContinuousPvtRunner, HatchConfig, IntegerMsAmbiguityConfig, KalmanConfig,
    ObservablesBuilder, ObservablesInputBuilder, ObservablesSource, PseudorangeEpoch,
    PvtEpochOutput, PvtEpochSolutionKind, PvtSolution, TowSample, TrackingChannelSnapshot,
    TrackingTimestampObservation,
};
use gnss_core::tracking::{BitSynchronizer, DllPllTracking, TrackingConfig};
use num_complex::Complex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const DATA_FILE: &str =
    "../../data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat";
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
const NAV_TOW_TAG_LATENCY_MS: usize = 8 * 20;
const PROCESS_MS: usize = 60_000;
const STREAM_CHUNK_MS: usize = 1;
const LEGACY_FINE_DOPPLER_WINDOW_MS: usize = 2_001;

/// One channel's state during the per-ms tracking loop.
struct Channel {
    prn: u8,
    trk: DllPllTracking,
    bit_sync: BitSynchronizer,
    nav: NavDecoder,
    collector: EphemerisCollector,
    boundary: Option<usize>,
    sync_start_ms: usize,
    prompt_i: Vec<f64>,
    code_phase: Vec<f64>,
    sample_counter: Vec<u64>,
    tracking_sample_counter: Vec<u64>,
    rem_code_phase_samples: Vec<f64>,
    dll_error_history_chips: Vec<f64>,
    cn0_history_db_hz: Vec<f64>,
    doppler_history_hz: Vec<f64>,
    last_subframe_ms: Option<usize>,
    last_subframe_sample: Option<u64>,
    tow_count: u32,
    cn0_db_hz: f64,
    doppler_hz: f64,
}

struct TrackingWorker {
    command_tx: SyncSender<TrackingWorkerCommand>,
    report_rx: Receiver<TrackingStepReport>,
    join: JoinHandle<Channel>,
}

enum TrackingWorkerCommand {
    Process {
        ms: usize,
        chunk: Arc<[Complex<f64>]>,
    },
    Finish,
}

#[derive(Debug, Clone)]
struct TrackingStepReport {
    prn: u8,
    ms: usize,
    doppler_hz: f64,
    cn0_db_hz: f64,
    prompt_i_abs: f64,
    bit_sync_boundary: Option<usize>,
}

#[test]
fn test_pvt_e2e() {
    let data_path = Path::new(DATA_FILE);
    if !data_path.exists() {
        eprintln!("SKIP: real data not present");
        return;
    }

    let spm = FS as usize / 1000;
    let available_ms = (IqReader::open(data_path, IqFormat::Int16)
        .unwrap()
        .total_samples() as usize)
        / spm;
    // 60 s: need sf 1-3 (each 30 s) for ephemeris; allow time for PLL lock + 3 subframes.
    let total_ms = PROCESS_MS.min(available_ms);
    assert!(
        total_ms >= LEGACY_FINE_DOPPLER_WINDOW_MS,
        "need at least {} ms of IQ for legacy fine Doppler, got {} ms",
        LEGACY_FINE_DOPPLER_WINDOW_MS,
        total_ms
    );
    let fine_window = read_iq_window_ms(
        data_path,
        IqFormat::Int16,
        LEGACY_FINE_DOPPLER_WINDOW_MS,
        spm,
    );

    // Acquire each PRN and fine-tune Doppler.
    let acq_cfg = PcpsConfig {
        fs: FS,
        doppler_max: 10_000.0,
        doppler_step: 250.0,
        threshold: 15.0,
        ..Default::default()
    };
    let mut acq = PcpsAcquisition::new(acq_cfg);

    let mut channels: Vec<Channel> = Vec::new();
    for &(prn, ref_dopp) in REF_DOPPLERS {
        let prn_idx = (prn - 1) as usize;
        let ar = acq.acquire_at_sample(prn_idx, ms_slice(&fine_window, 0, spm), 0);
        if !ar.detected {
            eprintln!("PRN {} not acquired, skipping", prn);
            continue;
        }
        // Fine Doppler search around the GNSS-SDR reference value.
        let mut best_dopp = ref_dopp;
        let mut best_qi = f64::MAX;
        for offset in (-250..=250).step_by(50) {
            let init_dopp = ref_dopp + offset as f64;
            let cfg = TrackingConfig {
                fs: FS,
                pll_bw: 50.0,
                pll_order: 3,
                dll_bw: 4.0,
                ..Default::default()
            };
            let mut trk = DllPllTracking::new(
                prn_idx,
                init_dopp,
                ar.code_phase_chips,
                cfg,
                ar.sample_index,
            );
            for ms in 0..2000 {
                trk.process(ms_slice(&fine_window, ms, spm));
            }
            let out = trk.process(ms_slice(&fine_window, 2000, spm));
            let qi = (out.prompt.im / (out.prompt.re.abs() + 1e-30)).abs();
            if qi < best_qi {
                best_qi = qi;
                best_dopp = init_dopp;
            }
        }
        println!(
            "PRN {}: cp={:.1} | best_dopp={:.0} Q/I={:.3}",
            prn, ar.code_phase_chips, best_dopp, best_qi
        );

        let cfg = TrackingConfig {
            fs: FS,
            pll_bw: 50.0,
            pll_order: 3,
            dll_bw: 4.0,
            ..Default::default()
        };
        channels.push(Channel {
            prn,
            trk: DllPllTracking::new(
                prn_idx,
                best_dopp,
                ar.code_phase_chips,
                cfg,
                ar.sample_index,
            ),
            bit_sync: BitSynchronizer::new(),
            nav: NavDecoder::new(),
            collector: EphemerisCollector::new(),
            boundary: None,
            sync_start_ms: 2000,
            prompt_i: Vec::with_capacity(total_ms),
            code_phase: Vec::with_capacity(total_ms),
            sample_counter: Vec::with_capacity(total_ms),
            tracking_sample_counter: Vec::with_capacity(total_ms),
            rem_code_phase_samples: Vec::with_capacity(total_ms),
            dll_error_history_chips: Vec::with_capacity(total_ms),
            cn0_history_db_hz: Vec::with_capacity(total_ms),
            doppler_history_hz: Vec::with_capacity(total_ms),
            last_subframe_ms: None,
            last_subframe_sample: None,
            tow_count: 0,
            cn0_db_hz: 0.0,
            doppler_hz: 0.0,
        });
    }
    if channels.is_empty() {
        panic!("no satellites acquired");
    }
    assert!(
        channels.len() >= 4,
        "need ≥4 acquired satellites, got {}",
        channels.len()
    );

    // Per-ms tracking loop: track all channels, record prompt_i, run bit sync,
    // and at each bit boundary feed the 20 ms coherent sum to NavDecoder.
    let mut ephemerides: EphemerisMap = EphemerisMap::new();
    let iono_fallback = load_optional_rinex_iono(data_path, &RinexNavConfig::default());
    let mut iono = iono_fallback
        .as_ref()
        .map(|fallback| fallback.iono.clone())
        .unwrap_or_default();
    let mut iono_source = iono_fallback
        .as_ref()
        .map(|fallback| fallback.candidate.source_label.as_str())
        .unwrap_or("none");
    let mut subframes_decoded_per_prn: std::collections::BTreeMap<u8, u32> = Default::default();

    channels.sort_by_key(|ch| ch.prn);

    // Phase 1: track all channels in parallel, collect prompt_i, run bit sync.
    let mut stream_reader = IqReader::open(data_path, IqFormat::Int16).unwrap();
    let workers: Vec<TrackingWorker> = channels.into_iter().map(spawn_tracking_worker).collect();
    for ms in 0..total_ms {
        let mut chunk = vec![Complex::new(0.0, 0.0); spm * STREAM_CHUNK_MS];
        read_iq_exact(&mut stream_reader, &mut chunk, spm);
        let chunk: Arc<[Complex<f64>]> = Arc::from(chunk.into_boxed_slice());
        for worker in &workers {
            worker
                .command_tx
                .send(TrackingWorkerCommand::Process {
                    ms,
                    chunk: Arc::clone(&chunk),
                })
                .expect("tracking worker should accept process command");
        }

        let mut reports = Vec::with_capacity(workers.len());
        for worker in &workers {
            reports.push(
                worker
                    .report_rx
                    .recv()
                    .expect("tracking worker should report each ms"),
            );
        }
        reports.sort_by_key(|report| report.prn);

        for report in reports
            .iter()
            .filter(|report| report.bit_sync_boundary.is_some())
        {
            eprintln!(
                "PRN {} bit sync locked at ms={} boundary={}",
                report.prn,
                report.ms,
                report.bit_sync_boundary.unwrap()
            );
        }

        // Periodic tracking-quality diagnostic (every 5 s).
        if ms > 0 && ms % 5000 == 0 {
            for report in &reports {
                eprintln!(
                    "  [ms={:>6}] PRN {:2} dopp={:+.0} |I|={:.0} cn0={:.1}",
                    ms, report.prn, report.doppler_hz, report.prompt_i_abs, report.cn0_db_hz
                );
            }
        }
    }
    let mut channels: Vec<Channel> = workers
        .into_iter()
        .map(|worker| {
            worker
                .command_tx
                .send(TrackingWorkerCommand::Finish)
                .expect("tracking worker should accept finish command");
            worker.join.join().expect("tracking worker should join")
        })
        .collect();
    channels.sort_by_key(|ch| ch.prn);

    // Phase 2: for each bit-synced channel, replay prompt_i from the first
    // full bit boundary through NavDecoder + EphemerisCollector. This mirrors
    // real_tracking.rs (which decodes after collecting all prompt_i).
    // Also record per-symbol TOW for later interpolation.
    let mut per_channel_tow: BTreeMap<u8, Vec<TowSample>> = BTreeMap::new();
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

        let mut tow_history: Vec<TowSample> = Vec::new();
        let mut bit_idx = first_bit_ms;
        while bit_idx + 20 <= ch.prompt_i.len() {
            let sum: f64 = ch.prompt_i[bit_idx..bit_idx + 20].iter().sum();
            if ch.nav.feed_symbol(sum) {
                ch.tow_count = ch.nav.tow;
                ch.last_subframe_ms = Some(bit_idx + 20);
                ch.last_subframe_sample = ch.sample_counter.get(bit_idx + 19).copied();
                *subframes_decoded_per_prn.entry(ch.prn).or_insert(0) += 1;
                let sf_id = ch.nav.subframe_id;
                if subframes_decoded_per_prn[&ch.prn] <= 2 {
                    eprintln!(
                        "PRN {} subframe {} decoded at bit_idx={} tow={}",
                        ch.prn, sf_id, bit_idx, ch.nav.tow
                    );
                }
                if let Some(eph) = ch.collector.feed(ch.prn, &ch.nav.words) {
                    eprintln!("PRN {} ephemeris complete", ch.prn);
                    ephemerides.insert(ch.prn, eph);
                }
                if let Some(params) = parse_iono(&ch.nav.words) {
                    eprintln!("PRN {} ionosphere page 18 decoded", ch.prn);
                    iono = params;
                    iono_source = "broadcast-page18";
                }
            }
            // Record TOW at each symbol (for interpolation).
            if ch.nav.tow_set() {
                if let Some(rx_sample) = ch.sample_counter.get(bit_idx + 19) {
                    tow_history.push(TowSample {
                        rx_sample: rx_sample.saturating_sub((NAV_TOW_TAG_LATENCY_MS * spm) as u64)
                            as f64,
                        tow_sow: ch.nav.tow_at_current_symbol_ms() as f64 * 1e-3,
                    });
                }
            }
            bit_idx += 20;
        }
        per_channel_tow.insert(ch.prn, tow_history);
    }

    println!("\n=== Ephemeris collection ===");
    for (prn, eph) in &ephemerides {
        println!(
            "PRN {}: sqrt_a={:.1} e={:.6} toe={:.0} toc={:.0} tow={}",
            prn, eph.sqrt_a, eph.eccentricity, eph.toe, eph.toc, eph.tow
        );
    }
    println!("Subframes decoded per PRN: {:?}", subframes_decoded_per_prn);
    println!(
        "Ephemerides: {}/{} satellites",
        ephemerides.len(),
        channels.len()
    );
    println!(
        "Ionosphere source: {} valid={} alpha={:?} beta={:?}",
        iono_source, iono.valid, iono.alpha, iono.beta
    );

    if ephemerides.len() < 4 {
        eprintln!(
            "Only {} ephemerides decoded (need 4). Subframes: {:?}",
            ephemerides.len(),
            subframes_decoded_per_prn
        );
        return;
    }

    // === Pseudorange formation (raw NDO, best known method) ===
    let rx_ms = channels.iter().map(|c| c.prompt_i.len()).max().unwrap_or(0) as u64;
    let rx_sample = channels
        .iter()
        .filter_map(|c| c.sample_counter.last().copied())
        .max()
        .unwrap_or(rx_ms * spm as u64);

    let mut channel_snapshots: Vec<TrackingChannelSnapshot<'_>> = Vec::new();
    println!("\nPRN  tow  ms_sf   NDO    cp_ms   travel   PR");
    for ch in channels.iter() {
        if ch.last_subframe_ms.is_none() {
            continue;
        }
        if ch.last_subframe_sample.is_none() {
            continue;
        }
        if !ephemerides.contains_key(&ch.prn) {
            continue;
        }
        let tow_count = ch.tow_count;
        if tow_count == 0 {
            continue;
        }
        let last_sf_ms = ch.last_subframe_ms.unwrap() as u64;
        if rx_ms <= last_sf_ms {
            continue;
        }
        let ms_since_subframe = rx_ms - last_sf_ms;
        let cp_idx = (rx_ms as usize).min(ch.code_phase.len()) - 1;
        let nco_chips = ch.code_phase[cp_idx];
        let rem_code_phase_samples = ch.rem_code_phase_samples[cp_idx];
        let code_phase_ms = nco_chips / 1023.0;
        let tow_history = per_channel_tow
            .get(&ch.prn)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        channel_snapshots.push(TrackingChannelSnapshot {
            prn: ch.prn,
            tow_count,
            how_epoch_rx_sample: ch.last_subframe_sample.unwrap() as f64,
            code_phase_chips: nco_chips,
            tracking_sample_counter: ch.tracking_sample_counter[cp_idx],
            rem_code_phase_samples,
            tow_history,
            cn0_db_hz: ch.cn0_db_hz,
            doppler_hz: ch.doppler_hz,
            tracking_jitter_chips: ch.dll_error_history_chips[cp_idx],
        });
        println!(
            "{:3} {:4} {:6} NDO={:6.1} cp_ms={:.4} rem_samp={:.3}",
            ch.prn, tow_count, ms_since_subframe, nco_chips, code_phase_ms, rem_code_phase_samples
        );
    }
    let observable_inputs = ObservablesInputBuilder::new(rx_sample as f64, FS)
        .with_channel_snapshots(&channel_snapshots)
        .build();
    let pseudoranges_nco = from_common_receive_epoch(
        &observable_inputs.nco_reference,
        rx_sample as f64,
        FS,
        0.075,
    );
    println!(
        "TOW-history interpolation delta: min={:.6} ms max={:.6} ms",
        observable_inputs.tow_history_delta_min_ms, observable_inputs.tow_history_delta_max_ms
    );
    let pseudoranges_quantized = from_common_receive_epoch(
        &observable_inputs.quantized_reference,
        rx_sample as f64,
        FS,
        0.075,
    );
    let pseudoranges_rx_rem = from_common_receive_epoch(
        &observable_inputs.rx_rem_reference,
        rx_sample as f64,
        FS,
        0.075,
    );
    let max_code_phase_fraction_delta_m = pseudoranges_nco
        .iter()
        .zip(pseudoranges_quantized.iter())
        .map(|(full, quantized)| (full.pseudorange_m - quantized.pseudorange_m).abs())
        .fold(0.0_f64, f64::max);
    println!(
        "Max fractional code-phase PR delta: {:.6} m",
        max_code_phase_fraction_delta_m
    );
    let max_rx_rem_delta_m = pseudoranges_nco
        .iter()
        .zip(pseudoranges_rx_rem.iter())
        .map(|(base, rx_rem)| (rx_rem.pseudorange_m - base.pseudorange_m).abs())
        .fold(0.0_f64, f64::max);
    println!(
        "Max rem_code_phase receive-time PR delta: {:.6} m",
        max_rx_rem_delta_m
    );
    let pseudoranges_tow_history_code_phase = from_tow_history_common_receive_epoch(
        &observable_inputs.tow_history_code_phase,
        rx_sample as f64,
        FS,
        0.075,
    );
    if pseudoranges_tow_history_code_phase.len() >= 4 {
        let max_tow_history_pr_delta_m = pseudoranges_nco
            .iter()
            .filter_map(|base| {
                pseudoranges_tow_history_code_phase
                    .iter()
                    .find(|candidate| candidate.prn == base.prn)
                    .map(|tow| (tow.pseudorange_m - base.pseudorange_m).abs())
            })
            .fold(0.0_f64, f64::max);
        let (_, centered_tow_history_pr_delta_m) =
            pseudorange_delta_stats(&pseudoranges_nco, &pseudoranges_tow_history_code_phase);
        println!(
            "Max TOW-history+code-phase PR delta: raw={:.6} m centered={:.6} m",
            max_tow_history_pr_delta_m, centered_tow_history_pr_delta_m
        );
        let sol_tow_history = solve_wls(
            &pseudoranges_tow_history_code_phase,
            &ephemerides,
            &iono,
            0.0,
            0.0,
            0.0,
        );
        println!(
            "TOW-history+code-phase LLA: lat={:.6} lon={:.6} alt={:.1} converged={}",
            sol_tow_history.lla[0].to_degrees(),
            sol_tow_history.lla[1].to_degrees(),
            sol_tow_history.lla[2],
            sol_tow_history.converged
        );
    }
    let observables = ObservablesBuilder::new(rx_sample as f64, FS)
        .with_tracking_timestamp_observations(&observable_inputs.tracking_timestamp)
        .with_nco_reference_observations(&observable_inputs.nco_reference)
        .build();
    println!(
        "Max TOW-history+tracking-sample+rem PR delta: raw={:.6} m centered={:.6} m",
        observables.reference_raw_delta_m, observables.reference_centered_delta_m
    );
    if observables.source == ObservablesSource::TrackingTimestamp {
        assert!(
            observables.reference_centered_delta_m < 1.0,
            "GNSS-SDR timestamp pseudoranges diverged from NCO reference: {:.3} m",
            observables.reference_centered_delta_m
        );
    }
    let pseudoranges = observables.pseudoranges;
    match observables.source {
        ObservablesSource::TrackingTimestamp => {
            println!("Using TOW-history+tracking-sample+rem pseudoranges for WLS")
        }
        ObservablesSource::NcoFallback => println!("Using NCO pseudoranges for WLS fallback"),
    }
    for pr in &pseudoranges {
        let travel_ms = (pr.receive_time_sow - pr.transmit_time_sow) * 1000.0;
        println!(
            "  PRN {:2}: travel={:.3}ms PR={:.0}m",
            pr.prn, travel_ms, pr.pseudorange_m
        );
    }

    if pseudoranges.len() < 4 {
        panic!("Need ≥4 pseudoranges, got {}", pseudoranges.len());
    }

    let sol: PvtSolution = solve_wls(&pseudoranges, &ephemerides, &iono, 0.0, 0.0, 0.0);

    let lat_deg = sol.lla[0].to_degrees();
    let lon_deg = sol.lla[1].to_degrees();
    println!("\n=== PVT Solution ===");
    println!(
        "ECEF: [{:.1}, {:.1}, {:.1}]",
        sol.ecef[0], sol.ecef[1], sol.ecef[2]
    );
    println!(
        "LLA:  lat={:.6}° lon={:.6}° alt={:.1} m",
        lat_deg, lon_deg, sol.lla[2]
    );
    println!("Clock bias: {:.3} µs", sol.clock_bias_s * 1e6);
    println!("n_sats={} converged={}", sol.n_sats, sol.converged);
    assert_eq!(
        sol.satellite_diagnostics.len(),
        sol.used_prns.len(),
        "satellite diagnostics must align with used PRNs"
    );
    println!("\nWLS satellite diagnostics:");
    for diagnostic in &sol.satellite_diagnostics {
        println!(
            "  PRN {:2}: el={:5.1}deg az={:6.1}deg sigma={:5.2}m(cn0={:.2},el={:.2},iono={:.2},jit={:.2}) weight={:.4} resid={:+.3}m std={:+.2} iono={:.3}m trop={:.3}m",
            diagnostic.prn,
            diagnostic.elevation_rad.to_degrees(),
            diagnostic.azimuth_rad.to_degrees(),
            diagnostic.pseudorange_sigma_m,
            diagnostic.cn0_sigma_m,
            diagnostic.elevation_sigma_m,
            diagnostic.iono_sigma_m,
            diagnostic.tracking_jitter_sigma_m,
            diagnostic.weight,
            diagnostic.residual_m,
            diagnostic.standardized_residual,
            diagnostic.iono_delay_m,
            diagnostic.tropo_delay_m
        );
    }

    // Per-satellite residual analysis: compare pseudorange to geometric range
    // (computed from the solved position + ephemeris satellite position).
    println!("\nPer-satellite analysis (PR - geometric_range - clk_bias):");
    let true_ecef = gnss_core::pvt::lla_to_ecef([REF_LAT.to_radians(), REF_LON.to_radians(), 52.0]);
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
        let range_sol = (dx_s * dx_s + dy_s * dy_s + dz_s * dz_s).sqrt();
        // Range from TRUE position
        let dx_t = xs_r - true_ecef[0];
        let dy_t = ys_r - true_ecef[1];
        let dz_t = zs_r - true_ecef[2];
        let range_true = (dx_t * dx_t + dy_t * dy_t + dz_t * dz_t).sqrt();
        let resid_sol =
            pr.pseudorange_m + 299_792_458.0 * sat.clock_correction_s - range_sol - sol_bias_m;
        let resid_true =
            pr.pseudorange_m + 299_792_458.0 * sat.clock_correction_s - range_true - sol_bias_m;
        println!("  PRN {:2}: PR={:.0} range_sol={:.0} range_true={:.0} | resid_sol={:+.0} resid_true={:+.0} (diff={:+.0})",
            pr.prn, pr.pseudorange_m, range_sol, range_true, resid_sol, resid_true, resid_true - resid_sol);
    }
    println!("Residuals (m): {:?}", sol.residuals_m);

    let velocity = gnss_core::pvt::solve_doppler_velocity(&pseudoranges, &ephemerides, sol.ecef);
    let speed_mps = (velocity.ecef_velocity_mps[0].powi(2)
        + velocity.ecef_velocity_mps[1].powi(2)
        + velocity.ecef_velocity_mps[2].powi(2))
    .sqrt();
    println!("\nDoppler velocity solution:");
    println!(
        "  ECEF vel=[{:+.3}, {:+.3}, {:+.3}] m/s ENU vel=[{:+.3}, {:+.3}, {:+.3}] m/s speed={:.3} m/s clk_drift={:+.3} m/s converged={} used={:?}",
        velocity.ecef_velocity_mps[0],
        velocity.ecef_velocity_mps[1],
        velocity.ecef_velocity_mps[2],
        velocity.enu_velocity_mps[0],
        velocity.enu_velocity_mps[1],
        velocity.enu_velocity_mps[2],
        speed_mps,
        velocity.clock_drift_mps,
        velocity.converged,
        velocity.used_prns
    );
    println!("  Doppler residuals (m/s): {:?}", velocity.residuals_mps);
    let velocity_residual_rms_mps = rms(&velocity.residuals_mps);
    assert!(
        velocity.converged,
        "Doppler velocity solution did not converge"
    );
    assert!(
        speed_mps < 50.0,
        "Doppler velocity magnitude is implausible: {:.3} m/s",
        speed_mps
    );
    assert!(
        velocity_residual_rms_mps < 1.0,
        "Doppler velocity residual RMS too large: {:.3} m/s",
        velocity_residual_rms_mps
    );

    let continuous_epochs =
        build_continuous_epochs(&channels, &per_channel_tow, &ephemerides, total_ms, spm);
    assert!(
        continuous_epochs.len() >= 3,
        "need at least 3 continuous epochs, got {}",
        continuous_epochs.len()
    );
    let mut continuous_runner = ContinuousPvtRunner::new(ContinuousPvtConfig {
        hatch: Some(HatchConfig {
            window_epochs: 10,
            max_gap_s: 2.0,
            divergence_reset_threshold_m: 50_000.0,
            ..Default::default()
        }),
        integer_ms_ambiguity: Some(IntegerMsAmbiguityConfig::default()),
        kalman: Some(KalmanConfig {
            initialization_epochs: 2,
            ..Default::default()
        }),
        ..Default::default()
    });
    let continuous_results = continuous_runner.solve_epochs(
        &continuous_epochs,
        &ephemerides,
        &iono,
        sol.lla[0],
        sol.lla[1],
        sol.lla[2],
    );
    let pvt_outputs = PvtEpochOutput::from_results(&continuous_results);
    assert_eq!(pvt_outputs.len(), continuous_results.len());
    assert!(pvt_outputs.iter().any(|output| {
        output.solution_kind == PvtEpochSolutionKind::Kalman
            && output.velocity.is_some()
            && output.time.corrected_gps_time_sow.is_some()
    }));
    let raw_velocity_speeds_mps: Vec<f64> = continuous_results
        .iter()
        .filter_map(|result| result.raw_velocity_solution.as_ref())
        .map(velocity_speed_mps)
        .collect();
    let smoothed_velocity_speeds_mps: Vec<f64> = continuous_results
        .iter()
        .filter_map(|result| result.smoothed_velocity_solution.as_ref())
        .map(velocity_speed_mps)
        .collect();
    let raw_errors_m = position_errors_m(continuous_results.iter().filter_map(|result| {
        result
            .raw_solution
            .converged
            .then_some(&result.raw_solution)
    }));
    let smoothed_errors_m = position_errors_m(continuous_results.iter().filter_map(|result| {
        result
            .smoothed_solution
            .as_ref()
            .filter(|solution| solution.converged)
    }));
    let kalman_errors_m =
        kalman_position_errors_m(continuous_results.iter().filter_map(|result| {
            result
                .kalman_solution
                .as_ref()
                .filter(|solution| solution.converged)
        }));
    let raw_converged_count = continuous_results
        .iter()
        .filter(|result| result.raw_solution.converged)
        .count();
    let smoothed_converged_count = continuous_results
        .iter()
        .filter(|result| {
            result
                .smoothed_solution
                .as_ref()
                .is_some_and(|solution| solution.converged)
        })
        .count();
    let kalman_converged_count = continuous_results
        .iter()
        .filter(|result| {
            result
                .kalman_solution
                .as_ref()
                .is_some_and(|solution| solution.converged)
        })
        .count();
    let raw_failed_epochs: Vec<usize> = continuous_results
        .iter()
        .filter_map(|result| (!result.raw_solution.converged).then_some(result.epoch_index))
        .collect();
    println!("\nContinuous PVT epochs:");
    for result in &continuous_results {
        let smoothed = result.smoothed_solution.as_ref();
        let raw_velocity = result.raw_velocity_solution.as_ref();
        let smoothed_velocity = result.smoothed_velocity_solution.as_ref();
        let kalman = result.kalman_solution.as_ref();
        let (_, centered_delta_m) =
            pseudorange_delta_stats(&pseudoranges, &result.raw_pseudoranges);
        let raw_error_m = if result.raw_solution.converged {
            position_errors_m(std::iter::once(&result.raw_solution))
                .first()
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let kalman_error_m = kalman
            .filter(|solution| solution.converged)
            .map(|solution| kalman_position_errors_m(std::iter::once(solution))[0])
            .unwrap_or(0.0);
        println!(
            "  epoch {:2}: raw converged={} n_sats={} err={:.2}m q={:.2} std_rms={:.2} gdop={:.2} speed={:.2}m/s v_rms={:.3}m/s centered_pr_delta={:.3}m used={:?} smoothed converged={} n_sats={} q={:.2} speed={:.2}m/s v_rms={:.3}m/s kalman converged={} err={:.2}m speed={:.2}m/s pos_sigma={:.2}m vel_sigma={:.3}m/s",
            result.epoch_index,
            result.raw_solution.converged,
            result.raw_solution.n_sats,
            raw_error_m,
            result.raw_solution.epoch_quality_scale,
            result.raw_solution.standardized_residual_rms,
            result.raw_solution.gdop,
            raw_velocity.map(velocity_speed_mps).unwrap_or(0.0),
            raw_velocity
                .map(|velocity| rms(&velocity.residuals_mps))
                .unwrap_or(0.0),
            centered_delta_m,
            result.raw_solution.used_prns,
            smoothed.is_some_and(|solution| solution.converged),
            smoothed.map(|solution| solution.n_sats).unwrap_or(0),
            smoothed
                .map(|solution| solution.epoch_quality_scale)
                .unwrap_or(0.0),
            smoothed_velocity.map(velocity_speed_mps).unwrap_or(0.0),
            smoothed_velocity
                .map(|velocity| rms(&velocity.residuals_mps))
                .unwrap_or(0.0),
            kalman.is_some_and(|solution| solution.converged),
            kalman_error_m,
            kalman.map(kalman_speed_mps).unwrap_or(0.0),
            kalman
                .map(|solution| solution.position_measurement_sigma_m)
                .unwrap_or(0.0),
            kalman
                .map(|solution| solution.velocity_measurement_sigma_mps)
                .unwrap_or(0.0)
        );
    }
    assert!(
        raw_errors_m.len() >= 3,
        "need at least 3 converged raw continuous epochs, got {}",
        raw_errors_m.len()
    );
    assert!(
        smoothed_errors_m.len() >= 3,
        "need at least 3 converged smoothed continuous epochs, got {}",
        smoothed_errors_m.len()
    );
    let raw_rms_m = rms(&raw_errors_m);
    let smoothed_rms_m = rms(&smoothed_errors_m);
    let kalman_rms_m = rms(&kalman_errors_m);
    let raw_velocity_max_mps = raw_velocity_speeds_mps
        .iter()
        .copied()
        .fold(0.0_f64, f64::max);
    let smoothed_velocity_max_mps = smoothed_velocity_speeds_mps
        .iter()
        .copied()
        .fold(0.0_f64, f64::max);
    println!(
        "\nContinuous PVT: epochs={} raw_ok={} smoothed_ok={} kalman_ok={} raw_rms={:.2}m smoothed_rms={:.2}m kalman_rms={:.2}m raw_max={:.2}m smoothed_max={:.2}m kalman_max={:.2}m raw_vmax={:.2}m/s smoothed_vmax={:.2}m/s",
        continuous_results.len(),
        raw_converged_count,
        smoothed_converged_count,
        kalman_converged_count,
        raw_rms_m,
        smoothed_rms_m,
        kalman_rms_m,
        raw_errors_m.iter().copied().fold(0.0_f64, f64::max),
        smoothed_errors_m.iter().copied().fold(0.0_f64, f64::max),
        kalman_errors_m.iter().copied().fold(0.0_f64, f64::max),
        raw_velocity_max_mps,
        smoothed_velocity_max_mps
    );
    assert!(
        raw_failed_epochs.is_empty(),
        "continuous raw PVT failed at epochs {:?}",
        raw_failed_epochs
    );
    assert!(
        smoothed_rms_m < raw_rms_m * 10.0 + 1.0,
        "carrier smoothing diverged: raw_rms={:.3}m smoothed_rms={:.3}m",
        raw_rms_m,
        smoothed_rms_m
    );
    assert!(
        raw_rms_m < 250.0,
        "continuous raw PVT RMS too large: {:.3}m",
        raw_rms_m
    );
    assert!(
        smoothed_rms_m < 500.0,
        "continuous smoothed PVT RMS too large: {:.3}m",
        smoothed_rms_m
    );
    assert!(
        kalman_errors_m.len() >= 3,
        "need at least 3 converged Kalman continuous epochs, got {}",
        kalman_errors_m.len()
    );
    assert!(
        kalman_converged_count + 1 >= continuous_results.len(),
        "Kalman should only miss the warm-up epoch: kalman_ok={} epochs={}",
        kalman_converged_count,
        continuous_results.len()
    );
    assert!(
        kalman_rms_m < 500.0,
        "continuous Kalman PVT RMS too large: {:.3}m",
        kalman_rms_m
    );
    assert!(
        kalman_rms_m < raw_rms_m,
        "continuous Kalman should improve raw RMS: raw_rms={:.3}m kalman_rms={:.3}m",
        raw_rms_m,
        kalman_rms_m
    );
    assert!(
        raw_velocity_speeds_mps.len() >= 3,
        "need at least 3 raw velocity epochs, got {}",
        raw_velocity_speeds_mps.len()
    );
    assert!(
        smoothed_velocity_speeds_mps.len() >= 3,
        "need at least 3 smoothed velocity epochs, got {}",
        smoothed_velocity_speeds_mps.len()
    );
    assert!(
        raw_velocity_max_mps < 50.0,
        "continuous raw velocity too large: {:.3}m/s",
        raw_velocity_max_mps
    );
    assert!(
        smoothed_velocity_max_mps < 50.0,
        "continuous smoothed velocity too large: {:.3}m/s",
        smoothed_velocity_max_mps
    );
    println!("Reference: lat={:.4}° lon={:.4}°", REF_LAT, REF_LON);

    assert!(sol.converged, "WLS did not converge");
    assert!(
        sol.n_sats >= 4,
        "need ≥4 sats in solution, got {}",
        sol.n_sats
    );
    // Position accuracy: tolerate ~10 km for this rough first PVT (clock bias
    // alignment + pseudorange formation are approximate). Tighten later.
    let lat_err = (lat_deg - REF_LAT).abs();
    let lon_err = (lon_deg - REF_LON).abs();
    println!("Error: lat={:.4}° lon={:.4}°", lat_err, lon_err);
    // Tight thresholds for pseudorange-quality feedback loop.
    // With correct sub-ms pseudorange, both should be < 0.005° (~500 m).
    // Current bug: code_phase_chips is NCO accumulator, not signal code phase
    // → lon error typically ~0.05° = 5.5 km. This assert goes RED on the bug.
    assert!(lat_err < 0.005, "latitude error too large: {:.4}°", lat_err);
    assert!(
        lon_err < 0.005,
        "longitude error too large: {:.4}°",
        lon_err
    );
}

fn build_continuous_epochs(
    channels: &[Channel],
    per_channel_tow: &BTreeMap<u8, Vec<TowSample>>,
    ephemerides: &EphemerisMap,
    total_ms: usize,
    spm: usize,
) -> Vec<PseudorangeEpoch> {
    (500..total_ms)
        .step_by(1_000)
        .filter_map(|epoch_ms| {
            let cp_idx = epoch_ms.saturating_sub(1);
            let rx_sample = channels
                .iter()
                .filter_map(|ch| ch.sample_counter.get(cp_idx).copied())
                .max()
                .unwrap_or((epoch_ms * spm) as u64);
            let observations: Vec<TrackingTimestampObservation<'_>> = channels
                .iter()
                .filter_map(|ch| {
                    if !ephemerides.contains_key(&ch.prn) {
                        return None;
                    }
                    let tow_history = per_channel_tow.get(&ch.prn)?;
                    if tow_history.len() < 2 {
                        return None;
                    }
                    let tracking_rx_sample = *ch.tracking_sample_counter.get(cp_idx)? as f64
                        + *ch.rem_code_phase_samples.get(cp_idx)?;
                    let first_tow_sample = tow_history.first()?.rx_sample;
                    let last_tow_sample = tow_history.last()?.rx_sample;
                    if tracking_rx_sample < first_tow_sample || tracking_rx_sample > last_tow_sample
                    {
                        return None;
                    }
                    Some(TrackingTimestampObservation {
                        prn: ch.prn,
                        tow_history,
                        tracking_sample_counter: *ch.tracking_sample_counter.get(cp_idx)?,
                        rem_code_phase_samples: *ch.rem_code_phase_samples.get(cp_idx)?,
                        cn0_db_hz: *ch.cn0_history_db_hz.get(cp_idx)?,
                        doppler_hz: *ch.doppler_history_hz.get(cp_idx)?,
                        tracking_jitter_chips: *ch.dll_error_history_chips.get(cp_idx)?,
                    })
                })
                .collect();
            let pseudoranges = from_tracking_timestamp_common_receive_epoch(
                &observations,
                rx_sample as f64,
                FS,
                0.075,
            );
            (pseudoranges.len() >= 4).then_some(PseudorangeEpoch { pseudoranges })
        })
        .collect()
}

fn position_errors_m<'a>(solutions: impl Iterator<Item = &'a PvtSolution>) -> Vec<f64> {
    let ref_ecef = gnss_core::pvt::lla_to_ecef([REF_LAT.to_radians(), REF_LON.to_radians(), 52.0]);
    solutions
        .map(|solution| {
            let dx = solution.ecef[0] - ref_ecef[0];
            let dy = solution.ecef[1] - ref_ecef[1];
            let dz = solution.ecef[2] - ref_ecef[2];
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .collect()
}

fn kalman_position_errors_m<'a>(
    solutions: impl Iterator<Item = &'a gnss_core::pvt::KalmanSolution>,
) -> Vec<f64> {
    let ref_ecef = gnss_core::pvt::lla_to_ecef([REF_LAT.to_radians(), REF_LON.to_radians(), 52.0]);
    solutions
        .map(|solution| {
            let dx = solution.ecef[0] - ref_ecef[0];
            let dy = solution.ecef[1] - ref_ecef[1];
            let dz = solution.ecef[2] - ref_ecef[2];
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .collect()
}

fn velocity_speed_mps(solution: &gnss_core::pvt::VelocitySolution) -> f64 {
    (solution.ecef_velocity_mps[0].powi(2)
        + solution.ecef_velocity_mps[1].powi(2)
        + solution.ecef_velocity_mps[2].powi(2))
    .sqrt()
}

fn kalman_speed_mps(solution: &gnss_core::pvt::KalmanSolution) -> f64 {
    (solution.ecef_velocity_mps[0].powi(2)
        + solution.ecef_velocity_mps[1].powi(2)
        + solution.ecef_velocity_mps[2].powi(2))
    .sqrt()
}

fn spawn_tracking_worker(mut channel: Channel) -> TrackingWorker {
    let (command_tx, command_rx) = sync_channel::<TrackingWorkerCommand>(1);
    let (report_tx, report_rx) = sync_channel::<TrackingStepReport>(1);
    let join = thread::spawn(move || {
        while let Ok(command) = command_rx.recv() {
            match command {
                TrackingWorkerCommand::Process { ms, chunk } => {
                    let report = process_tracking_ms(&mut channel, &chunk, ms);
                    report_tx
                        .send(report)
                        .expect("main thread should receive tracking reports");
                }
                TrackingWorkerCommand::Finish => break,
            }
        }
        channel
    });

    TrackingWorker {
        command_tx,
        report_rx,
        join,
    }
}

fn process_tracking_ms(
    channel: &mut Channel,
    chunk: &[Complex<f64>],
    ms: usize,
) -> TrackingStepReport {
    let out = channel.trk.process(chunk);
    channel.cn0_db_hz = out.cn0_db_hz;
    channel.doppler_hz = out.carrier_doppler_hz;
    let prompt_i = out.prompt.re;
    channel.prompt_i.push(prompt_i);
    channel.code_phase.push(out.code_phase_chips);
    channel.sample_counter.push(out.sample_counter);
    channel
        .tracking_sample_counter
        .push(out.tracking_sample_counter);
    channel
        .rem_code_phase_samples
        .push(out.rem_code_phase_samples);
    channel.dll_error_history_chips.push(out.dll_error.abs());
    channel.cn0_history_db_hz.push(out.cn0_db_hz);
    channel.doppler_history_hz.push(out.carrier_doppler_hz);

    let mut bit_sync_boundary = None;
    if channel.boundary.is_none() && ms >= channel.sync_start_ms {
        if let Some(boundary) = channel.bit_sync.update(prompt_i) {
            channel.boundary = Some(boundary);
            channel
                .trk
                .set_bit_boundary_from_sync(boundary, channel.sync_start_ms);
            bit_sync_boundary = Some(boundary);
        }
    }

    TrackingStepReport {
        prn: channel.prn,
        ms,
        doppler_hz: channel.doppler_hz,
        cn0_db_hz: channel.cn0_db_hz,
        prompt_i_abs: prompt_i.abs(),
        bit_sync_boundary,
    }
}

fn read_iq_window_ms(
    path: &Path,
    format: IqFormat,
    window_ms: usize,
    samples_per_ms: usize,
) -> Vec<Complex<f64>> {
    let mut reader = IqReader::open(path, format).expect("open IQ file");
    let mut buffer = vec![Complex::new(0.0, 0.0); window_ms * samples_per_ms];
    let count = buffer.len();
    read_iq_exact(&mut reader, &mut buffer, count);
    buffer
}

fn read_iq_exact(reader: &mut IqReader, buffer: &mut [Complex<f64>], count: usize) {
    let mut total_read = 0;
    while total_read < count {
        let read = reader
            .read(&mut buffer[total_read..count], count - total_read)
            .expect("read IQ samples");
        if read == 0 {
            panic!(
                "unexpected end of IQ stream: requested {} samples, got {}",
                count, total_read
            );
        }
        total_read += read;
    }
}

fn ms_slice(samples: &[Complex<f64>], ms: usize, samples_per_ms: usize) -> &[Complex<f64>] {
    let start = ms * samples_per_ms;
    let end = start + samples_per_ms;
    &samples[start..end]
}

fn rms(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    (values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt()
}
