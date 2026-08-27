#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_dir/.." && pwd)"
profile="release"
skip_build=false

usage() {
    printf 'Usage: %s [--debug] [--skip-build]\n' "$0"
}

while (($# > 0)); do
    case "$1" in
        --debug)
            profile="debug"
            ;;
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

cd "$repository_root"

if [[ "$skip_build" == false ]]; then
    if [[ "$profile" == "release" ]]; then
        cargo build --release -p gnss-gui
    else
        cargo build -p gnss-gui
    fi
fi

binary_path="$repository_root/target/$profile/gnss-gui"
bundle_path="$repository_root/target/$profile/bundle/osx/GNSScope.app"
contents_path="$bundle_path/Contents"
macos_path="$contents_path/MacOS"
resources_path="$contents_path/Resources"
plist_template="$repository_root/packaging/macos/Info.plist.in"
plist_path="$contents_path/Info.plist"
version="$(awk -F '"' '/^version = / { print $2; exit }' crates/gnss-gui/Cargo.toml)"

if [[ ! -x "$binary_path" ]]; then
    printf 'GNSScope binary not found: %s\n' "$binary_path" >&2
    exit 1
fi
if [[ -z "$version" ]]; then
    printf 'Unable to read gnss-gui version from Cargo.toml\n' >&2
    exit 1
fi

mkdir -p "$macos_path" "$resources_path"
install -m 755 "$binary_path" "$macos_path/gnss-gui"
sed "s/@VERSION@/$version/g" "$plist_template" > "$plist_path"
plutil -lint "$plist_path" >/dev/null
codesign --force --deep --sign - --timestamp=none "$bundle_path"

printf '%s\n' "$bundle_path"
