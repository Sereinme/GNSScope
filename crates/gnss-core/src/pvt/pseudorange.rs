//! Pseudorange formation from tracking observables and decoded GPS TOW.
//!
//! Absolute pseudorange: `P = c * (T_rx - T_tx)`. The GPS HOW TOW count is in
//! 6-second units and tags the start of the next subframe, which is also the
//! end epoch of the decoded subframe.

const C_LIGHT: f64 = 299_792_458.0;
/// GPS C/A code period: 1 ms.
const CODE_PERIOD_S: f64 = 1e-3;
/// GPS L1 C/A chips per code period.
const CHIPS_PER_CODE: f64 = 1023.0;
const GPS_L1_CA_CODE_RATE: f64 = 1.023e6;

/// One satellite's pseudorange observation at a given receive epoch.
#[derive(Debug, Clone, Copy)]
pub struct Pseudorange {
    pub prn: u8,
    /// Pseudorange [m].
    pub pseudorange_m: f64,
    /// GPS transmit time (seconds of week) of the received signal.
    pub transmit_time_sow: f64,
    /// Receiver time (seconds of week) of the receive epoch.
    pub receive_time_sow: f64,
    /// Carrier-to-noise density from tracking [dB-Hz], used as WLS weight.
    pub cn0_db_hz: f64,
    /// Carrier Doppler [Hz] (for future velocity solution).
    pub doppler_hz: f64,
    /// Absolute DLL discriminator/code tracking error estimate [chips].
    pub tracking_jitter_chips: f64,
}

/// Tracking-derived observation for one satellite at a common receiver epoch.
#[derive(Debug, Clone, Copy)]
pub struct TrackingObservation {
    pub prn: u8,
    /// Receiver sample index at the decoded HOW epoch. HOW TOW marks the
    /// first symbol of the next subframe, i.e. the end of the decoded subframe.
    pub how_epoch_rx_sample: f64,
    /// HOW TOW count in 6-second units.
    pub tow_count: u32,
    /// Code NCO phase in chips at the common receive epoch.
    pub code_phase_chips: f64,
    /// Per-channel receive-time offset relative to `common_rx_sample`.
    ///
    /// Keep this at 0.0 when `code_phase_chips` is already used as the full
    /// sub-ms signal phase. GNSS-SDR-style observables can use this for
    /// `Code_phase_samples` (< 1 sample) once the code phase is not separately
    /// counted in transmit time.
    pub rx_sample_offset_samples: f64,
    pub cn0_db_hz: f64,
    pub doppler_hz: f64,
    pub tracking_jitter_chips: f64,
}

/// Decoded telemetry time tag at a receiver sample epoch.
#[derive(Debug, Clone, Copy)]
pub struct TowSample {
    pub rx_sample: f64,
    pub tow_sow: f64,
}

/// GNSS-SDR-style observation backed by telemetry TOW history.
#[derive(Debug, Clone, Copy)]
pub struct TowHistoryObservation<'a> {
    pub prn: u8,
    pub tow_history: &'a [TowSample],
    pub rx_sample_offset_samples: f64,
    pub cn0_db_hz: f64,
    pub doppler_hz: f64,
    pub tracking_jitter_chips: f64,
}

/// GNSS-SDR-style tracking timestamp observation backed by telemetry TOW history.
#[derive(Debug, Clone, Copy)]
pub struct TrackingTimestampObservation<'a> {
    pub prn: u8,
    pub tow_history: &'a [TowSample],
    /// Code-aligned integer receiver sample tag from tracking.
    pub tracking_sample_counter: u64,
    /// Sub-sample residual paired with `tracking_sample_counter`.
    pub rem_code_phase_samples: f64,
    pub cn0_db_hz: f64,
    pub doppler_hz: f64,
    pub tracking_jitter_chips: f64,
}

/// Per-channel tracking snapshot used to form a consistent Observables input set.
#[derive(Debug, Clone, Copy)]
pub struct TrackingChannelSnapshot<'a> {
    pub prn: u8,
    pub tow_count: u32,
    pub how_epoch_rx_sample: f64,
    pub code_phase_chips: f64,
    pub tracking_sample_counter: u64,
    pub rem_code_phase_samples: f64,
    pub tow_history: &'a [TowSample],
    pub cn0_db_hz: f64,
    pub doppler_hz: f64,
    pub tracking_jitter_chips: f64,
}

#[derive(Debug, Clone)]
pub struct ObservablesInputSet<'a> {
    pub nco_reference: Vec<TrackingObservation>,
    pub quantized_reference: Vec<TrackingObservation>,
    pub rx_rem_reference: Vec<TrackingObservation>,
    pub tow_history_code_phase: Vec<TowHistoryObservation<'a>>,
    pub tracking_timestamp: Vec<TrackingTimestampObservation<'a>>,
    pub tow_history_delta_min_ms: f64,
    pub tow_history_delta_max_ms: f64,
}

pub struct ObservablesInputBuilder<'a, 'b> {
    common_rx_sample: f64,
    fs: f64,
    channel_snapshots: &'b [TrackingChannelSnapshot<'a>],
}

impl<'a, 'b> ObservablesInputBuilder<'a, 'b> {
    pub fn new(common_rx_sample: f64, fs: f64) -> Self {
        Self {
            common_rx_sample,
            fs,
            channel_snapshots: &[],
        }
    }

    pub fn with_channel_snapshots(mut self, snapshots: &'b [TrackingChannelSnapshot<'a>]) -> Self {
        self.channel_snapshots = snapshots;
        self
    }

    pub fn build(self) -> ObservablesInputSet<'a> {
        let mut nco_reference = Vec::new();
        let mut quantized_reference = Vec::new();
        let mut rx_rem_reference = Vec::new();
        let mut tow_history_code_phase = Vec::new();
        let mut tracking_timestamp = Vec::new();
        let mut tow_history_delta_min_ms = f64::INFINITY;
        let mut tow_history_delta_max_ms = f64::NEG_INFINITY;

        if self.fs <= 0.0 {
            return ObservablesInputSet {
                nco_reference,
                quantized_reference,
                rx_rem_reference,
                tow_history_code_phase,
                tracking_timestamp,
                tow_history_delta_min_ms,
                tow_history_delta_max_ms,
            };
        }

        for snapshot in self.channel_snapshots {
            if snapshot.tow_count == 0 || self.common_rx_sample <= snapshot.how_epoch_rx_sample {
                continue;
            }

            if let Some(interp_tow_sow) =
                interpolate_tow_sow(snapshot.tow_history, self.common_rx_sample)
            {
                let how_linear_tow_sow = snapshot.tow_count as f64 * 6.0
                    + (self.common_rx_sample - snapshot.how_epoch_rx_sample) / self.fs;
                let delta_ms = (interp_tow_sow - how_linear_tow_sow) * 1000.0;
                tow_history_delta_min_ms = tow_history_delta_min_ms.min(delta_ms);
                tow_history_delta_max_ms = tow_history_delta_max_ms.max(delta_ms);
            }

            nco_reference.push(TrackingObservation {
                prn: snapshot.prn,
                how_epoch_rx_sample: snapshot.how_epoch_rx_sample,
                tow_count: snapshot.tow_count,
                code_phase_chips: snapshot.code_phase_chips,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: snapshot.cn0_db_hz,
                doppler_hz: snapshot.doppler_hz,
                tracking_jitter_chips: snapshot.tracking_jitter_chips,
            });

            let quantized_code_phase_chips =
                code_phase_samples_from_chips(snapshot.code_phase_chips, self.fs).floor()
                    * GPS_L1_CA_CODE_RATE
                    / self.fs;
            quantized_reference.push(TrackingObservation {
                prn: snapshot.prn,
                how_epoch_rx_sample: snapshot.how_epoch_rx_sample,
                tow_count: snapshot.tow_count,
                code_phase_chips: quantized_code_phase_chips,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: snapshot.cn0_db_hz,
                doppler_hz: snapshot.doppler_hz,
                tracking_jitter_chips: snapshot.tracking_jitter_chips,
            });

            rx_rem_reference.push(TrackingObservation {
                prn: snapshot.prn,
                how_epoch_rx_sample: snapshot.how_epoch_rx_sample,
                tow_count: snapshot.tow_count,
                code_phase_chips: snapshot.code_phase_chips,
                rx_sample_offset_samples: snapshot.rem_code_phase_samples,
                cn0_db_hz: snapshot.cn0_db_hz,
                doppler_hz: snapshot.doppler_hz,
                tracking_jitter_chips: snapshot.tracking_jitter_chips,
            });

            if snapshot.tow_history.len() >= 2 {
                tow_history_code_phase.push(TowHistoryObservation {
                    prn: snapshot.prn,
                    tow_history: snapshot.tow_history,
                    rx_sample_offset_samples: code_phase_samples_from_chips(
                        snapshot.code_phase_chips,
                        self.fs,
                    ),
                    cn0_db_hz: snapshot.cn0_db_hz,
                    doppler_hz: snapshot.doppler_hz,
                    tracking_jitter_chips: snapshot.tracking_jitter_chips,
                });
                tracking_timestamp.push(TrackingTimestampObservation {
                    prn: snapshot.prn,
                    tow_history: snapshot.tow_history,
                    tracking_sample_counter: snapshot.tracking_sample_counter,
                    rem_code_phase_samples: snapshot.rem_code_phase_samples,
                    cn0_db_hz: snapshot.cn0_db_hz,
                    doppler_hz: snapshot.doppler_hz,
                    tracking_jitter_chips: snapshot.tracking_jitter_chips,
                });
            }
        }

        ObservablesInputSet {
            nco_reference,
            quantized_reference,
            rx_rem_reference,
            tow_history_code_phase,
            tracking_timestamp,
            tow_history_delta_min_ms,
            tow_history_delta_max_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservablesSource {
    TrackingTimestamp,
    NcoFallback,
}

#[derive(Debug, Clone)]
pub struct ObservablesResult {
    pub source: ObservablesSource,
    pub pseudoranges: Vec<Pseudorange>,
    pub reference_pseudoranges: Vec<Pseudorange>,
    pub reference_raw_delta_m: f64,
    pub reference_centered_delta_m: f64,
}

pub struct ObservablesBuilder<'a> {
    common_rx_sample: f64,
    fs: f64,
    nominal_anchor_travel_time_s: f64,
    tracking_timestamp_observations: &'a [TrackingTimestampObservation<'a>],
    nco_reference_observations: &'a [TrackingObservation],
}

impl<'a> ObservablesBuilder<'a> {
    pub fn new(common_rx_sample: f64, fs: f64) -> Self {
        Self {
            common_rx_sample,
            fs,
            nominal_anchor_travel_time_s: 0.075,
            tracking_timestamp_observations: &[],
            nco_reference_observations: &[],
        }
    }

    pub fn with_nominal_anchor_travel_time_s(mut self, seconds: f64) -> Self {
        self.nominal_anchor_travel_time_s = seconds;
        self
    }

    pub fn with_tracking_timestamp_observations(
        mut self,
        observations: &'a [TrackingTimestampObservation<'a>],
    ) -> Self {
        self.tracking_timestamp_observations = observations;
        self
    }

    pub fn with_nco_reference_observations(
        mut self,
        observations: &'a [TrackingObservation],
    ) -> Self {
        self.nco_reference_observations = observations;
        self
    }

    pub fn build(self) -> ObservablesResult {
        let tracking_pseudoranges = from_tracking_timestamp_common_receive_epoch(
            self.tracking_timestamp_observations,
            self.common_rx_sample,
            self.fs,
            self.nominal_anchor_travel_time_s,
        );
        let reference_pseudoranges = from_common_receive_epoch(
            self.nco_reference_observations,
            self.common_rx_sample,
            self.fs,
            self.nominal_anchor_travel_time_s,
        );
        let (reference_raw_delta_m, reference_centered_delta_m) =
            pseudorange_delta_stats(&reference_pseudoranges, &tracking_pseudoranges);

        if tracking_pseudoranges.len() >= 4 {
            ObservablesResult {
                source: ObservablesSource::TrackingTimestamp,
                pseudoranges: tracking_pseudoranges,
                reference_pseudoranges,
                reference_raw_delta_m,
                reference_centered_delta_m,
            }
        } else {
            ObservablesResult {
                source: ObservablesSource::NcoFallback,
                pseudoranges: reference_pseudoranges.clone(),
                reference_pseudoranges,
                reference_raw_delta_m,
                reference_centered_delta_m,
            }
        }
    }
}

impl Pseudorange {
    /// Build a pseudorange from per-channel tracking state.
    ///
    /// - `tow_count`: HOW TOW count in 6-second units. This is the first-bit
    ///   epoch of the next subframe, not the start of the decoded subframe.
    /// - `ms_since_how_epoch`: milliseconds elapsed since that HOW TOW epoch.
    /// - `rx_sample_index`: absolute sample counter at the receive epoch.
    /// - `fs`: sampling frequency [Hz].
    /// - `cn0_db_hz`, `doppler_hz`: from tracking output.
    pub fn from_tracking(
        prn: u8,
        tow_count: u32,
        ms_since_how_epoch: u64,
        rx_sample_index: u64,
        fs: f64,
        cn0_db_hz: f64,
        doppler_hz: f64,
    ) -> Self {
        Self::from_tracking_with_quality(
            prn,
            tow_count,
            ms_since_how_epoch,
            rx_sample_index,
            fs,
            cn0_db_hz,
            doppler_hz,
            0.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_tracking_with_quality(
        prn: u8,
        tow_count: u32,
        ms_since_how_epoch: u64,
        rx_sample_index: u64,
        fs: f64,
        cn0_db_hz: f64,
        doppler_hz: f64,
        tracking_jitter_chips: f64,
    ) -> Self {
        let how_epoch_sow = tow_count as f64 * 6.0;
        let transmit_time_sow = how_epoch_sow + ms_since_how_epoch as f64 * 1e-3;
        let receive_time_sow = rx_sample_index as f64 / fs;
        let pseudorange_m = (receive_time_sow - transmit_time_sow) * C_LIGHT;

        Pseudorange {
            prn,
            pseudorange_m,
            transmit_time_sow,
            receive_time_sow,
            cn0_db_hz,
            doppler_hz,
            tracking_jitter_chips,
        }
    }
}

/// Interpolate or lightly extrapolate decoded TOW to a receiver sample epoch.
///
/// `history` must be ordered by `rx_sample`. TOW is unwrapped across the GPS
/// week boundary for the local interpolation interval, then wrapped back to SOW.
pub fn interpolate_tow_sow(history: &[TowSample], rx_sample: f64) -> Option<f64> {
    if history.is_empty() {
        return None;
    }
    if history.len() == 1 {
        return Some(wrap_sow(history[0].tow_sow));
    }

    let upper_idx = history
        .iter()
        .position(|sample| sample.rx_sample >= rx_sample)
        .unwrap_or(history.len() - 1);
    let lower_idx = if upper_idx == 0 { 0 } else { upper_idx - 1 };
    let upper_idx = if lower_idx == upper_idx { 1 } else { upper_idx };

    let a = history[lower_idx];
    let b = history[upper_idx];
    let dt_sample = b.rx_sample - a.rx_sample;
    if dt_sample.abs() < f64::EPSILON {
        return Some(wrap_sow(a.tow_sow));
    }

    let b_tow = unwrap_sow_near(b.tow_sow, a.tow_sow);
    let frac = (rx_sample - a.rx_sample) / dt_sample;
    Some(wrap_sow(a.tow_sow + frac * (b_tow - a.tow_sow)))
}

fn unwrap_sow_near(tow_sow: f64, reference_sow: f64) -> f64 {
    const WEEK_S: f64 = 604_800.0;
    let mut tow = tow_sow;
    while tow - reference_sow > WEEK_S / 2.0 {
        tow -= WEEK_S;
    }
    while reference_sow - tow > WEEK_S / 2.0 {
        tow += WEEK_S;
    }
    tow
}

fn wrap_sow(tow_sow: f64) -> f64 {
    const WEEK_S: f64 = 604_800.0;
    tow_sow.rem_euclid(WEEK_S)
}

/// Form pseudoranges at a common receiver epoch from per-channel tracking
/// observations.
///
/// `nominal_anchor_travel_time_s` sets the absolute receiver clock epoch by
/// anchoring the earliest decoded HOW epoch. It only affects the common receiver
/// clock bias; inter-satellite pseudorange differences come from each channel's
/// HOW epoch and code phase.
pub fn from_common_receive_epoch(
    observations: &[TrackingObservation],
    common_rx_sample: f64,
    fs: f64,
    nominal_anchor_travel_time_s: f64,
) -> Vec<Pseudorange> {
    if fs <= 0.0 {
        return Vec::new();
    }

    let Some(anchor) = observations
        .iter()
        .min_by(|a, b| a.how_epoch_rx_sample.total_cmp(&b.how_epoch_rx_sample))
    else {
        return Vec::new();
    };

    let anchor_how_sow = anchor.tow_count as f64 * 6.0;
    let receive_time_sow = anchor_how_sow
        + nominal_anchor_travel_time_s
        + (common_rx_sample - anchor.how_epoch_rx_sample) / fs;

    observations
        .iter()
        .filter_map(|obs| {
            if common_rx_sample <= obs.how_epoch_rx_sample || obs.tow_count == 0 {
                return None;
            }
            let obs_rx_sample = common_rx_sample + obs.rx_sample_offset_samples;
            let seconds_since_how = (obs_rx_sample - obs.how_epoch_rx_sample) / fs;
            let code_phase_s = sub_ms_offset(obs.code_phase_chips);
            let transmit_time_sow = obs.tow_count as f64 * 6.0 + seconds_since_how + code_phase_s;
            let pseudorange_m = (receive_time_sow - transmit_time_sow) * C_LIGHT;
            Some(Pseudorange {
                prn: obs.prn,
                pseudorange_m,
                transmit_time_sow,
                receive_time_sow,
                cn0_db_hz: obs.cn0_db_hz,
                doppler_hz: obs.doppler_hz,
                tracking_jitter_chips: obs.tracking_jitter_chips,
            })
        })
        .collect()
}

/// Form pseudoranges from TOW history at a common receiver sample.
///
/// This mirrors the common-reception-time idea used by GNSS-SDR: each channel's
/// transmit time is interpolated from telemetry TOW history at its receiver
/// timestamp. The absolute receiver clock is anchored with
/// `nominal_anchor_travel_time_s`; WLS absorbs that common bias.
pub fn from_tow_history_common_receive_epoch(
    observations: &[TowHistoryObservation<'_>],
    common_rx_sample: f64,
    fs: f64,
    nominal_anchor_travel_time_s: f64,
) -> Vec<Pseudorange> {
    if fs <= 0.0 {
        return Vec::new();
    }

    let mut tagged: Vec<(TowHistoryObservation<'_>, f64)> = observations
        .iter()
        .filter_map(|obs| {
            let rx_sample = common_rx_sample + obs.rx_sample_offset_samples;
            interpolate_tow_sow(obs.tow_history, rx_sample).map(|tow_sow| (*obs, tow_sow))
        })
        .collect();
    if tagged.is_empty() {
        return Vec::new();
    }

    tagged.sort_by(|a, b| b.1.total_cmp(&a.1));
    let anchor_tow_sow = tagged[0].1;
    let receive_time_sow = wrap_sow(anchor_tow_sow + nominal_anchor_travel_time_s);

    tagged
        .into_iter()
        .map(|(obs, tx_sow)| {
            let tx_sow = unwrap_sow_near(tx_sow, receive_time_sow);
            let pseudorange_m = (receive_time_sow - tx_sow) * C_LIGHT;
            Pseudorange {
                prn: obs.prn,
                pseudorange_m,
                transmit_time_sow: wrap_sow(tx_sow),
                receive_time_sow,
                cn0_db_hz: obs.cn0_db_hz,
                doppler_hz: obs.doppler_hz,
                tracking_jitter_chips: obs.tracking_jitter_chips,
            }
        })
        .collect()
}

/// Form pseudoranges from GNSS-SDR-style tracking timestamps.
pub fn from_tracking_timestamp_common_receive_epoch(
    observations: &[TrackingTimestampObservation<'_>],
    common_rx_sample: f64,
    fs: f64,
    nominal_anchor_travel_time_s: f64,
) -> Vec<Pseudorange> {
    let tow_observations: Vec<TowHistoryObservation<'_>> = observations
        .iter()
        .map(|obs| TowHistoryObservation {
            prn: obs.prn,
            tow_history: obs.tow_history,
            rx_sample_offset_samples: obs.tracking_sample_counter as f64
                + obs.rem_code_phase_samples
                - common_rx_sample,
            cn0_db_hz: obs.cn0_db_hz,
            doppler_hz: obs.doppler_hz,
            tracking_jitter_chips: obs.tracking_jitter_chips,
        })
        .collect();

    from_tow_history_common_receive_epoch(
        &tow_observations,
        common_rx_sample,
        fs,
        nominal_anchor_travel_time_s,
    )
}

/// Code-phase in chips at a given sample within the 1 ms code period.
pub fn code_phase_chips(code_phase_chips: f64) -> f64 {
    code_phase_chips.rem_euclid(CHIPS_PER_CODE)
}

/// Convert a GPS L1 C/A code phase in chips into samples within one code period.
pub fn code_phase_samples_from_chips(chips: f64, fs: f64) -> f64 {
    code_phase_chips(chips) * fs / GPS_L1_CA_CODE_RATE
}

pub fn pseudorange_delta_stats(base: &[Pseudorange], candidate: &[Pseudorange]) -> (f64, f64) {
    let deltas: Vec<f64> = base
        .iter()
        .filter_map(|base_pr| {
            candidate
                .iter()
                .find(|candidate_pr| candidate_pr.prn == base_pr.prn)
                .map(|candidate_pr| candidate_pr.pseudorange_m - base_pr.pseudorange_m)
        })
        .collect();
    if deltas.is_empty() {
        return (0.0, 0.0);
    }

    let raw = deltas
        .iter()
        .map(|delta| delta.abs())
        .fold(0.0_f64, f64::max);
    let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let centered = deltas
        .iter()
        .map(|delta| (delta - mean).abs())
        .fold(0.0_f64, f64::max);
    (raw, centered)
}

/// Resolve GPS C/A 1 ms code-period ambiguities using predicted pseudoranges.
///
/// `predicted_pseudoranges_m` is the geometry/satellite-clock prediction for
/// each PRN, up to a common receiver clock bias. The resolver only applies
/// integer multiples of one C/A code period, preserving sub-ms measurements.
pub fn resolve_integer_ms_ambiguities(
    pseudoranges: &[Pseudorange],
    predicted_pseudoranges_m: &[(u8, f64)],
    max_shift_ms: i32,
) -> Vec<Pseudorange> {
    if pseudoranges.is_empty() || predicted_pseudoranges_m.is_empty() || max_shift_ms <= 0 {
        return pseudoranges.to_vec();
    }

    let residuals: Vec<f64> = pseudoranges
        .iter()
        .filter_map(|pr| {
            predicted_pseudoranges_m
                .iter()
                .find(|(prn, _)| *prn == pr.prn)
                .map(|(_, predicted)| pr.pseudorange_m - predicted)
        })
        .collect();
    let Some(common_bias_m) = median(&residuals) else {
        return pseudoranges.to_vec();
    };

    let code_period_m = C_LIGHT * CODE_PERIOD_S;
    pseudoranges
        .iter()
        .map(|pr| {
            let Some((_, predicted)) = predicted_pseudoranges_m
                .iter()
                .find(|(prn, _)| *prn == pr.prn)
            else {
                return *pr;
            };
            let residual = pr.pseudorange_m - predicted;
            let shift_ms = ((common_bias_m - residual) / code_period_m).round() as i32;
            let shift_ms = shift_ms.clamp(-max_shift_ms, max_shift_ms);
            let mut adjusted = *pr;
            adjusted.pseudorange_m += shift_ms as f64 * code_period_m;
            adjusted
        })
        .collect()
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        Some(0.5 * (sorted[mid - 1] + sorted[mid]))
    } else {
        Some(sorted[mid])
    }
}

/// Fractional ms offset within a code period, used for sub-ms transmit-time
/// refinement.
pub fn sub_ms_offset(code_phase_chips: f64) -> f64 {
    (code_phase_chips % CHIPS_PER_CODE) / CHIPS_PER_CODE * CODE_PERIOD_S
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pseudorange_sign() {
        // Receiver time later than transmit time gives positive pseudorange.
        // tow_count=10 => HOW epoch SOW = 60 s; +1000 ms => T_tx = 61 s.
        // rx_sample_index at fs=4e6, 62 s => 248_000_000.
        let pr = Pseudorange::from_tracking(1, 10, 1000, 248_000_000, 4e6, 45.0, 1000.0);
        assert!((pr.transmit_time_sow - 61.0).abs() < 1e-9);
        assert!((pr.receive_time_sow - 62.0).abs() < 1e-9);
        assert!((pr.pseudorange_m - C_LIGHT).abs() < 1.0);
    }

    #[test]
    fn test_common_receive_epoch_matches_manual_formula() {
        let observations = [
            TrackingObservation {
                prn: 1,
                how_epoch_rx_sample: 40_000_000.0,
                tow_count: 100,
                code_phase_chips: 255.75,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: 45.0,
                doppler_hz: 1200.0,
                tracking_jitter_chips: 0.0,
            },
            TrackingObservation {
                prn: 2,
                how_epoch_rx_sample: 40_012_000.0,
                tow_count: 100,
                code_phase_chips: 511.5,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: 43.0,
                doppler_hz: -800.0,
                tracking_jitter_chips: 0.0,
            },
        ];
        let prs = from_common_receive_epoch(&observations, 40_400_000.0, 4e6, 0.075);
        assert_eq!(prs.len(), 2);

        let rx_sow = 600.0 + 0.075 + 0.100;
        let tx0 = 600.0 + 0.100 + 255.75 / 1023.0 * 1e-3;
        let tx1 = 600.0 + 0.097 + 511.5 / 1023.0 * 1e-3;
        assert!((prs[0].receive_time_sow - rx_sow).abs() < 1e-12);
        assert!((prs[0].transmit_time_sow - tx0).abs() < 1e-12);
        assert!((prs[1].transmit_time_sow - tx1).abs() < 1e-12);
    }

    #[test]
    fn test_fractional_how_epoch_sample_changes_pseudorange() {
        let mut observations = [
            TrackingObservation {
                prn: 1,
                how_epoch_rx_sample: 40_000_000.0,
                tow_count: 100,
                code_phase_chips: 0.0,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: 45.0,
                doppler_hz: 1200.0,
                tracking_jitter_chips: 0.0,
            },
            TrackingObservation {
                prn: 2,
                how_epoch_rx_sample: 40_000_000.0,
                tow_count: 100,
                code_phase_chips: 0.0,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: 43.0,
                doppler_hz: -800.0,
                tracking_jitter_chips: 0.0,
            },
        ];
        let base = from_common_receive_epoch(&observations, 40_400_000.0, 4e6, 0.075);
        observations[1].how_epoch_rx_sample += 0.5;
        let shifted = from_common_receive_epoch(&observations, 40_400_000.0, 4e6, 0.075);

        let expected = C_LIGHT * 0.5 / 4e6;
        let delta = shifted[1].pseudorange_m - base[1].pseudorange_m;
        assert!(
            (delta - expected).abs() < 1e-3,
            "delta={delta}, expected={expected}"
        );
    }

    #[test]
    fn test_rx_sample_offset_refines_channel_pseudorange() {
        let mut observations = [TrackingObservation {
            prn: 1,
            how_epoch_rx_sample: 40_000_000.0,
            tow_count: 100,
            code_phase_chips: 0.0,
            rx_sample_offset_samples: 0.0,
            cn0_db_hz: 45.0,
            doppler_hz: 1200.0,
            tracking_jitter_chips: 0.0,
        }];
        let base = from_common_receive_epoch(&observations, 40_400_000.0, 4e6, 0.075);
        observations[0].rx_sample_offset_samples = 0.25;
        let refined = from_common_receive_epoch(&observations, 40_400_000.0, 4e6, 0.075);

        let expected = -C_LIGHT * 0.25 / 4e6;
        let delta = refined[0].pseudorange_m - base[0].pseudorange_m;
        assert!(
            (delta - expected).abs() < 1e-3,
            "delta={delta}, expected={expected}"
        );
    }

    #[test]
    fn test_interpolate_tow_sow() {
        let history = [
            TowSample {
                rx_sample: 1000.0,
                tow_sow: 10.0,
            },
            TowSample {
                rx_sample: 5000.0,
                tow_sow: 10.001,
            },
        ];
        let tow = interpolate_tow_sow(&history, 3000.0).unwrap();
        assert!((tow - 10.0005).abs() < 1e-12);

        let extrapolated = interpolate_tow_sow(&history, 9000.0).unwrap();
        assert!((extrapolated - 10.002).abs() < 1e-12);
    }

    #[test]
    fn test_interpolate_tow_sow_wraps_week() {
        let history = [
            TowSample {
                rx_sample: 0.0,
                tow_sow: 604_799.999,
            },
            TowSample {
                rx_sample: 4000.0,
                tow_sow: 0.0,
            },
        ];
        let tow = interpolate_tow_sow(&history, 8000.0).unwrap();
        assert!((tow - 0.001).abs() < 1e-9, "tow={tow}");
    }

    #[test]
    fn test_tow_history_common_receive_epoch() {
        let h1 = [
            TowSample {
                rx_sample: 40_000_000.0,
                tow_sow: 600.0,
            },
            TowSample {
                rx_sample: 40_400_000.0,
                tow_sow: 600.100,
            },
        ];
        let h2 = [
            TowSample {
                rx_sample: 40_012_000.0,
                tow_sow: 600.0,
            },
            TowSample {
                rx_sample: 40_412_000.0,
                tow_sow: 600.100,
            },
        ];
        let observations = [
            TowHistoryObservation {
                prn: 1,
                tow_history: &h1,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: 45.0,
                doppler_hz: 1000.0,
                tracking_jitter_chips: 0.0,
            },
            TowHistoryObservation {
                prn: 2,
                tow_history: &h2,
                rx_sample_offset_samples: 0.0,
                cn0_db_hz: 43.0,
                doppler_hz: -500.0,
                tracking_jitter_chips: 0.0,
            },
        ];

        let prs = from_tow_history_common_receive_epoch(&observations, 40_400_000.0, 4e6, 0.075);
        assert_eq!(prs.len(), 2);
        assert!((prs[0].pseudorange_m - 0.075 * C_LIGHT).abs() < 1e-3);
        assert!((prs[1].pseudorange_m - 0.078 * C_LIGHT).abs() < 1e-3);
    }

    #[test]
    fn test_code_phase_samples_from_chips_wraps_one_code_period() {
        let samples = code_phase_samples_from_chips(1023.25, 4e6);
        let expected = 0.25 * 4e6 / 1.023e6;
        assert!((samples - expected).abs() < 1e-12);
    }

    #[test]
    fn test_tracking_timestamp_observation_matches_offset_observation() {
        let history = [
            TowSample {
                rx_sample: 40_000_000.0,
                tow_sow: 600.0,
            },
            TowSample {
                rx_sample: 40_400_000.0,
                tow_sow: 600.100,
            },
        ];
        let common_rx_sample = 40_400_000.0;
        let offset = 1082.25;
        let offset_observation = [TowHistoryObservation {
            prn: 1,
            tow_history: &history,
            rx_sample_offset_samples: offset,
            cn0_db_hz: 45.0,
            doppler_hz: 1000.0,
            tracking_jitter_chips: 0.0,
        }];
        let timestamp_observation = [TrackingTimestampObservation {
            prn: 1,
            tow_history: &history,
            tracking_sample_counter: common_rx_sample as u64 + offset.floor() as u64,
            rem_code_phase_samples: offset.fract(),
            cn0_db_hz: 45.0,
            doppler_hz: 1000.0,
            tracking_jitter_chips: 0.0,
        }];

        let from_offset = from_tow_history_common_receive_epoch(
            &offset_observation,
            common_rx_sample,
            4e6,
            0.075,
        );
        let from_timestamp = from_tracking_timestamp_common_receive_epoch(
            &timestamp_observation,
            common_rx_sample,
            4e6,
            0.075,
        );

        assert_eq!(from_timestamp.len(), 1);
        assert!((from_timestamp[0].pseudorange_m - from_offset[0].pseudorange_m).abs() < 1e-6);
    }

    #[test]
    fn test_observables_builder_prefers_tracking_timestamp_source() {
        let history = [
            TowSample {
                rx_sample: 40_000_000.0,
                tow_sow: 600.0,
            },
            TowSample {
                rx_sample: 40_400_000.0,
                tow_sow: 600.100,
            },
        ];
        let tracking = [
            tracking_timestamp_observation(1, &history, 40_400_000, 0.1),
            tracking_timestamp_observation(2, &history, 40_400_010, 0.2),
            tracking_timestamp_observation(3, &history, 40_400_020, 0.3),
            tracking_timestamp_observation(4, &history, 40_400_030, 0.4),
        ];
        let nco = [
            nco_observation(1),
            nco_observation(2),
            nco_observation(3),
            nco_observation(4),
        ];

        let built = ObservablesBuilder::new(40_400_000.0, 4e6)
            .with_tracking_timestamp_observations(&tracking)
            .with_nco_reference_observations(&nco)
            .build();

        assert_eq!(built.source, ObservablesSource::TrackingTimestamp);
        assert_eq!(built.pseudoranges.len(), 4);
        assert_eq!(built.reference_pseudoranges.len(), 4);
    }

    #[test]
    fn test_observables_builder_falls_back_to_nco_when_tracking_timestamp_insufficient() {
        let history = [
            TowSample {
                rx_sample: 40_000_000.0,
                tow_sow: 600.0,
            },
            TowSample {
                rx_sample: 40_400_000.0,
                tow_sow: 600.100,
            },
        ];
        let tracking = [tracking_timestamp_observation(1, &history, 40_400_000, 0.1)];
        let nco = [
            nco_observation(1),
            nco_observation(2),
            nco_observation(3),
            nco_observation(4),
        ];

        let built = ObservablesBuilder::new(40_400_000.0, 4e6)
            .with_tracking_timestamp_observations(&tracking)
            .with_nco_reference_observations(&nco)
            .build();

        assert_eq!(built.source, ObservablesSource::NcoFallback);
        assert_eq!(built.pseudoranges.len(), 4);
        assert_eq!(built.reference_pseudoranges.len(), 4);
    }

    #[test]
    fn test_observables_input_builder_forms_all_timestamp_inputs() {
        let history = [
            TowSample {
                rx_sample: 40_000_000.0,
                tow_sow: 600.0,
            },
            TowSample {
                rx_sample: 40_400_000.0,
                tow_sow: 600.100,
            },
        ];
        let snapshots = [TrackingChannelSnapshot {
            prn: 7,
            tow_count: 100,
            how_epoch_rx_sample: 40_000_000.0,
            code_phase_chips: 511.5,
            tracking_sample_counter: 40_400_123,
            rem_code_phase_samples: 0.25,
            tow_history: &history,
            cn0_db_hz: 46.0,
            doppler_hz: -1200.0,
            tracking_jitter_chips: 0.125,
        }];

        let inputs = ObservablesInputBuilder::new(40_400_000.0, 4e6)
            .with_channel_snapshots(&snapshots)
            .build();

        assert_eq!(inputs.nco_reference.len(), 1);
        assert_eq!(inputs.quantized_reference.len(), 1);
        assert_eq!(inputs.rx_rem_reference.len(), 1);
        assert_eq!(inputs.tow_history_code_phase.len(), 1);
        assert_eq!(inputs.tracking_timestamp.len(), 1);
        assert_eq!(
            inputs.tracking_timestamp[0].tracking_sample_counter,
            40_400_123
        );
        assert!((inputs.tracking_timestamp[0].tracking_jitter_chips - 0.125).abs() < 1e-12);
        assert!((inputs.tracking_timestamp[0].rem_code_phase_samples - 0.25).abs() < 1e-12);
        assert!((inputs.tow_history_delta_min_ms - 0.0).abs() < 1e-9);
        assert!((inputs.tow_history_delta_max_ms - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_observables_input_builder_filters_invalid_channels() {
        let history = [TowSample {
            rx_sample: 40_000_000.0,
            tow_sow: 600.0,
        }];
        let snapshots = [
            TrackingChannelSnapshot {
                prn: 1,
                tow_count: 0,
                how_epoch_rx_sample: 40_000_000.0,
                code_phase_chips: 0.0,
                tracking_sample_counter: 40_400_000,
                rem_code_phase_samples: 0.0,
                tow_history: &history,
                cn0_db_hz: 45.0,
                doppler_hz: 1000.0,
                tracking_jitter_chips: 0.0,
            },
            TrackingChannelSnapshot {
                prn: 2,
                tow_count: 100,
                how_epoch_rx_sample: 40_500_000.0,
                code_phase_chips: 0.0,
                tracking_sample_counter: 40_400_000,
                rem_code_phase_samples: 0.0,
                tow_history: &history,
                cn0_db_hz: 45.0,
                doppler_hz: 1000.0,
                tracking_jitter_chips: 0.0,
            },
        ];

        let inputs = ObservablesInputBuilder::new(40_400_000.0, 4e6)
            .with_channel_snapshots(&snapshots)
            .build();

        assert!(inputs.nco_reference.is_empty());
        assert!(inputs.tracking_timestamp.is_empty());
    }

    #[test]
    fn test_resolve_integer_ms_ambiguities_aligns_common_clock_bias() {
        let code_period_m = C_LIGHT * 1e-3;
        let predicted = [
            (1, 20_000_000.0),
            (2, 21_000_000.0),
            (3, 22_000_000.0),
            (4, 23_000_000.0),
            (5, 24_000_000.0),
        ];
        let mut pseudoranges: Vec<Pseudorange> = predicted
            .iter()
            .map(|(prn, predicted_m)| pseudorange_for_ambiguity(*prn, predicted_m + 50.0))
            .collect();
        pseudoranges[2].pseudorange_m -= code_period_m;

        let resolved = resolve_integer_ms_ambiguities(&pseudoranges, &predicted, 2);

        for (resolved, (_, predicted_m)) in resolved.iter().zip(predicted.iter()) {
            assert!((resolved.pseudorange_m - predicted_m - 50.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_resolve_integer_ms_ambiguities_leaves_unambiguous_ranges() {
        let predicted = [(1, 20_000_000.0), (2, 21_000_000.0), (3, 22_000_000.0)];
        let pseudoranges: Vec<Pseudorange> = predicted
            .iter()
            .map(|(prn, predicted_m)| pseudorange_for_ambiguity(*prn, predicted_m - 25.0))
            .collect();

        let resolved = resolve_integer_ms_ambiguities(&pseudoranges, &predicted, 2);

        for (before, after) in pseudoranges.iter().zip(resolved.iter()) {
            assert_eq!(before.pseudorange_m, after.pseudorange_m);
        }
    }

    fn tracking_timestamp_observation<'a>(
        prn: u8,
        tow_history: &'a [TowSample],
        tracking_sample_counter: u64,
        rem_code_phase_samples: f64,
    ) -> TrackingTimestampObservation<'a> {
        TrackingTimestampObservation {
            prn,
            tow_history,
            tracking_sample_counter,
            rem_code_phase_samples,
            cn0_db_hz: 45.0,
            doppler_hz: 1000.0,
            tracking_jitter_chips: 0.0,
        }
    }

    fn nco_observation(prn: u8) -> TrackingObservation {
        TrackingObservation {
            prn,
            how_epoch_rx_sample: 40_000_000.0,
            tow_count: 100,
            code_phase_chips: 0.0,
            rx_sample_offset_samples: 0.0,
            cn0_db_hz: 45.0,
            doppler_hz: 1000.0,
            tracking_jitter_chips: 0.0,
        }
    }

    fn pseudorange_for_ambiguity(prn: u8, pseudorange_m: f64) -> Pseudorange {
        Pseudorange {
            prn,
            pseudorange_m,
            transmit_time_sow: 100.0,
            receive_time_sow: 100.075,
            cn0_db_hz: 45.0,
            doppler_hz: 0.0,
            tracking_jitter_chips: 0.0,
        }
    }
}
