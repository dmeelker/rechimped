# Rechimped

A small desktop app for viewing and editing your own TimeChimp time entries and travel, without
going through the web app.

- **Time**: your week day by day, with add, edit, copy and delete, a "Recent" picker, preset
  entries, and gaps and overlaps between entries marked. It also shows a weekly billable
  percentage, with a chart of the last five weeks.
- **Travel**: your trips per week, with add, edit and delete, preset trips and address suggestions.
- **Report**: a monthly overview of your hours.
- Keyboard shortcuts throughout; the footer lists them.

## Goals

- **A fast and responsive experience**: a native, GPU-rendered app instead of a web page. It
  starts quickly, switches tabs and weeks without waiting on page loads, and stays idle when
  nothing is happening.
- **Extra usability features**: the things you do many times a week should take few keystrokes.
  Where the web app makes you click through forms, Rechimped aims to remember, suggest and
  prefill.

## Improvements over the web version

- Everything can be done from the keyboard, with shortcuts listed in the footer.
- A "Recent" picker fills a new entry from what you booked in the last five weeks.
- Preset time entries and trips, saved from a form and started from the "+ Add" dropdown.
- Lenient time input: `9`, `930` or `12:30`, and an end time as a duration such as `+1h30`.
- Gaps and overlaps between consecutive entries of a day are marked.
- A weekly billable percentage against your own goal, with a chart of the last five weeks.
- Hide projects and tasks you don't use, and limit which tags each project offers, so forms only show
  what is relevant.
- Choose your work days; the week view shows only those (plus today).
- Your settings live in a plain `config.toml` you can read and edit.

You log in with your normal TimeChimp account (MFA is supported). The app keeps a session token
and a `config.toml` with your settings and presets in `%APPDATA%\rechimped\`. Delete that
folder to remove everything it stored.

## Tech

Written in Rust, as a Cargo workspace of two crates:

- `timechimp-api`: an async client for TimeChimp's web API (`reqwest`, `tokio`, `serde`), with
  login through AWS Cognito using SRP, so your password is never sent as-is.
- `rechimped`: the desktop app (`rechimped.exe`), built with [gpui](https://www.gpui.rs/),
  the GPU-accelerated UI framework from the Zed editor, and the
  [gpuikit](https://crates.io/crates/gpuikit) component library.

Build with `cargo build --release -p rechimped` (Rust 1.93 or newer). It has been built and
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
