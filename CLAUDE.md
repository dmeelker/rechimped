# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Toolchain

The GUI needs Rust ≥ 1.93 (gpui uses `slice::as_array`; `rust-version` in `timechimp-gui/Cargo.toml` enforces it).

```sh
cargo build                                   # whole workspace
cargo run -p timechimp-gui                    # desktop app (binary: rechimped)
cargo test                                    # all tests
cargo test -p timechimp-gui lenient_times     # single test by name
cargo clippy --workspace
TIMECHIMP_REFRESH_TOKEN=... cargo run -p timechimp --example smoke [-- --write]
```

The `smoke` example hits the real API; `--write` creates and then deletes a test time entry and mileage registration.

A recurring `note: did not finalize incremental compilation session directory … Access is denied` is harmless (Windows/Defender locking `target\debug\incremental`); delete that incremental folder if it sticks.

gpui crates are pinned to `=1.17.2` (`gpui-unofficial`, `gpui-platform-gpui-unofficial`), the version gpuikit 0.9 is locked against. Upgrading gpui means moving every `*-gpui-unofficial` crate in `Cargo.lock` together (`cargo update -p <crate> --precise ...`), or mixed versions fail to resolve.

## Architecture

Cargo workspace with two crates.

### `timechimp` — API client library
Client for TimeChimp's **undocumented internal** web API (`web.timechimp.com/api`, as used by app.timechimp.com); it may change without notice.
- `auth.rs`: `Authenticator` logs in against AWS Cognito with SRP (`srp.rs`, password never sent), handles SMS/TOTP MFA via `LoginOutcome::MfaRequired(MfaChallenge)`, and refreshes access tokens from a refresh token. Persisting `refresh_token()` lets apps skip the password.
- `client.rs`: `Client` (cheap to clone, `Arc<Authenticator>`) with one async method per endpoint. Report data (`query_time`, `query_mileage`) comes from a server-side grid and must be paged by the caller.
- `models.rs`: serde models; `NewTimeEntry` / `NewMileage` are the write payloads.
- `srp.rs` has unit tests against a pycognito-generated vector in `testdata/`.
- `error.rs`: `Error::is_auth()` says whether logging in again is the fix (Cognito `Rejected`, login protocol errors, a 401 after a refresh) as opposed to network/server trouble.
- Tests: `models.rs` decodes the JSON fixtures in `testdata/` (hand-written to the observed shapes, not captured responses; swap in scrubbed real ones when the API changes) and snapshots the `to_json` request bodies; `tests/client.rs` runs `Client` against a one-shot local HTTP server via `Client::with_base_url` and `Authenticator::with_access_token` (both `#[doc(hidden)]`, for tests).

### `timechimp-gui` — Rechimped, the gpui + gpuikit desktop app
One root entity, `TimeChimpApp` (`app.rs`), holds all state; `view.rs` implements `Render` for it.
- **Async**: reqwest needs tokio, gpui has its own executor. `TimeChimpApp::run` spawns the future on a global tokio runtime (`logic::runtime()`), awaits the `JoinHandle` inside `cx.spawn_in`, and applies the result in a callback with `&mut Self, Window, Context`. Week loads carry a generation counter so stale responses are dropped. All network work goes through `run`.
- **Keyboard**: browse-mode shortcuts are a single `on_key_down` handler (`TimeChimpApp::on_key`) on the element tracking `self.focus`; it only fires while that element (not a form input) has focus. Forms and the login card are separate modal elements with key context `"Modal"` handling the `Confirm`/`Cancel`/`CalculateDistance`/`CycleAddress` actions.
- **gpuikit overrides** (`main.rs::bind_keys`, registered *after* `gpuikit::init` so they win at equal context depth): in the `Input` context Tab/Shift-Tab move focus instead of inserting `\t`, Enter → `Confirm`, Escape → `Cancel` instead of blurring. gpuikit inputs aren't tab stops by default; `forms::text_input` sets `tab_stop(true)` on the shared focus handle.
- gpuikit's input has no password masking, hence the custom `password.rs` field.
- Config: `%APPDATA%\timechimp-tui\config.toml` (`logic::load_config`; reread on reload; a starter file listing projects and non-`z_` tags is written when missing; an old `projects.toml` is renamed to it). `billable_goal` (percent) drives the Week tab's billable bar and the report's colouring. `[projects]` maps project names to the tag names they may use (`resolve_project_tags`, case-insensitive). The time form shows those tags as checkboxes (`TimeForm.tags`, rebuilt on project change); tags on an entry that no project configures are kept untouched (`kept_tag_ids`). `[[trips]]` (`logic::TripPreset`: name, project, from, to, optional distance, return_trip, type as `business`/`home-work`, billable, notes) are preset trips: buttons above the Mileage panel (`render_presets`) open the trip form for the selected day filled in; the form's "Save as preset" appends one as text (`logic::append_trip_preset`, so comments survive; named from its notes or the addresses). Right-clicking a preset button (gpuikit `context_menu`) offers Delete, which confirms via the shared delete dialog (`DeleteTarget::Preset`) and removes the block with `toml_edit` (`logic::remove_trip_preset`), keeping the rest of the file and moving the section comment above the first preset to the next one.
- Forms (`forms.rs`) are built from gpuikit `InputState` / `SelectState<i64>` / `Switch` entities; values are read back on submit and validated in `TimeChimpApp::build_save`. Changing a time form's project replaces its task `SelectState` (selects can't change options in place).
- Delete confirmation uses gpuikit `DialogState`; popups and forms are `deferred` overlays from `TimeChimpApp::modal`. The overlay's `absolute().inset_0()` must not be wrapped in an unpositioned div, or it collapses to zero height. Status messages are gpuikit toasts (`cx.toast_manager()` must stay rendered in the root).
- App icon: `assets/rechimped.ico` (a monkey face drawn by `assets/make-icon.ps1`; rerun it after changing the design), embedded as icon resource 1 by `build.rs` via `embed-resource` (needs the Windows SDK `rc.exe`); gpui loads resource 1 for its windows.
- Theme (`main.rs::theme`): Catppuccin Latte, TimeChimp orange accent, white button fill, and an enlarged `theme.controls.medium` rung (28px); all gpuikit controls size themselves from those metrics, so tune sizes there rather than per element.
- Buttons: gpuikit's `ButtonVariant` is only `Filled`/`Destructive`. Wrap secondary buttons in `view::pop(theme, button(..))` (outline + shadow); a screen's main action uses the custom `view::primary_button` (solid accent). Keep new buttons consistent with that.
- Recent picker (new time entries only): `logic::load_history` fetches the last 5 weeks on reload; `apply_week` merges every loaded week into `TimeChimpApp.history`, and `logic::recent_items` groups it by project/task/notes. Picking replaces the form's project select together with its subscription (`TimeForm.project_sub`). ↑/↓/Enter in the filter are bound in context `RecentPicker > Input` (`main.rs`).
- Form tab order is paint order, so field order in `render_form` is the Tab sequence (time form: Start → End → Notes → Hours, on purpose).
- User menu (click the name in the header; holds Log out): `TimeChimpApp.user_menu`, rendered as a `deferred` dropdown inside the name's `relative()` wrapper plus a separate window-wide transparent backdrop in `render_main` that closes it. gpuikit's `context_menu` only opens on right-click, hence the custom one.
- Tables: `view::panel` may shrink (`min_h_0`) and its rows go in `view::table_rows`, a plain `overflow_y_scroll` div, so a long table scrolls under its fixed title and column header; the page body itself no longer scrolls.
- Mileage tab: `apply_week` sorts trips by (date, id), so `render_mileage` groups consecutive rows per day under a header and ↑/↓ follows screen order. Editing a trip (E / double-click / Edit) `PUT`s `mileage/{id}` via `Client::update_mileage`, which carries the approval statuses over from the original, as the web app does.
- Effects (`fx.rs`, state in `TimeChimpApp.fx`): gpui has no custom-shader hook, so they are built from its GPU primitives, redrawn per frame from elapsed time: aurora "orbs" are drop shadows with a huge blur (gpui's Gaussian shadow shader), ribbons are gradient-filled `PathBuilder` paths. The aurora and the login card's `glow_frame` run on the login screen; `sweep_bar` under the header sweeps once when `fx.sweep` is bumped (tab switch, week loaded) and loops while `busy`; F12 (`ToggleShowcase`, global binding) puts the aurora behind the main screen and shows the frame-time HUD. `Fx::begin_frame` (top of `render`) only requests animation frames in those states and honours `reduce_motion`, so the app stays idle otherwise; `end_frame` must stay the last child of the root, as its canvas measures CPU time per frame.
- Gaps/overlaps between consecutive entries of a day come from `logic::junction` and render as marker rows via `entry_rows_with_gaps` in the Week tab's entry list.
- `logic.rs` holds UI-free code: data loading, the lenient time parser (`9`, `930`, `12:30`; End also takes `+15`, `+1h`, `+1h30`, `+1.5h` = start + that duration, expanded on blur and at save), and the session token at `%APPDATA%\timechimp-tui\refresh_token` (path kept from the removed TUI so existing sessions stay valid; `TIMECHIMP_REFRESH_TOKEN` overrides it).

## Checking UI changes

Running the app with a stored session talks to the real account. When testing, avoid saving or deleting unless asked; cancel dialogs with Esc.

To verify visually on Windows: start `target\debug\rechimped.exe`, post input with `PostMessage` to the process's `MainWindowHandle` (a `WM_LBUTTONDOWN/UP` on an empty spot first, so the window takes key input; then `WM_KEYDOWN/UP` with VK codes), and capture with `PrintWindow`. Don't use `SendKeys`: it types into whatever window has the foreground. Stop the app before rebuilding, since a running exe blocks the linker.
