# Rechimped

A small desktop app for viewing and editing your own TimeChimp time entries and travel, without
going through the web app.

- **Time**: your week day by day, with add, edit, copy and delete, a "Recent" picker, preset
  entries, and gaps and overlaps between entries marked. It also shows a weekly billable
  percentage, with a chart of the last five weeks.
- **Travel**: your trips per week, with add, edit and delete, preset trips and address suggestions.
- **Report**: a monthly overview of your hours.
- Keyboard shortcuts throughout; the footer lists them.

You log in with your normal TimeChimp account (MFA is supported). The app keeps a session token
and a `config.toml` with your settings and presets in `%APPDATA%\rechimped\`. Delete that
folder to remove everything it stored.

## Tech

Written in Rust, as a Cargo workspace of two crates:

- `timechimp`: an async client for TimeChimp's web API (`reqwest`, `tokio`, `serde`), with
  login through AWS Cognito using SRP, so your password is never sent as-is.
- `timechimp-gui`: the desktop app (`rechimped.exe`), built with [gpui](https://www.gpui.rs/),
  the GPU-accelerated UI framework from the Zed editor, and the
  [gpuikit](https://crates.io/crates/gpuikit) component library.

Build with `cargo build --release -p timechimp-gui` (Rust 1.93 or newer). It has been built and
used on Windows only.

## Written by Claude Code

This app was written with [Claude Code](https://claude.com/claude-code), Anthropic's AI coding
tool.

## Disclaimer

Rechimped is **not affiliated with, endorsed by or in any way related to TimeChimp**. It uses
TimeChimp's undocumented internal web API, the one their own web app uses, which can change or
break at any time.

The software is provided **"as is", without warranty of any kind**, express or implied, including
but not limited to the warranties of merchantability, fitness for a particular purpose and
non-infringement. In no event shall the authors be liable for any claim, damages or other
liability arising from its use. It edits real data in your TimeChimp account: use it at your own
risk.
