# husarion_asset_server — release automation.
#
#   just release            cut a release: bump version + CHANGELOG, tag vX.Y.Z, push
#
# A `vX.Y.Z` tag triggers .github/workflows/release.yml, which builds the
# `asset_server` binary for amd64 + arm64 (native runners, in ros:jazzy-ros-base)
# and attaches them to the GitHub Release. The rosbot snap fetches the binary by
# version (no in-snap Rust build). The full build/clippy/test gate runs in CI
# (this r2r crate needs a sourced ROS env to compile — the dev host has none), so
# `just release` only bumps + tags + pushes; CI is the build gate on the tag.

set shell := ["bash", "-uc"]

release:
    #!/usr/bin/env bash
    set -euo pipefail
    here="$(dirname "$(realpath "{{justfile()}}")")"
    cd "$here"

    cargo_toml="Cargo.toml"
    cargo_lock="Cargo.lock"
    changelog="CHANGELOG.md"

    # ---- 1. sanity --------------------------------------------------
    [ -z "$(git status --porcelain)" ] \
        || { echo "release: working tree dirty — commit or stash first" >&2; exit 1; }
    branch=$(git branch --show-current)
    [ "$branch" = "main" ] \
        || { echo "release: must be on 'main' (currently '$branch')" >&2; exit 1; }
    git fetch --quiet origin main
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] \
        || { echo "release: local 'main' is not in sync with origin/main" >&2; exit 1; }
    command -v claude >/dev/null \
        || { echo "release: 'claude' CLI not on PATH" >&2; exit 1; }
    command -v jq >/dev/null || { echo "release: 'jq' not on PATH" >&2; exit 1; }

    # ---- 2. commit range + current version --------------------------
    last_tag=$(git tag --list "v*" --sort=-v:refname | head -n1 || true)
    if [ -n "$last_tag" ]; then range="${last_tag}..HEAD"; desc="since ${last_tag}";
    else range=""; desc="full history (first release)"; fi
    commits=$(git log ${range:+$range} --no-merges --pretty='%h %s')
    [ -n "$commits" ] || { echo "release: no commits ${desc} — nothing to release." >&2; exit 1; }
    current=$(awk '/^\[package\]/{p=1} p && /^version *= */{gsub(/[" ]/,"",$3); print $3; exit}' "$cargo_toml")
    [ -n "$current" ] || { echo "release: couldn't read [package] version" >&2; exit 1; }
    echo "=== husarion_asset_server ${desc} (current: ${current}) ==="
    printf '%s\n' "$commits" | sed 's/^/  /'; echo

    # ---- 3. claude: next version + changelog section ----------------
    pf=$(mktemp); of=$(mktemp); sf=$(mktemp)
    trap 'rm -f "$pf" "$of" "$sf"' EXIT
    {
        printf 'You are preparing a release of husarion_asset_server (a Rust/r2r ROS 2 node serving package:// assets over a GetAsset service).\n\n'
        printf 'Current version: %s\nTag: vX.Y.Z\n\nCommits (newest first):\n\n%s\n\n' "$current" "$commits"
        printf 'Use the Read tool on %s to match the tone. Keep-a-Changelog format (### Added/Changed/Fixed/Removed; omit empty groups).\n' "$changelog"
        printf 'Pick the semver bump (patch=fixes, minor=features, major=breaking). Concise, user-facing bullets. Do NOT include the ## header.\n\n'
        printf 'Final message MUST be exactly:\n\n  VERSION: X.Y.Z\n  ---SECTION---\n  ### Added\n  - foo\n  ---END---\n\nReal newlines inside the block.\n'
    } > "$pf"
    claude -p "$(cat "$pf")" --allowed-tools Read --output-format json > "$of"
    raw=$(jq -r '.result // empty' "$of")
    [ -n "$raw" ] || { echo "release: claude returned empty" >&2; cat "$of" >&2; exit 1; }
    raw=$(printf '%s' "$raw" | sed -e '/^```/d')
    version=$(printf '%s\n' "$raw" | awk '/^VERSION: *[0-9]+\.[0-9]+\.[0-9]+/ {print $2; exit}')
    section=$(printf '%s\n' "$raw" | awk '/^---SECTION---$/{i=1;next} /^---END---$/{exit} i')
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "release: bad VERSION from claude" >&2; printf '%s\n' "$raw" >&2; exit 1; }
    [ -n "$section" ] || { echo "release: empty section from claude" >&2; exit 1; }
    new_tag="v${version}"
    echo "claude proposes: ${current} -> ${version}  (tag ${new_tag})"; echo

    # ---- 4. apply + gate the changelog format -----------------------
    printf '%s\n' "$section" > "$sf"
    python3 .release/apply-release.py "$version" "$sf" "$cargo_toml" "$cargo_lock" "$changelog"
    if command -v pre-commit >/dev/null 2>&1; then
        pre-commit run mdformat --files "$changelog" >/dev/null 2>&1 || true
    fi

    # ---- 5. confirm, commit, tag, push ------------------------------
    echo "=== release diff ==="
    git --no-pager diff "$cargo_toml" "$cargo_lock" "$changelog"; echo
    read -rp "Commit, tag ${new_tag}, push (→ CI builds + uploads the binaries)? [y/N] " confirm
    [ "$confirm" = "y" ] || [ "$confirm" = "Y" ] || { echo "release: aborted."; git checkout -- "$cargo_toml" "$cargo_lock" "$changelog"; exit 1; }
    git add "$cargo_toml" "$cargo_lock" "$changelog"
    git commit -m "chore: release ${new_tag}"
    git tag "$new_tag"
    git push origin main
    git push origin "$new_tag"
    echo "Pushed ${new_tag}. CI: .github/workflows/release.yml builds amd64+arm64 binaries → GitHub Release."
