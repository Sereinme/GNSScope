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

if [[ "${skip_build}" == false ]]; then
    cargo build --release --locked -p gnss-gui
fi

version="$(awk -F '"' '/^version = / { print $2; exit }' crates/gnss-gui/Cargo.toml)"
binary_path="${repository_root}/target/release/gnss-gui"
dist_dir="${repository_root}/target/release/dist"
package_root="${repository_root}/target/release/package/linux/GNSScope-${version}"
deb_root="${repository_root}/target/release/package/deb"

if [[ -z "${version}" ]]; then
    printf 'Unable to read gnss-gui version\n' >&2
    exit 1
fi
if [[ ! -x "${binary_path}" ]]; then
    printf 'GNSScope binary not found: %s\n' "${binary_path}" >&2
    exit 1
fi
for command_name in dpkg-deb tar; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        printf 'Required command not found: %s\n' "${command_name}" >&2
        exit 1
    fi
done

rm -rf "${package_root}" "${deb_root}"
mkdir -p "${dist_dir}" "${package_root}/bin" "${package_root}/share/applications"
install -m 755 "${binary_path}" "${package_root}/bin/gnsscope"
install -m 644 packaging/linux/io.gnsscope.desktop "${package_root}/share/applications/io.gnsscope.desktop"
install -m 644 README.md README.zh-CN.md "${package_root}/"

archive_path="${dist_dir}/GNSScope-v${version}-linux-x86_64.tar.gz"
rm -f "${archive_path}"
tar -C "$(dirname "${package_root}")" -czf "${archive_path}" "$(basename "${package_root}")"

mkdir -p "${deb_root}/DEBIAN" "${deb_root}/usr/bin" "${deb_root}/usr/share/applications" \
    "${deb_root}/usr/share/doc/gnsscope"
install -m 755 "${binary_path}" "${deb_root}/usr/bin/gnsscope"
install -m 644 packaging/linux/io.gnsscope.desktop "${deb_root}/usr/share/applications/io.gnsscope.desktop"
install -m 644 README.md README.zh-CN.md "${deb_root}/usr/share/doc/gnsscope/"

cat >"${deb_root}/DEBIAN/control" <<EOF
Package: gnsscope
Version: ${version}
Section: science
Priority: optional
Architecture: amd64
Maintainer: GNSScope contributors
Depends: libc6, libfontconfig1, libgcc-s1, libvulkan1, libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxkbcommon0
Description: Offline GPS L1 C/A software receiver and analysis application
 GNSScope processes recorded interleaved IQ files through acquisition,
 tracking, navigation-message decoding, observables, continuous PVT,
 persistence, and desktop visualization.
EOF

deb_path="${dist_dir}/GNSScope-v${version}-linux-x86_64.deb"
rm -f "${deb_path}"
dpkg-deb --build --root-owner-group "${deb_root}" "${deb_path}" >/dev/null

printf '%s\n%s\n' "${archive_path}" "${deb_path}"
