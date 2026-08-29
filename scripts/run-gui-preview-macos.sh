#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_dir/.." && pwd)"
layout="${1:-overview}"
bundle_path="$repository_root/target/release/bundle/osx/GNSScope.app"

width=1440
height=900
workspace_view="overview"
open_settings=0
open_comparison=0
open_interface_settings=0
open_new_analysis=0
open_run_library=0
settings_category="input"

case "$layout" in
    overview)
        ;;
    channels)
        workspace_view="channels"
        ;;
    position)
        workspace_view="position"
        ;;
    navigation)
        workspace_view="navigation"
        ;;
    settings)
        open_settings=1
        ;;
    settings-tracking)
        open_settings=1
        settings_category="tracking"
        ;;
    comparison)
        open_comparison=1
        ;;
    interface-settings)
        open_interface_settings=1
        ;;
    new-analysis)
        open_new_analysis=1
        ;;
    run-library)
        open_run_library=1
        ;;
    narrow)
        width=800
        height=720
        workspace_view="channels"
        ;;
    narrow-settings)
        width=800
        height=720
        workspace_view="channels"
        open_settings=1
        ;;
    *)
        printf 'Unknown preview layout: %s\n' "$layout" >&2
        printf 'Expected overview, channels, position, navigation, settings, settings-tracking, comparison, interface-settings, new-analysis, run-library, narrow, or narrow-settings.\n' >&2
        exit 2
        ;;
esac

if [[ "${GNSSCOPE_SKIP_BUILD:-0}" == "1" ]]; then
    "$script_dir/package-macos.sh" --skip-build >/dev/null
else
    "$script_dir/package-macos.sh" >/dev/null
fi

# LaunchServices registration is required when this script runs under GUI automation parents.
exec /usr/bin/open -W -n \
    --env "GNSSCOPE_PREVIEW=1" \
    --env "GNSSCOPE_WINDOW_WIDTH=$width" \
    --env "GNSSCOPE_WINDOW_HEIGHT=$height" \
    --env "GNSSCOPE_WORKSPACE_VIEW=$workspace_view" \
    --env "GNSSCOPE_OPEN_SETTINGS=$open_settings" \
    --env "GNSSCOPE_OPEN_COMPARISON=$open_comparison" \
    --env "GNSSCOPE_OPEN_INTERFACE_SETTINGS=$open_interface_settings" \
    --env "GNSSCOPE_OPEN_NEW_ANALYSIS=$open_new_analysis" \
    --env "GNSSCOPE_OPEN_RUN_LIBRARY=$open_run_library" \
    --env "GNSSCOPE_SETTINGS_CATEGORY=$settings_category" \
    --env "GNSSCOPE_START_DRAFT=$open_settings" \
    "$bundle_path"
