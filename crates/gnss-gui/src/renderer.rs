use crate::{GuiChannelStatus, GuiReceiverWorkspaceState};

pub fn render_workspace_snapshot(state: &GuiReceiverWorkspaceState) -> String {
    let mut out = String::new();
    push_line(
        &mut out,
        format_args!(
            "GNSScope Receiver Workspace | status={:?} progress={:.0}% message={}",
            state.status,
            state.progress_fraction * 100.0,
            state.status_message
        ),
    );
    push_line(
        &mut out,
        format_args!(
            "Settings | fs={:.3}MHz if={:.1}Hz iq={:?} nav={}",
            state.settings.sample_rate_hz / 1e6,
            state.settings.intermediate_frequency_hz,
            state.settings.iq_format,
            state.nav_source.label
        ),
    );
    push_line(
        &mut out,
        format_args!(
            "Nav Source | kind={:?} iono={} ephemeris={} path={}",
            state.nav_source.kind,
            state.nav_source.ionosphere_available,
            state.nav_source.ephemeris_available,
            format_optional_path(state.nav_source.path.as_ref())
        ),
    );
    if state.settings_issues.is_empty() {
        push_line(&mut out, format_args!("Settings Issues | none"));
    } else {
        push_line(
            &mut out,
            format_args!("Settings Issues | {}", state.settings_issues.len()),
        );
        for issue in &state.settings_issues {
            push_line(
                &mut out,
                format_args!("  {}: {}", issue.field, issue.message),
            );
        }
    }

    if let Some(pvt) = &state.latest_pvt {
        push_line(
            &mut out,
            format_args!(
                "Position | kind={:?} lat={:.6}deg lon={:.6}deg alt={:.1}m",
                pvt.solution_kind, pvt.latitude_deg, pvt.longitude_deg, pvt.altitude_m
            ),
        );
        push_line(
            &mut out,
            format_args!(
                "ECEF | x={:.1}m y={:.1}m z={:.1}m",
                pvt.ecef_m[0], pvt.ecef_m[1], pvt.ecef_m[2]
            ),
        );
        if let Some(velocity) = &pvt.velocity {
            push_line(
                &mut out,
                format_args!(
                    "Velocity | east={:+.2}m/s north={:+.2}m/s up={:+.2}m/s speed={:.2}m/s heading={}",
                    velocity.east_mps,
                    velocity.north_mps,
                    velocity.up_mps,
                    velocity.speed_mps,
                    format_optional_degrees(velocity.heading_deg)
                ),
            );
            push_line(
                &mut out,
                format_args!(
                    "Clock Drift | {:+.3}m/s residual_rms={:.3}m/s",
                    velocity.clock_drift_mps, velocity.residual_rms_mps
                ),
            );
        } else {
            push_line(&mut out, format_args!("Velocity | unavailable"));
        }
        push_line(
            &mut out,
            format_args!(
                "Time | gps_week={} rx_sow={} corrected_sow={} clk_bias={:+.3}us clk_drift={}",
                format_optional_u16(pvt.time.gps_week),
                format_optional_seconds(pvt.time.receiver_time_sow),
                format_optional_seconds(pvt.time.corrected_gps_time_sow),
                pvt.time.receiver_clock_bias_us,
                format_optional_ppm(pvt.time.receiver_clock_drift_ppm)
            ),
        );
        push_line(
            &mut out,
            format_args!(
                "Quality | converged={} sats={} used={:?} excluded={:?} rms={:.2}m std_rms={:.2} gdop={:.2} hdop={:.2} h_sigma={:.1}m v_sigma={:.1}m",
                pvt.quality.converged,
                pvt.quality.n_sats,
                pvt.quality.used_prns,
                pvt.quality.excluded_prns,
                pvt.quality.residual_rms_m,
                pvt.quality.standardized_residual_rms,
                pvt.quality.gdop,
                pvt.quality.hdop,
                pvt.quality.horizontal_sigma_m,
                pvt.quality.vertical_sigma_m
            ),
        );
    } else {
        push_line(&mut out, format_args!("Position | unavailable"));
        push_line(&mut out, format_args!("Velocity | unavailable"));
        push_line(&mut out, format_args!("Time | unavailable"));
        push_line(&mut out, format_args!("Quality | unavailable"));
    }

    push_line(
        &mut out,
        format_args!(
            "Channels | locked={} total={}",
            state.locked_channel_count(),
            state.channels.len()
        ),
    );
    for channel in &state.channels {
        push_channel(&mut out, channel);
    }
    out
}

fn push_channel(out: &mut String, channel: &GuiChannelStatus) {
    push_line(
        out,
        format_args!(
            "  PRN {:02} | acq={} trk={} nav={} cn0={:.1}dB-Hz doppler={:+.0}Hz residual={}",
            channel.prn,
            channel.acquired,
            channel.tracking_locked,
            channel.nav_locked,
            channel.cn0_db_hz,
            channel.doppler_hz,
            format_optional_meters(channel.residual_m)
        ),
    );
}

fn push_line(out: &mut String, args: std::fmt::Arguments<'_>) {
    use std::fmt::Write;
    writeln!(out, "{args}").expect("writing to String should not fail");
}

fn format_optional_degrees(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.1}deg"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_seconds(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.3}s"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_ppm(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:+.3}ppm"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_meters(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:+.3}m"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_u16(value: Option<u16>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_path(value: Option<&std::path::PathBuf>) -> String {
    value
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GuiProcessingSettings, GuiReceiverWorkspaceState};
    use std::path::PathBuf;

    #[test]
    fn test_renderer_outputs_first_screen_sections() {
        let snapshot = render_workspace_snapshot(&GuiReceiverWorkspaceState::preview());

        assert!(snapshot.contains("GNSScope Receiver Workspace"));
        assert!(snapshot.contains("Settings |"));
        assert!(snapshot.contains("Nav Source |"));
        assert!(snapshot.contains("Position | kind=Kalman"));
        assert!(snapshot.contains("Velocity | east="));
        assert!(snapshot.contains("Time | gps_week="));
        assert!(snapshot.contains("Quality | converged=true"));
        assert!(snapshot.contains("Channels | locked=5 total=5"));
        assert!(snapshot.contains("PRN 20"));
    }

    #[test]
    fn test_renderer_reports_ready_iq_only_state_without_pvt() {
        let state = GuiReceiverWorkspaceState::new(GuiProcessingSettings {
            iq_file_path: Some(PathBuf::from("signal.dat")),
            use_rinex_nav_environment: false,
            auto_discover_colocated_nav: false,
            ..Default::default()
        });

        let snapshot = render_workspace_snapshot(&state);

        assert!(snapshot.contains("status=Ready"));
        assert!(snapshot.contains("nav=IQ-only mode"));
        assert!(snapshot.contains("Settings Issues | none"));
        assert!(snapshot.contains("Position | unavailable"));
    }
}
