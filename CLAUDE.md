# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository. It covers rules, invariants and pitfalls; read the code for what a feature does.

## Goals

These come before convenience: a change that breaks one isn't done.

1. **No clippy warnings.** Keep `cargo clippy --workspace --all-targets` free of warnings and errors: fix every one before committing, including ones that were already there, rather than leaving them for later.
2. **Performance is key.** The app must feel instant and cost nothing while idle.
   - Idle means idle: request animation frames only while something actually moves (see `Fx::begin_frame`); a static screen does no work.
   - Never block the UI thread: network and other slow work goes through `TimeChimpApp::run`.
   - Keep `render` cheap: work out derived data when its inputs change and keep it (as the review's `Analysis` is), not on every frame.
   - Lay out only what's visible: a list that can outgrow a screen is virtualized with gpui's `list` + `ListState`, as the Review tab does, never drawn as children of a scroll div (every child is laid out every frame: Review's ~120 rows took 158 ms CPU per frame in a debug build as plain children, 21 ms as a `list`).
   - Measure, don't guess: run with `--perf-log <file>` (see Checking UI changes) and compare a view's CPU per frame before and after; a change that pushes a view well above its baseline (debug build, mock: Time ~12 ms, Review ~14 ms, Travel and Report ~24 ms median) needs fixing before it's committed.

## Toolchain

The GUI needs Rust ≥ 1.95 (gpui uses `std::hint::cold_path`; `rust-version` in `rechimped/Cargo.toml` enforces it).

```sh
cargo build                                   # whole workspace
cargo run -p rechimped                        # desktop app (binary: rechimped)
cargo run -p rechimped -- --mock              # same, against a generated in-memory account
cargo test                                    # all tests
cargo test -p rechimped lenient_times         # single test by name
cargo clippy --workspace --all-targets
TIMECHIMP_REFRESH_TOKEN=... cargo run -p timechimp-api --example smoke [-- --write]
```

The `smoke` example hits the real API; `--write` creates and then deletes a test time entry and mileage registration.

gpui crates are pinned to `=1.17.2` (`gpui-unofficial`, `gpui-platform-gpui-unofficial`), the version gpuikit 0.9 is locked against. Upgrading gpui means moving every `*-gpui-unofficial` crate in `Cargo.lock` together (`cargo update -p <crate> --precise ...`), or mixed versions fail to resolve.

A recurring `note: did not finalize incremental compilation session directory … Access is denied` is harmless (Windows/Defender locking `target\debug\incremental`); delete that folder if it sticks.

**CI**: `.github/workflows/check.yml` runs `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` on Windows on every push and pull request.

**Packaging**: `scripts\package-windows.ps1` (zip) and `scripts\package-linux.ps1` (tar.gz, built in Docker from `linux/Dockerfile`; Docker Desktop must be running) write to `dist\`, named with the version from `rechimped/Cargo.toml`. `docs/release-readme.txt` ships as `README.txt`: keep it in step with `README.md`. `.github/workflows/release.yml` runs those same scripts, so change packaging in the scripts, not the workflow. Pushing a tag `vX.Y.Z` (must match the Cargo.toml version) builds and publishes a release; a manual run only builds. The Linux binary needs glibc ≥ 2.35, libxcb, libxkbcommon(-x11) and OpenSSL 3. The window's `app_id` `rechimped` must match `linux/rechimped.desktop`.

App icon: `assets/make-icon.ps1` draws `assets/rechimped.ico` and `.png`; rerun it after changing the design. `build.rs` embeds the ico as resource 1 (needs the Windows SDK `rc.exe`).

## Architecture

Cargo workspace with two crates.

### `timechimp-api`

Client for TimeChimp's **undocumented internal** web API (`web.timechimp.com/api`, as used by app.timechimp.com); it may change without notice.
- `auth.rs`: AWS Cognito login with SRP (password never sent), SMS/TOTP MFA, token refresh. Persisting `refresh_token()` lets apps skip the password.
- `client.rs`: one async method per endpoint. Report grids (`query_time`, `query_mileage`) must be paged by the caller. `query_time` filters on the user server-side; `query_mileage` doesn't (that grid's key is unverified), so its rows include everyone the caller can see.
- `Error::is_auth()` says whether logging in again is the fix, as opposed to network/server trouble (including other Cognito errors such as throttling).
- Tests: JSON fixtures in `testdata/` are hand-written to the observed shapes, not captured; swap in scrubbed real ones when the API changes. `tests/` run against a local HTTP server standing in for TimeChimp and Cognito (`#[doc(hidden)]` constructors `with_base_url`, `with_access_token`, `from_refresh_token_at`, `login_at`), so a test never reaches AWS.

### `rechimped` — the gpui + gpuikit desktop app

One root entity, `TimeChimpApp` (`app/mod.rs`), holds UI state and a `Session` (`session.rs`): the client and everything of the logged-in user. Logging out replaces the `Session` whole. Behaviour is split over `app/` (one `impl TimeChimpApp` block per concern); `view/` renders, one file per screen or tab, with shared building blocks in `view::widgets`. UI-free logic lives in `domain` (unit-tested), files in `store`, data loading in `net`.

**Layering rules**
- Keep logic testable without a window: form contents are worked out as plain data in `seed.rs`, submitted forms are snapshotted into plain structs and checked in `validate.rs`, figures and findings live in `domain`. `app/` only turns that into entities. Unit tests build API models with `test_support`.
- Session state (day, week, month, report, review, selection, base data) is private to `Session` and changes only through its methods. Assign settings only through `Session::set_settings` (it redoes the review analysis). Every `invalidate_report` goes with an `invalidate_review`.
- Views return "Not loaded" rather than stale data: `week_loaded()`, `report()` and `review_rows()` are `None` unless loaded for what's shown.
- Whatever covers the main screen is one `app::Overlay`; views change it only through `open_overlay`/`close_overlay`/`toggle_menu`/`close_menu`, never by assigning fields.
- **Backend** (`backend.rs`): the session talks to a `Backend` trait; `timechimp_api::Client` implements it by delegating, `mock::Mock` (`--mock`) is an in-memory account built to answer the way the API does. A new API call goes in the trait *and* in both. Only `main.rs` knows the mock's concrete type. The mock points `store` at `%TEMP%\rechimped-mock` (emptied on start); `store::override_dir` must run before any file path is worked out, and a later override panics rather than leave some files in the real folder.

**Async**: reqwest needs tokio, gpui has its own executor. All network work goes through `TimeChimpApp::run` (saves and deletes: `run_write`, so logout lets them finish and they block another save), which spawns on a global tokio runtime and applies the result in a callback. Results for an older `Session` (by `epoch`) are dropped; week and review loads also carry a generation counter so a response for a period already left is dropped. Keep both when adding loads.

**Config** (`%APPDATA%\rechimped\config.toml`, `store::config`): written back with `toml_edit`, changing values in place so the rest of the file and its comments stay. Keep that for every new write. Its I/O takes the path, so tests use a temp dir. While it fails to load (`Session::config_writable`), Settings won't open and nothing writes to it, since the defaults shown meanwhile would overwrite it; a reload of the fixed file lifts that. The Settings window edits it; `[excluded_tasks]` defaults to Intern → Verlof/Ziek while absent, and is written once it differs so clearing it sticks. Hidden projects and tasks are left out of the pickers only: their entries still show, and editing or copying one keeps its project and task.

**Keyboard**
- Browse-mode shortcuts are one `on_key_down` handler (`TimeChimpApp::on_key`) looking keys up in the pure, unit-tested keymap `nav::browse_command`; it only fires while the main view (not an input) has focus. Review has its own block in that keymap so Time-tab keys never leak into it. The Settings window likewise has `settings_key`.
- Forms, Settings and the login card are modal elements with key context `"Modal"`.
- `main.rs::bind_keys` overrides gpuikit, registered *after* `gpuikit::init` so it wins at equal depth: in `Input`, Tab/Shift-Tab move focus, Enter → `Confirm`, Escape → `Cancel`. gpuikit inputs aren't tab stops by default; `forms::text_input` sets `tab_stop(true)`.
- Field-specific keys (address suggestions, date field calendar, Recent picker) are bound in contexts like `AddressField > Input` and call `cx.propagate()` when they have nothing to do, so Enter/Escape still reach `Confirm`/`Cancel`.
- Form tab order is paint order: field order in `render_form` is the Tab sequence (time form: Start → End → Notes → Hours, on purpose).

**gpuikit gaps and workarounds** (don't "simplify" these back to gpuikit)
- No password masking: custom `password.rs`.
- `Combobox`'s input is private (no tab stop, no focus on open, no free text): the trip form's address field is custom.
- A focused `Calendar` draws its focus ring as a filled shadow, so the date field keeps the focus and drives the calendar via `Calendar::set_selected`; its month chevrons are invisible, so `render_date` overlays visible ones.
- `context_menu` only opens on right-click: dropdowns are custom (`TimeChimpApp.menu`, a `deferred` panel plus a window-wide backdrop).
- `Tabs` fills the selected tab: the tab strip is custom, underline only.
- `progress` draws Success in the accent colour: status bars use `view::widgets::meter`.
- `SelectState` can't change its options in place: changing a time form's project replaces its task select (and its subscription).
- `ButtonVariant` is only `Filled`/`Destructive`: wrap secondary buttons in `view::widgets::pop`, a screen's main action uses `view::widgets::primary_button`. Keep new buttons consistent.

**UI conventions and pitfalls**
- Theme (`main.rs::theme`): gpuikit controls size themselves from `theme.controls`, so tune sizes there rather than per element.
- Popups and forms are `deferred` overlays from `TimeChimpApp::modal`. An overlay's `absolute().inset_0()` must not be wrapped in an unpositioned div, or it collapses to zero height. `cx.toast_manager()` must stay rendered in the root.
- A gpui animation (`with_animation`) asks for each frame by notifying the view it is drawn in, which redraws that view and every ancestor. So the sweep bar is its own view and everything under it (toolbar, tab, footer) is the cached `view::cached::TabBody`, redrawn only when `TimeChimpApp` notifies: keep new animations out of `TimeChimpApp`'s own tree, and keep state changes going through `cx.notify()` on the app, or the cached part shows stale data.
- Effects (`fx.rs`) are built from gpui primitives (blurred shadows, gradient paths) since gpui has no shader hook. `Fx::end_frame` must stay the last child of the root: its canvas measures CPU time per frame.
- Scrolling lists use `view::widgets::with_scrollbar`; tables put their rows in `view::widgets::table_rows` so they scroll under a fixed header. Scrollbar state is in `scroll.rs` (pure geometry unit-tested).
- Hours are shown as `hh:mm` everywhere (`domain::hh_mm`), never as fractions.
- An entry is locked when its internal or client approval status is 1 or more (`domain::lock_status`), as in the web app; locked entries can be copied but not edited or deleted. Rejected (-1) entries stay editable.
- `Session::now` is passed into `seed` functions rather than read there, so they stay testable.
- On Linux with client-side decorations (GNOME/Wayland) `view/frame.rs` draws the title bar, shadow and resize strips; with server decorations the content is returned as is.

## Checking UI changes

Check all UI work against the mock: `target\debug\rechimped.exe --mock`; saving and deleting there is fine. On a work day it opens with the travel reminder: Esc first. Use the live backend only for API changes. Without `--mock`, the app with a stored session talks to the real account: avoid saving or deleting unless asked, and cancel dialogs with Esc.

To verify visually on Windows: start the exe, post input with `PostMessage` to the process's `MainWindowHandle` (a `WM_LBUTTONDOWN/UP` on an empty spot first, so the window takes key input; then `WM_KEYDOWN/UP` with VK codes), and capture with `PrintWindow`. Don't use `SendKeys`: it types into whatever window has the foreground. Stop the app before rebuilding, since a running exe blocks the linker.

- A posted Ctrl isn't seen as held (gpui reads the real key state), so Ctrl shortcuts can't be driven this way: click instead (Settings… is in the user menu under the name in the header).
- `PrintWindow` captures the frame too: a point in the screenshot is client point (x − 8, y − 31) for `WM_LBUTTONDOWN`/`WM_MOUSEMOVE` (`MK_LBUTTON` in wParam while dragging).

To verify visually on Linux (GNOME/Wayland): drive input with `ydotool` (its `ydotoold` service runs with a drop-in giving the socket `/tmp/.ydotool_socket` to the user; set `YDOTOOL_SOCKET=/tmp/.ydotool_socket`) and capture with `python3 scripts/screenshot-linux.py <out.png>` (the desktop portal; GNOME refuses `gnome-screenshot`). ydotool types into whatever window has focus, so start the app and let it take focus first; a click outside it hands focus elsewhere.

- Keys: `ydotool key <code>:1 <code>:0` with Linux keycodes (Esc 1, 1–4 are 2–5, ← 105, → 106, ↑ 103, ↓ 108). Ctrl does work here: `29:1 <code>:1 <code>:0 29:0`.
- Mouse: `--absolute` moves go through pointer acceleration, so a screen point (x, y) is `ydotool mousemove --absolute -x x/2 -y y/2` (measured, 1920×1080, default accel). Then `ydotool click 0xC0` (left click), `ydotool mousemove -w -x 0 -y -8` to scroll down.
- The screenshot is the whole screen, window decorations included; the portal saves to `~/Pictures` first, the script moves it out.

Measure performance with `--perf-log <file>` (any build, combines with `--mock`), not with screenshots: it writes a tab-separated line per frame, `t_ms` (since start), `cpu_ms` (render to end of paint, `Fx::end_frame`), `anim` (1 when `Fx` asked for the next frame: login, showcase; loads and gpui's own animations such as the sweep bar don't set it) and `view` (`Login`, or the tab plus `+Overlay` and `+Menu`, `TimeChimpApp::perf_view`). Group by `view` for medians and maxima. An idle screen writes nothing, so a run of lines while nothing moves means something keeps redrawing. F12 still shows the HUD, but it also turns on the aurora, so its figures include that.
