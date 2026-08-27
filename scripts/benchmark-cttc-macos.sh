#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
duration_ms="${GNSSCOPE_BENCHMARK_MS:-100000}"
runs="${GNSSCOPE_BENCHMARK_RUNS:-3}"
mode="${GNSSCOPE_BENCHMARK_MODE:-both}"
data_path="${repo_root}/data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN.dat"

for command_name in cargo jq; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        echo "Required command not found: ${command_name}" >&2
        exit 1
    fi
done
if [[ ! -f "${data_path}" ]]; then
    echo "CTTC data not found: ${data_path}" >&2
    exit 1
fi
if ! [[ "${duration_ms}" =~ ^[1-9][0-9]*$ ]]; then
    echo "GNSSCOPE_BENCHMARK_MS must be a positive integer" >&2
    exit 1
fi
if ! [[ "${runs}" =~ ^[1-9][0-9]*$ ]]; then
    echo "GNSSCOPE_BENCHMARK_RUNS must be a positive integer" >&2
    exit 1
fi
if [[ "${mode}" != "serial" && "${mode}" != "parallel" && "${mode}" != "both" ]]; then
    echo "GNSSCOPE_BENCHMARK_MODE must be serial, parallel, or both" >&2
    exit 1
fi

cd "${repo_root}"
test_binary="$({
    cargo test --release -p gnss-core --test streaming_runner_real --no-run --message-format=json
} | jq -r 'select(.profile.test == true and .target.name == "streaming_runner_real") | .executable' | tail -n 1)"
if [[ -z "${test_binary}" ]]; then
    echo "Unable to resolve the streaming_runner_real test executable" >&2
    exit 1
fi
if [[ "${test_binary}" != /* ]]; then
    test_binary="${repo_root}/${test_binary}"
fi
if [[ ! -f "${test_binary}" || ! -x "${test_binary}" ]]; then
    echo "Unable to resolve the streaming_runner_real test executable" >&2
    exit 1
fi

run_mode() {
    local label="$1"
    local parallel_tracking="$2"
    local run
    local output
    local elapsed_s

    for ((run = 1; run <= runs; run++)); do
        output="$(
            cd "${repo_root}/crates/gnss-core"
            GNSSCOPE_STREAMING_TEST_MS="${duration_ms}" \
                GNSSCOPE_PARALLEL_TRACKING="${parallel_tracking}" \
                "${test_binary}" test_streaming_runner_real_data_smoke --nocapture 2>&1
        )"
        printf '%s\n' "${output}" >&2
        elapsed_s="$(printf '%s\n' "${output}" | sed -nE 's/^streamed [0-9]+ ms in ([0-9.]+) s.*/\1/p' | tail -n 1)"
        if [[ -z "${elapsed_s}" ]]; then
            echo "Unable to parse elapsed time for ${label} run ${run}" >&2
            exit 1
        fi
        printf '%s,%d,%s\n' "${label}" "${run}" "${elapsed_s}"
    done
}

printf 'mode,run,elapsed_s\n'
if [[ "${mode}" == "serial" || "${mode}" == "both" ]]; then
    run_mode serial 0
fi
if [[ "${mode}" == "parallel" || "${mode}" == "both" ]]; then
    run_mode parallel 1
fi
