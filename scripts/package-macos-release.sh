#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "${script_dir}/.." && pwd)"
skip_build=false

usage() {
    printf 'Usage: %s [--skip-build]\n' "$0"
}

while (($# > 0)); do
    case "$1" in
        --skip-build)
            skip_build=true
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            printf 'Unknown argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

cd "${repository_root}"

if [[ "${skip_build}" == true ]]; then
    bundle_path="$("${script_dir}/package-macos.sh" --skip-build)"
else
    bundle_path="$("${script_dir}/package-macos.sh")"
fi

version="$(awk -F '"' '/^version = / { print $2; exit }' crates/gnss-gui/Cargo.toml)"
architecture="$(uname -m)"
dist_dir="${repository_root}/target/release/dist"
staging_dir="${repository_root}/target/release/package/dmg"
dmg_path="${dist_dir}/GNSScope-v${version}-macos-${architecture}.dmg"

if [[ -z "${version}" ]]; then
    printf 'Unable to read gnss-gui version\n' >&2
    exit 1
fi
if [[ ! -d "${bundle_path}" ]]; then
    printf 'GNSScope application bundle not found: %s\n' "${bundle_path}" >&2
    exit 1
fi

rm -rf "${staging_dir}"
rm -f "${dmg_path}"
mkdir -p "${dist_dir}" "${staging_dir}"
ditto "${bundle_path}" "${staging_dir}/GNSScope.app"
ln -s /Applications "${staging_dir}/Applications"

hdiutil create -volname GNSScope -srcfolder "${staging_dir}" -ov -format UDZO "${dmg_path}" >/dev/null
hdiutil verify "${dmg_path}" >/dev/null

printf '%s\n' "${dmg_path}"
