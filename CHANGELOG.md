# Changelog

All notable changes to `husarion_asset_server`. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this crate uses [Semantic Versioning](https://semver.org/).

A `vX.Y.Z` tag (cut via `just release`) publishes prebuilt `asset_server`
binaries (amd64 + arm64) on the GitHub Release — consumed by the rosbot snap via
fetch-by-version, so the snap never compiles the r2r node from source.

## [0.2.0] — 2026-06-26

### Added
- `asset_conformance` provider conformance suite — an untyped `GetAsset` client that validates a live provider against the spec (SPEC §16), passing 9/9 checks.
- Binary release pipeline: `just release` cuts a `vX.Y.Z` tag that publishes prebuilt `asset_server` binaries (amd64 + arm64) on the GitHub Release, so the rosbot snap fetches by version instead of compiling.

### Fixed
- Resolve the namespaced `{node_fqn}/get_asset` service correctly and drive sync `GetAsset` calls via spin.
- Wait for provider service discovery before running conformance checks.

