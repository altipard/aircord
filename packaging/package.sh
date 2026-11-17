#!/usr/bin/env bash
# Builds Aircord's native package for the host OS into dist/. Used by `task package` and both release pipelines.
#
#   macOS    Aircord.app via cargo-bundle, zipped with ditto   -> .app.zip
#   Linux    binary, .desktop file and icon under usr/local/   -> .tar.xz   (install: sudo tar -xJf <file> -C /)
#   Windows  the release binary                                -> .exe
#
# Environment, all optional:
#   AIRCORD_TAG       release tag (v1.2.3, v1.2.3-rc1); default: git describe of the checkout
#   AIRCORD_BUILD     CFBundleVersion on macOS; default: the numeric part of the tag
#   ASSET             output file name; default: aircord-<tag>-<os>-<arch>.<ext>
#   DIST_DIR          output directory; default: dist
#   CARGO_TARGET_DIR  respected like cargo does; default: target
#
# The version comes from the tag, never from Cargo.toml: v1.2.3-rc1 -> 1.2.3 in Info.plist. An untagged checkout
# without any reachable tag packages as 0.0.0.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

tag="${AIRCORD_TAG:-$(git describe --tags --always --dirty 2>/dev/null || true)}"
tag="${tag:-v0.0.0}"
version="${tag#v}"
numeric="${version%%-*}"
if [[ ! "$numeric" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]]; then
  numeric="0.0.0" # git describe printed only a commit hash: no tag is reachable
fi
build="${AIRCORD_BUILD:-$numeric}"

case "$(uname -s)" in
  Darwin) os=macos ext=app.zip ;;
  Linux) os=linux ext=tar.xz ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ext=exe ;;
  *)
    echo "package: unsupported host OS $(uname -s)" >&2
    exit 1
    ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch=arm64 ;;
  x86_64 | amd64) arch=amd64 ;;
  *) arch="$(uname -m)" ;;
esac

# cargo-bundle runs from the package directory below, so a relative target dir has to become absolute first.
if [[ -n "${CARGO_TARGET_DIR:-}" && "$CARGO_TARGET_DIR" != /* && "$CARGO_TARGET_DIR" != ?:* ]]; then
  export CARGO_TARGET_DIR="$root/$CARGO_TARGET_DIR"
fi
target_dir="${CARGO_TARGET_DIR:-$root/target}"
dist="${DIST_DIR:-$root/dist}"
asset="${ASSET:-aircord-${tag}-${os}-${arch}.${ext}}"
out="$dist/$asset"
mkdir -p "$dist"
rm -f "$out"

echo "package: Aircord ${version} (${os}/${arch}) -> ${out}"
cargo build --release --locked -p aircord

case "$os" in
  macos)
    if ! command -v cargo-bundle >/dev/null 2>&1; then
      echo "package: cargo-bundle is missing; install it with: cargo install cargo-bundle --locked" >&2
      exit 1
    fi
    # cargo-bundle resolves the metadata's relative paths against the package directory.
    (cd apps/aircord && cargo bundle --release --format osx)
    app="$target_dir/release/bundle/osx/Aircord.app"
    plist="$app/Contents/Info.plist"
    /usr/libexec/PlistBuddy \
      -c "Set :CFBundleShortVersionString ${numeric}" \
      -c "Set :CFBundleVersion ${build}" \
      "$plist"
    # Ad-hoc signature, so the edited Info.plist is sealed into the bundle. No Developer ID: users still
    # right-click -> Open on first launch.
    codesign --force --sign - "$app"
    ditto -c -k --keepParent "$app" "$out"
    ;;
  linux)
    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    install -Dm755 "$target_dir/release/aircord" "$stage/usr/local/bin/aircord"
    install -Dm644 packaging/linux/aircord.desktop "$stage/usr/local/share/applications/aircord.desktop"
    install -Dm644 assets/icon.png "$stage/usr/local/share/icons/hicolor/512x512/apps/aircord.png"
    tar -C "$stage" --owner=0 --group=0 -cJf "$out" usr
    ;;
  windows)
    cp "$target_dir/release/aircord.exe" "$out"
    ;;
esac

ls -l "$out"
