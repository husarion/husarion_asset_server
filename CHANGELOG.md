# Changelog

All notable changes to `husarion_asset_server`. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this crate uses [Semantic Versioning](https://semver.org/).

A `vX.Y.Z` tag (cut via `just release`) publishes prebuilt `asset_server`
binaries (amd64 + arm64) on the GitHub Release — consumed by the rosbot snap via
fetch-by-version, so the snap never compiles the r2r node from source.
