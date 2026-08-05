#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

target=${AGENTMUX_RELEASE_TARGET:-aarch64-apple-darwin}
if [ "$target" != "aarch64-apple-darwin" ]; then
  echo "v2 release target must be aarch64-apple-darwin" >&2
  exit 2
fi
version=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
if [ -n "${GITHUB_REF_NAME:-}" ] && [ "${GITHUB_REF_NAME#v}" != "$version" ]; then
  echo "tag $GITHUB_REF_NAME does not match Cargo version $version" >&2
  exit 2
fi

export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0}
export CARGO_INCREMENTAL=0
cargo build --locked --release --target "$target"

dist="$repository_root/dist"
archive_name="agentmux-$version-$target.tar.gz"
archive="$dist/$archive_name"
staging_parent=$(mktemp -d "${TMPDIR:-/tmp}/agentmux-release.XXXXXX")
trap 'rm -rf "$staging_parent"' EXIT HUP INT TERM
stage="$staging_parent/agentmux-$version-$target"
mkdir -p "$stage"
install -m 0755 "target/$target/release/agentmux" "$stage/agentmux"
install -m 0644 README.md LICENSE PROVIDER_COMPATIBILITY.md SECURITY.md "$stage/"
install -m 0644 compatibility/bootstrap/catalog-v1.json "$stage/provider-catalog-v1.json"
install -m 0644 compatibility/bootstrap/catalog-v1.sig "$stage/provider-catalog-v1.sig"
install -m 0644 compatibility/schema/catalog-v1.schema.json "$stage/provider-catalog-v1.schema.json"
install -m 0644 docs/provider-version-catalog-migration-plan.md "$stage/"

python3 scripts/generate_supply_chain.py \
  --sbom "$stage/agentmux-$version.spdx.json" \
  --licenses "$stage/THIRD_PARTY_LICENSES.md"
mkdir -p "$dist"
python3 scripts/package_archive.py "$stage" "$archive" --epoch "$SOURCE_DATE_EPOCH"
install -m 0644 "$stage/agentmux-$version.spdx.json" "$dist/"
install -m 0644 "$stage/THIRD_PARTY_LICENSES.md" "$dist/"

checksum=$(shasum -a 256 "$archive" | awk '{print $1}')
printf '%s  %s\n' "$checksum" "$archive_name" > "$dist/SHA256SUMS"
python3 scripts/render_homebrew.py \
  --version "$version" \
  --sha256 "$checksum" \
  --output "$dist/agentmux.rb"

if [ -n "${GITHUB_OUTPUT:-}" ]; then
  printf 'archive=%s\n' "$archive" >> "$GITHUB_OUTPUT"
  printf 'sbom=%s\n' "$dist/agentmux-$version.spdx.json" >> "$GITHUB_OUTPUT"
fi
printf 'packaged %s\n' "$archive"
