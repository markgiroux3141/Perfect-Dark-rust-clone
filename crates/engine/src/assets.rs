//! Run-time asset discovery. `AssetRoot::discover()` checks, in order: the
//! `PD_ASSETS` environment variable, then `assets/` beside the executable and
//! walking up from it (so `target/release/x.exe` finds the repo's `assets/`), then
//! the current directory. Readers for bytes, UTF-8, JSON and PNG. No caching
//! policy here: a game builds its own caches keyed by its own ids.
