//! Pseudorange formation from tracking observables and decoded TOW.
//!
//! Absolute pseudorange: P = c · (T_rx − T_tx), where T_tx is reconstructed
//! from the decoded TOW (HOW, subframe 1/2/3 word 2) plus the code-phase
//! offset within the current subframe, and T_rx is the receiver sample-clock
//! time.

const C_LIGHT: f64 = 299_792_458.0;
/// GPS C/A code period: 1 ms = 30000 chips at 1.023 Mchips/s.
const CODE_PERIOD_S: f64 = 1e-3;
/// GPS L1 C/A chips per code period.
const CHIPS_PER_CODE: f64 = 1023.0;

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
}

impl Pseudorange {
    /// Build a pseudorange from per-channel tracking state.
    ///
    /// - `tow_count`: the HOW TOW count (×6 s) of the most recently decoded
    ///   subframe on this channel.
    /// - `ms_into_subframe`: milliseconds elapsed since the start of that
    ///   subframe's first bit (i.e. since the preamble of the last decoded
    ///   subframe).
    /// - `rx_sample_index`: absolute sample counter at the receive epoch.
    /// - `fs`: sampling frequency [Hz].
    /// - `cn0_db_hz`, `doppler_hz`: from tracking output.
    pub fn from_tracking(
        prn: u8,
        tow_count: u32,
        ms_into_subframe: u64,
        rx_sample_index: u64,
        fs: f64,
        cn0_db_hz: f64,
        doppler_hz: f64,
    ) -> Self {
        // Transmit time (sow): TOW of the start of the *next* subframe (IS-GPS-200
        // convention: HOW TOW is the TOW of the next subframe's first bit, i.e.
        // the time at which the *current* subframe ends). Subtract one subframe
        // (6 s) to get the start of the current subframe, then add ms elapsed.
        let subframe_start_sow = tow_count as f64 * 6.0 - 6.0;
        let transmit_time_sow = subframe_start_sow + ms_into_subframe as f64 * 1e-3;

        // Receiver time (sow): sample counter / fs (s). This is a relative time
        // from the start of file; we anchor it to the GPS week using the
        // earliest decoded TOW across channels (caller supplies via rx_sample_index
        // being relative to the same epoch as the subframe decode).
        // For the M2 single-file offline pipeline, the caller passes a sample
        // index already aligned to the same epoch; the absolute week offset
        // cancels in (T_rx - T_tx) because both carry it.
        let receive_time_sow = rx_sample_index as f64 / fs;

        let pseudorange_m = (receive_time_sow - transmit_time_sow) * C_LIGHT;

        Pseudorange {
            prn,
            pseudorange_m,
            transmit_time_sow,
            receive_time_sow,
            cn0_db_hz,
            doppler_hz,
        }
    }
}

/// Code-phase in chips at a given sample within the 1 ms code period.
pub fn code_phase_chips(code_phase_chips: f64) -> f64 {
    code_phase_chips % CHIPS_PER_CODE
}

/// Fractional ms offset within a code period, used for sub-ms transmit-time
/// refinement.
pub fn _sub_ms_offset(code_phase_chips: f64) -> f64 {
    (code_phase_chips % CHIPS_PER_CODE) / CHIPS_PER_CODE * CODE_PERIOD_S
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pseudorange_sign() {
        // Receiver time later than transmit time → positive pseudorange.
        // tow_count=10 → subframe start sow = 54 s. ms_into_subframe=1000 → +1 s.
        // transmit_time_sow = 55 s. rx_sample_index at fs=4e6, 56 s → 224_000_000.
        let pr = Pseudorange::from_tracking(1, 10, 1000, 224_000_000, 4e6, 45.0, 1000.0);
        assert!((pr.transmit_time_sow - 55.0).abs() < 1e-9);
        assert!((pr.receive_time_sow - 56.0).abs() < 1e-9);
        // pseudorange = (56 - 55) * c ≈ 299_792_458 m
        assert!((pr.pseudorange_m - C_LIGHT).abs() < 1.0);
    }
}
