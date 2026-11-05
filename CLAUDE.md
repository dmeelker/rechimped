# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Toolchain

The GUI needs Rust ≥ 1.93 (gpui uses `slice::as_array`; `rust-version` in `timechimp-gui/Cargo.toml` enforces it).

```sh
cargo build                                   # whole workspace
cargo run -p timechimp-gui                    # desktop app
cargo test                                    # all tests
cargo test -p timechimp-gui lenient_times     # single test by name
cargo clippy --workspace
TIMECHIMP_REFRESH_TOKEN=... cargo run -p timechimp --example smoke [-- --write]
```

The `smoke` example hits the real API; `--write` creates and then deletes a test time entry and mileage registration.

gpui crates are pinned to `=1.17.2` (`gpui-unofficial`, `gpui-platform-gpui-unofficial`), the version gpuikit 0.9 is locked against. Upgrading gpui means moving every `*-gpui-unofficial` crate in `Cargo.lock` together (`cargo update -p <crate> --precise ...`), or mixed versions fail to resolve.

## Architecture

Cargo workspace with two crates.

### `timechimp` — API client library
Client for TimeChimp's **undocumented internal** web API (`web.timechimp.com/api`, as used by app.timechimp.com); it may change without notice.
- `auth.rs`: `Authenticator` logs in against AWS Cognito with SRP (`srp.rs`, password never sent), handles SMS/TOTP MFA via `LoginOutcome::MfaRequired(MfaChallenge)`, and refreshes access tokens from a refresh token. Persisting `refresh_token()` lets apps skip the password.
- `client.rs`: `Client` (cheap to clone, `Arc<Authenticator>`) with one async method per endpoint. Report data (`query_time`, `query_mileage`) comes from a server-side grid and must be paged by the caller.
- `models.rs`: serde models; `NewTimeEntry` / `NewMileage` are the write payloads.
- `srp.rs` has unit tests against a pycognito-generated vector in `testdata/`.

### `timechimp-gui` — gpui + gpuikit desktop app
One root entity, `TimeChimpApp` (`app.rs`), holds all state; `view.rs` implements `Render` for it.
- **Async**: reqwest needs tokio, gpui has its own executor. `TimeChimpApp::run` spawns the future on a global tokio runtime (`logic::runtime()`), awaits the `JoinHandle` inside `cx.spawn_in`, and applies the result in a callback with `&mut Self, Window, Context`. Week loads carry a generation counter so stale responses are dropped. All network work goes through `run`.
- **Keyboard**: browse-mode shortcuts are a single `on_key_down` handler (`TimeChimpApp::on_key`) on the element tracking `self.focus`; it only fires while that element (not a form input) has focus. Forms and the login card are separate modal elements with key context `"Modal"` handling the `Confirm`/`Cancel`/`CalculateDistance`/`CycleAddress` actions.
- **gpuikit overrides** (`main.rs::bind_keys`, registered *after* `gpuikit::init` so they win at equal context depth): in the `Input` context Tab/Shift-Tab move focus instead of inserting `\t`, Enter → `Confirm`, Escape → `Cancel` instead of blurring. gpuikit inputs aren't tab stops by default; `forms::text_input` sets `tab_stop(true)` on the shared focus handle.
- gpuikit's input has no password masking, hence the custom `password.rs` field.
- Tags: `%APPDATA%\timechimp-tui\projects.toml` maps project names to the tag names they may use (`logic::load_project_tags` / `resolve_project_tags`, case-insensitive; a starter file listing projects and non-`z_` tags is written when missing, reread on reload). The time form shows those tags as checkboxes (`TimeForm.tags`, rebuilt on project change); tags on an entry that no project configures are kept untouched (`kept_tag_ids`).
- Forms (`forms.rs`) are built from gpuikit `InputState` / `SelectState<i64>` / `Switch` entities; values are read back on submit and validated in `TimeChimpApp::build_save`. Changing a time form's project replaces its task `SelectState` (selects can't change options in place).
- Delete confirmation uses gpuikit `DialogState`; popups and forms are `deferred` overlays from `TimeChimpApp::modal`. Status messages are gpuikit toasts (`cx.toast_manager()` must stay rendered in the root).
- Theme: Catppuccin Latte with TimeChimp orange accent (`main.rs::theme`).
- `logic.rs` holds UI-free code: data loading, the lenient time parser (`9`, `930`, `12:30`), and the session token at `%APPDATA%\timechimp-tui\refresh_token` (path kept from the removed TUI so existing sessions stay valid; `TIMECHIMP_REFRESH_TOKEN` overrides it).

Running the app with a stored session talks to the real account. When testing, avoid saving or deleting unless asked.
