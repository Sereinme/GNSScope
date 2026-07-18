# Pseudorange Research: GNSS-SDR, IS-GPS-200, and RTKLIB

**Date**: 2026-07-18
**Purpose**: Research the exact pseudorange computation formula used by GNSS-SDR and the IS-GPS-200 standard, to inform the GNSScope PVT fix (adding `tracking_sample_counter` and `rem_code_phase_samples` to the tracking layer).

---

## 1. GNSS-SDR Pseudorange Computation

GNSS-SDR uses a **common reception time** algorithm. The key data path is:

```
Tracking → Telemetry Decoder → Observables
```

### 1.1 Tracking Layer Outputs

The tracking block (`gps_l1_ca_dll_pll_tracking_cc.cc`) outputs three critical fields per integration period:

| Field | Type | Meaning |
|-------|------|---------|
| `Tracking_sample_counter` | `uint64_t` | Absolute sample count since tracking start. Incremented by `d_correlation_length_samples` per integration cycle. |
| `Code_phase_samples` (aka `d_rem_code_phase_samples`) | `double` | Sub-sample code phase residual, always `< 1` sample. Represents where within the current sample the code phase is aligned. |
| `fs` | `uint64_t` | Sampling frequency in Hz (copied from signal source configuration). |

**Increment logic** (from GNSS-SDR GPU DLL-PLL tracking implementation):

After each correlation cycle during steady-state tracking:
```cpp
d_sample_counter += d_correlation_length_samples;  // increment by 1 ms worth of samples
current_synchro_data.Tracking_sample_counter = d_sample_counter + d_correlation_length_samples;
current_synchro_data.Code_phase_samples = d_rem_code_phase_samples;
```

**Receiver time computation** (from `hybrid_observables_cc.cc` or `hybrid_observables_gs.cc`):
```cpp
double compute_T_rx_s(const Gnss_Synchro& a)
{
    return (static_cast<double>(a.Tracking_sample_counter) + a.Code_phase_samples)
           / static_cast<double>(a.fs);
}
```

This gives a high-resolution reception time in seconds, precise to sub-sample level.

**Pull-in alignment** (acquisition-to-tracking handoff):
```cpp
// Compute delay between acquisition stamp and tracking start
acq_to_trk_delay_samples = d_sample_counter - d_acq_sample_stamp;
// Align to next PRN code period boundary
acq_trk_shif_correction_samples = d_correlation_length_samples
    - fmod(acq_to_trk_delay_samples, d_correlation_length_samples);
samples_offset = round(d_acq_code_phase_samples + acq_trk_shif_correction_samples);
d_sample_counter += samples_offset;  // jump to aligned sample index
```

**Key insight**: `Tracking_sample_counter` is an **absolute** sample index -- it starts from the acquisition sample stamp (the sample index at which acquisition detected the signal), and counts forward with the sample stream. Each tracked channel maintains its own counter but they all reference the same underlying sample stream.

Source: GNSS-SDR `src/algorithms/tracking/libs/` and `src/algorithms/tracking/gnuradio_blocks/gps_l1_ca_dll_pll_tracking_gpu_cc.cc`
(Observed in web search results summarizing code from commits `807ca24fc2`, `1c2f897655`)

### 1.2 Telemetry Decoder: TOW Maintenance

The GPS L1 C/A telemetry decoder (`gps_l1_ca_telemetry_decoder_cc.cc`) maintains `d_TOW_at_current_symbol_ms` as an unsigned integer in milliseconds:

**Setting TOW on subframe decode** (per GNSS-SDR commit `a797bfb641`):
```cpp
// HOW TOW is the TOW of the NEXT subframe's first symbol.
// When the decoder finishes the current subframe, the last symbol
// corresponds to the time at which the next subframe begins = HOW TOW.
// Therefore, set TOW to HOW TOW × 1000 ms.
d_TOW_at_current_symbol_ms = static_cast<unsigned int>(d_TOW) * 1000;
```

**Per-symbol increment** (every 20 ms C/A bit period):
```cpp
// GNSS-SDR uses either ++1 (per 1 ms prompt) or += 20 (per symbol):
d_TOW_at_current_symbol_ms += GPS_L1_CA_CODE_PERIOD_MS;  // += 1 for per-ms mode
// or (in GNSScope's port):
d_TOW_at_current_symbol_ms = (d_TOW_at_current_symbol_ms + 20) % 604_800_000;
```

**GNSScope implementation** (`nav/decoder.rs`, lines 150-167):
```rust
// On subframe decode:
self.tow_at_current_symbol_ms = (self.tow as u64) * 1000;
// Per-symbol:
self.tow_at_current_symbol_ms = (self.tow_at_current_symbol_ms + 20) % 604_800_000;
```

**TOW precision notes**:
- The double-precision `d_TOW_at_current_symbol` uses `floor(value * 1000.0) / 1000.0` -- effectively millisecond precision.
- The unsigned-int `d_TOW_at_current_symbol_ms` has exact millisecond precision.
- TOW interpolation between symbols (sub-20ms) requires external code phase information.

Source: GNSS-SDR `src/algorithms/telemetry_decoder/gnuradio_blocks/gps_l1_ca_telemetry_decoder_cc.cc`
(Observed in web search, commits `a797bfb641`, `1574e277d7`, `b2b889ec79`)

### 1.3 Observables Block: Pseudorange Formula

GNSS-SDR's `hybrid_observables_cc.cc` computes pseudorange via the **common reception time** algorithm.

**Step 1: Select reference satellite** (the one with the highest TOW):
```cpp
// Find channel with max TOW
auto max_elem = max_element(..., compare_by_d_TOW_hybrid_at_current_symbol);
double d_TOW_reference = max_elem->second.d_TOW_hybrid_at_current_symbol;
double d_ref_PRN_rx_time_ms = max_elem->second.Prn_timestamp_ms;
```

**Step 2: Compute pseudorange for each satellite**:
```cpp
// For each valid channel:
delta_rx_time_ms = Prn_timestamp_ms(channel) - d_ref_PRN_rx_time_ms;
delta_TOW_ms = (d_TOW_reference - d_TOW_at_current_symbol) * 1000.0;
traveltime_ms = delta_TOW_ms + delta_rx_time_ms + GPS_STARTOFFSET_ms;
pseudorange_m = traveltime_ms * GPS_C_m_ms;  // GPS_C_m_ms = 299792.458 (c in m/ms)
```

**Variable meanings**:
| Variable | Meaning |
|----------|---------|
| `Prn_timestamp_ms` | Receiver clock time (ms from sampling start) tagged to when the PRN symbol was processed. Equivalent to `sample_counter / fs * 1000`. |
| `d_TOW_reference` | TOW (seconds of week) of the reference satellite's current symbol. |
| `d_TOW_at_current_symbol` | TOW (seconds of week) at the current symbol of this satellite. |
| `delta_rx_time_ms` | Reception time difference between this channel and the reference -- compensates for different signal arrival times. |
| `delta_TOW_ms` | TOW difference between reference and this satellite (× 1000 for ms). |
| `GPS_STARTOFFSET_ms` | ~68.802 ms -- minimum GPS signal travel time (satellite directly overhead). Seeds the pseudorange. |
| `GPS_C_m_ms` | 299792.458 -- speed of light in meters per millisecond. |

**Newer version** (in `hybrid_observables_gs.cc`, commit `e58f96a`):
```cpp
double T_rx = compute_T_rx_s(gnss_synchro);  // = (Tracking_sample_counter + Code_phase_samples) / fs
```

**What makes this work for our offline scenario**:
- The **relative offset** between channels' `Tracking_sample_counter` captures the sub-ms travel time differences.
- Using a reference satellite with a known common travel time resolves the integer ms ambiguity.
- The `Code_phase_samples` provides the sub-sample fine timing.

Source: GNSS-SDR `src/algorithms/observables/gnuradio_blocks/hybrid_observables_cc.cc`
(Observed in web search at commits `e58f96a`, `8630cf2fb`)

### 1.4 The `Gnss_Synchro` Structure

Key fields in `gnss_synchro.h`:

| Field | Type | Set By |
|-------|------|--------|
| `Tracking_sample_counter` | `uint64_t` / `unsigned long int` | Tracking |
| `Code_phase_samples` | `double` | Tracking (copied from `d_rem_code_phase_samples`) |
| `fs` | `uint64_t` | Signal source configuration |
| `Prn_timestamp_ms` | `double` | Tracking (sample_counter-derived receiver time in ms) |
| `Tracking_timestamp_secs` | `double` | Tracking ( = `(sample_counter + rem_code_phase) / fs`) |
| `Acq_samplestamp_samples` | `unsigned long int` | Acquisition |
| `d_TOW_at_current_symbol` | `double` | Telemetry decoder (seconds, floored to ms) |
| `d_TOW_at_current_symbol_ms` | `unsigned int` | Telemetry decoder (milliseconds) |
| `d_TOW_hybrid_at_current_symbol` | `double` | Telemetry decoder (for multi-constellation) |
| `Pseudorange_m` | `double` | Observables (output) |

Source: GNSS-SDR `src/core/system_parameters/gnss_synchro.h` (commit `9d34147e0f`)

---

## 2. IS-GPS-200 Pseudorange Definition

### 2.1 Fundamental Formula

IS-GPS-200 defines pseudorange as (Sections 20.3.3.3.3.1, 30.3.3.3.1.1.1):

```
ρ_m(Li,x) = c · (t_u − t_sv_Li,x)
```

Where:
- **c** = speed of light (299,792,458 m/s)
- **t_u** = user receiver time at signal reception (in GPS time)
- **t_sv_Li,x** = time of transmission from the Space Vehicle (SV) for signal `x` on L-band carrier `Li`, **as measured by the user at their receiver**

Key reference: "the pseudorange is fundamentally expressed as the difference between user time in the receiver, t_u, and the time of transmission from the SV as measured by the user at their receiver, t_sv_Li,x" -- IS-GPS-200, Section 20.3.3.3.

### 2.2 Time of Transmission from Code Phase

The IS-GPS-200 specification states:

> "The transmission time measurements are the raw code phase measurements, which are extracted from the prompt-tap numerically-controlled oscillator (NCO) code phase of the delay lock loop."

This means:
1. The DLL's prompt tap code NCO phase directly provides `t_sv_uncompensated` (the uncompensated time of transmission).
2. This raw measurement must be corrected for satellite clock offset, relativity, and hardware biases.

### 2.3 Correction Chain

The compensated time of transmission is:

```
t_sv_Li,x = t_sv_uncompensated − (Δt_sv)_Li,x
```

Where the satellite clock correction is:

```
Δt_sv = a_f0 + a_f1(t − t_oc) + a_f2(t − t_oc)² + Δt_relativity
```

And for L1 C/A single-frequency users (Section 30.3.3.3.1.1.1):

```
(Δt_sv)_L1C/A = Δt_sv − T_GD + ISC_L1C/A
```

Where:
- `a_f0, a_f1, a_f2` = broadcast clock polynomial coefficients
- `t_oc` = clock reference epoch
- `Δt_relativity` = relativistic correction term
- `T_GD` = timing group delay (legacy single-frequency correction)
- `ISC_L1C/A` = inter-signal correction between L1 P(Y) and L1 C/A

### 2.4 Relevant IS-GPS-200 Sections

| Section | Content |
|---------|---------|
| 20.3.3.3 | Pseudorange model definition (t_u, t_sv, raw code phase) |
| 20.3.3.3.3.1 | L1 C/A single-frequency code phase correction algorithm |
| 20.3.3.3.3.2 | Legacy L1P(Y)/L2P(Y) single-frequency corrections (T_GD removal) |
| 20.3.3.3.3.3 | Ionospheric correction (for dual-frequency users) |
| 30.3.3.3.1.1.1 | Modernized single-frequency corrections (any signal, includes ISC) |
| 30.3.3.3.1.1.2 | Modernized ionosphere-free dual-frequency pseudorange equation |

### 2.5 Integer Millisecond Ambiguity

The IS-GPS-200 does not explicitly detail the integer ms resolution algorithm -- this is an implementation concern. However, the HOW (Handover Word) TOW count provides the mechanism: each subframe's HOW carries a 17-bit TOW count (0-100799, modulo 604800 seconds in the GPS week), providing a 6-second absolute time reference. Combined with bit/symbol counting within the subframe, the receiver can reconstruct integer ms.

The receiver must resolve:
- **Integer ms**: from the HOW TOW (which subframe is currently being received) + bit/symbol counting
- **Sub-ms**: from the DLL code phase measurement (0-1023 chips for GPS L1 C/A)
- **Sub-chip**: from the tracking loop's sub-sample code phase residual

Sources:
- IS-GPS-200K (available at MSOE EE4980 course materials)
- "Draper Technology Digest 2010" (Draper Laboratory) -- contains IS-GPS-200 pseudorange analysis
- "Inside GNSS - Inter-Signal Correction Appendices" (July/August 2009)

---

## 3. RTKLIB Pseudorange Handling

RTKLIB is a **post-processing library**, not a real-time SDR. It does not compute pseudorange from raw samples -- it reads pre-computed pseudorange values from receiver output files (RINEX, RTCM, proprietary binary formats).

### 3.1 Observation Data Structure (`obsd_t`)

From `rtklib.h`:
```c
typedef struct {
    gtime_t time;                   // Receiver sampling time (GPST)
    uint8_t sat, rcv;              // Satellite/receiver number
    uint16_t SNR[NFREQ+NEXOBS];   // Signal strength (0.001 dBHz)
    uint8_t  LLI[NFREQ+NEXOBS];   // Loss of lock indicator
    uint8_t  code[NFREQ+NEXOBS];  // Code indicator (CODE_??)
    double   L[NFREQ+NEXOBS];     // Carrier phase observation (cycles)
    double   P[NFREQ+NEXOBS];     // Pseudorange observation (meters)
    float    D[NFREQ+NEXOBS];     // Doppler frequency observation (Hz)
} obsd_t;
```

- `P[0]` = L1 pseudorange (meters)
- `P[1]` = L2 pseudorange (meters)

### 3.2 PVT Solution (pntpos.c)

RTKLIB's `estpos()` function applies corrections to the pre-computed pseudorange:

```c
meas = obs[j].P[0]               // raw pseudorange from receiver (m)
     - dts[j*2] * CLIGHT;        // subtract satellite clock bias (m)
meas -= ionocorr(time, nav, ...); // subtract ionospheric delay (m)
meas -= tropcorr(time, ...);      // subtract tropospheric delay (m)
v[j] = meas - (rho - x[3]);       // O-C residual
```

Where `rho` is the geometric range computed from satellite position `rs[j]` and approximate receiver position.

### 3.3 Relevance to GNSScope

RTKLIB's formula confirms the correction chain:
1. Raw pseudorange (from tracking/observables)
2. Subtract satellite clock bias (broadcast polynomial + T_GD + relativity)
3. Subtract ionospheric delay (Klobuchar or dual-frequency)
4. Subtract tropospheric delay (Saastamoinen or similar model)
5. = Corrected pseudorange used in WLS solution

This matches GNSScope's current implementation in `pvt/solver.rs` (lines 98-131), where `Pseudorange::from_tracking` outputs raw pseudorange and `solver.rs` applies clock, Sagnac, iono, and tropo corrections.

Source: RTKLIB `src/rtkcmn.c`, `src/pntpos.c`, `src/rtklib.h` (Tomoji Takasu)

---

## 4. The GNSS-SDR Formula Applied to GNSScope

### 4.1 Current GNSScope Problem

From `M2-PROGRESS.md`:
- `DllPllTracking.code_phase_chips` is an NCO accumulator (mod 1023), NOT the true signal code phase
- It carries an offset from the acquisition initialization that varies per satellite
- This causes 5km-level pseudorange inconsistencies across satellites

### 4.2 Required Architecture (GNSS-SDR Pattern)

The fix requires three additions to `DllPllTracking`:

**1. `tracking_sample_counter: u64`**
- Initialized to the acquisition sample stamp (the sample index at which the signal was acquired)
- Incremented by `fs * 0.001` (4,000 at 4 MHz) per 1 ms integration period
- Output in `TrackingOutput`

**2. `rem_code_phase_samples: f64` (sub-sample code phase residual)**
- Tracks the fractional-sample offset of the code phase within the current sample
- Updated per integration period from the code NCO state modulo sample rate
- Always in range [0, 1), in units of samples
- Output in `TrackingOutput`

**3. Pseudorange computation (in `Pseudorange::from_tracking` or new method)**

Replace the current `rx_sample_index / fs` approach with:

```
T_rx_s = (tracking_sample_counter + rem_code_phase_samples) / fs
T_tx_s = tow_at_current_symbol_ms / 1000.0
pseudorange_m = c * (T_rx_s - T_tx_s)
```

Where:
- `T_rx_s` is the precise receiver sample-clock time (seconds), with sub-sample precision from `rem_code_phase_samples`
- `T_tx_s` is the transmit time (seconds) from the telemetry decoder's per-symbol TOW
- The difference `T_rx_s - T_tx_s` naturally captures the full travel time including integer ms

### 4.3 Channel Alignment Constraint

For the offline file-processing case, all channels share the same sample stream. The `tracking_sample_counter` must be monotonic across the file. Since each channel starts tracking at a different sample index (its acquisition stamp), the absolute `tracking_sample_counter` values will differ between channels. This is correct -- the difference naturally encodes the relative signal arrival times.

At a given receiver epoch (same sample index), different channels will have processed different sample counts because they started at different times. To compute pseudorange at a common receive epoch:
- Pick the current sample index (from the file reader)
- For each channel, compute `T_rx = (current_sample_index + rem_code_phase) / fs`
- The pseudorange formula uses this `T_rx` directly

### 4.4 Integer Millisecond Resolution

With the GNSS-SDR approach, integer ms ambiguity is resolved implicitly:
- The HOW TOW gives the absolute second-of-week at the decoded subframe
- The per-symbol TOW counter tracks 20 ms increments from the subframe start
- `T_tx = tow_at_current_symbol_ms / 1000` gives the transmit time in seconds
- `T_rx = (sample_counter + rem_code_phase) / fs` gives the receive time in seconds
- The difference `T_rx - T_tx` = travel time in seconds, which naturally resolves to the correct integer ms (typically 65-85 ms for GPS) because `sample_counter` is in the millions (seconds × fs) and `tow_at_current_symbol_ms` is in the hundreds of thousands (seconds × 1000)

The key insight is that **both** `T_rx` and `T_tx` are expressed in the same time base (seconds), so subtraction directly yields the correct travel time without any modulo arithmetic on code phases.

---

## 5. Summary of Key Findings

| Question | Answer |
|----------|--------|
| How does GNSS-SDR compute pseudorange? | `PR = c × (T_rx − T_tx)` where `T_rx = (Tracking_sample_counter + Code_phase_samples) / fs` and `T_tx` comes from telemetry TOW |
| What is `Tracking_sample_counter`? | Absolute sample count since tracking start, incremented by correlation length per integration period |
| What is `Code_phase_samples`? | Sub-sample code phase residual, always < 1 sample, computed from the code NCO fractional state |
| What is `rem_code_phase_samples`? | Same as `Code_phase_samples` -- the internal `d_rem_code_phase_samples` double in the tracking block, copied to the Gnss_Synchro output as `Code_phase_samples` |
| How is TOW interpolated between symbols? | `d_TOW_at_current_symbol_ms` is incremented by 20 ms per symbol (or 1 ms per prompt). The per-symbol value gives exact TOW at each 20ms bit boundary. Sub-20ms interpolation uses code phase. |
| IS-GPS-200 pseudorange formula? | `ρ = c · (t_u − t_sv)` (Section 20.3.3.3). Time of transmission = raw code phase measurement from DLL prompt tap. |
| How is integer ms resolved? | HOW TOW gives 6-second reference. Per-symbol TOW counter gives 20ms increments. The full travel time `T_rx − T_tx` naturally yields the correct ms. |
| RTKLIB's role? | RTKLIB reads pre-computed pseudorange from receiver output. Its formula confirms the correction chain: raw PR → subtract clock/T_GD/relativity → subtract iono/tropo → WLS. |

---

## 6. References

1. GNSS-SDR `hybrid_observables_cc.cc` -- common reception time pseudorange algorithm (commit `e58f96a196c6`)
2. GNSS-SDR `gps_l1_ca_dll_pll_tracking_cc.cc` -- tracking sample counter increment and code phase output (commit `807ca24fc2`)
3. GNSS-SDR `gps_l1_ca_telemetry_decoder_cc.cc` -- `d_TOW_at_current_symbol_ms` per-symbol increment (commits `a797bfb641`, `1574e277d7`)
4. GNSS-SDR `gnss_synchro.h` -- Gnss_Synchro struct definition (commit `9d34147e0f`)
5. GNSS-SDR tracking interface refactoring -- `set_prn_code_phase(phase_samples)` rename (commit `dcc3e4d8588`)
6. IS-GPS-200K (available at MSOE EE4980 course page) -- Sections 20.3.3.3, 20.3.3.3.3.1, 30.3.3.3.1.1.1
7. "Draper Technology Digest 2010" -- IS-GPS-200 pseudorange analysis
8. "Inside GNSS - Inter-Signal Correction Appendices" (July/August 2009)
9. RTKLIB `rtkcmn.c`, `pntpos.c`, `rtklib.h` (Tomoji Takasu) -- `obsd_t.P[0]` pseudorange field and `estpos()` WLS solution
10. GNSScope `docs/M2-PROGRESS.md` -- root cause analysis of code_phase_chips NCO offset bug
11. GNSScope `crates/gnss-core/src/nav/decoder.rs` -- current `tow_at_current_symbol_ms` implementation
12. GNSScope `crates/gnss-core/src/pvt/pseudorange.rs` -- current pseudorange computation
