# Third-party notices

This project may optionally bundle the `cava` binary by downloading and building it from source during the Cargo build.

## cava

- Project: `cava`
- Upstream: https://github.com/karlstav/cava
- License: MIT (see upstream `LICENSE`)
- Usage in this project: used as an external spectrum bar producer (raw ASCII bars). This project handles rendering and styling.

### How it is obtained

When building with the Cargo feature `bundle-cava`, the build script (`build.rs`) downloads the upstream source tarball for a pinned tag (default: `0.10.6`), builds it using the upstream build system, and copies the resulting `cava` executable next to this project's built binary under `target/<profile>/cava`.

- To override the version: set `CLI_MUSIC_PLAYER_CAVA_BUNDLE_VERSION`.
- To override the source URL: set `CLI_MUSIC_PLAYER_CAVA_BUNDLE_URL`.
- To skip bundling: set `CLI_MUSIC_PLAYER_CAVA_BUNDLE_SKIP`.
