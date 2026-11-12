# Time review — plan

Implements [spec.md](spec.md). One commit per step, each leaving `cargo test` and
`cargo clippy --workspace --all-targets` clean. Paths are under `rechimped/src/`.

## Findings from reading the code

- The Time tab's table pieces are tied to the loaded week: `entry_row` reads the selection from
  `Session::selected_entry_id`, `click_entry` calls `Session::select_entry` (which also moves the
  day), and `edit_selected` / `copy_selected` / `ask_delete` / `row_actions` / `browse_command`
  decide between time entry and trip from `self.tab == Tab::Week`.
- `seed::time_form` puts a copy on `session.day()`, and `seed::new_slot` →
  `Session::next_start_time` only looks at the week's entries, so a copy from another week
  would start empty.
- `submit_form` and `delete_remote` always move the Time tab (`select_day(date)`) and reload
  the week.
- `cell` is single-line with an ellipsis; there is no wrapping variant yet.
- The Travel tab's day header is inline in `render_travel`.
- Report's month handling is spread over `step_period`, `go_today`, `shift_month` (nav.rs), the
  toolbar match (chrome.rs) and `reload` (loading.rs), each checking `Tab::Report`.
- `Session::reselect` already does "keep the selection, else the nearest row", which the review
  can reuse after a reload or delete.

## Step 1: findings and month weeks in `domain`

New `domain/review.rs`, UI-free and unit-tested:

- `has_notes(&TimeEntry) -> bool` (missing or whitespace-only is false).
- `note_key(&str) -> String`: lower-cased, trimmed, whitespace runs collapsed.
- `tag_mismatches(&[TimeEntry]) -> HashMap<i64, TagMismatch>`: groups entries with notes by
  (project id, project task id, `note_key`), compares sorted `tag_ids`, finds the single most
  common set; marks entries differing from it, or the whole group on a tie.
  `TagMismatch` holds what to print: either `Majority { others: usize, tags: Vec<String> }` or
  `Tie(Vec<(usize, Vec<String>)>)`, with a `Display`/`label()` giving
  `Tags differ: 3 others have WBSO` / `… have no tags` / `Tags differ: 1× WBSO, 1× no tags`.
  Tag names come from `tag_names`, sorted.
- `Findings { no_notes: usize, tags_differ: usize }` plus `has_finding(entry, &mismatches)`,
  used by the summary line and F / Shift+F.

In `domain/week.rs`, next to `weeks_up_to`: `weeks_of_month(month) -> Vec<NaiveDate>` (the
Mondays from the week of the 1st to the week of the last day).

Tests: as listed in the spec (grouping, case/whitespace, other project/task, entries without
notes left out, tag order, majority, tie, labels, all-equal groups; month weeks for a month
starting on a Monday, ending on a Sunday, spanning six weeks).

## Step 2: shared table pieces

- `view/widgets.rs`:
  - `wrap_cell(width, content)`: like `cell` but wrapping (no `whitespace_nowrap` /
    `text_ellipsis`).
  - `day_header(theme, id, date, summary)`, moved out of `render_travel` (which then uses it,
    unchanged on screen).
- `view/week.rs` `entry_row` takes a small options struct instead of reading the session:
  `EntryRow { selected: bool, wrap: bool, tag_note: Option<String> }`. The Notes cell shows
  `No notes` in `theme.warning()` when `!has_notes(e)`; with `wrap` the Notes and Tags cells use
  `wrap_cell`; `tag_note` is drawn under the tags in the warning colour. The Time tab passes
  `wrap: false, tag_note: None`, so it only gains the `No notes` marker.

Visible change on the Time tab: the `No notes` marker. Check it by eye (see Verification).

## Step 3: review state in `Session` and loading in `net`

- `net::load_month(client, user_id, month) -> Result<Vec<TimeEntry>>`: `load_weeks` over
  `weeks_of_month`, keep dates within the month, sort by (date, start).
- `Session` gets a `Review` struct (private fields, as with the week):
  `month` (first of month, default the month before today), `loaded: Option<NaiveDate>`,
  `entries`, `selected: Option<i64>`, `generation`.
  Methods: `review_month()`, `set_review_month(day)`, `begin_review_load() -> (u64, NaiveDate)`,
  `is_latest_review_load(gen)`, `apply_review(month, entries, select_id)` (reselect via
  `reselect`), `review_entries() -> Option<&[TimeEntry]>` (None unless loaded for the month
  shown, like `report()`), `selected_review_entry()`, `select_review_entry(id)`,
  `move_review_selection(delta)`, `next_review_finding(direction)` (wraps; uses
  `domain::review`). Moving to another month clears `entries` at once, as weeks do.
- `Session::next_start_time(date)` also looks in the review entries when `date` isn't in the
  loaded week, so a copy from the review starts where that day's last entry ended.

Tests in `session.rs`: default month, stepping clears entries, stale generation and other month
dropped, selection kept / nearest after reload, finding navigation wraps both ways,
`next_start_time` from review entries.

## Step 4: the tab, month navigation and keys (`app/`)

- `Tab::Review` (label "Review"), `Tab::ALL` of four; `render_main` dispatches to
  `render_review` (stub until step 5 is fine within the same commit, or do steps 4 and 5 as one
  commit if the stub is awkward).
- Month generalisation in `nav.rs`: `shown_month() -> Option<NaiveDate>` (Report / Review) and
  `set_shown_month(day)` which sets that tab's month and starts its load (`load_report` or
  `load_review`). `step_period`, `go_today` and `shift_month` use them instead of checking
  `Tab::Report`. The toolbar's Report arm becomes `Tab::Report | Tab::Review`, reading
  `shown_month()`; both get the Reload button.
- New `app/review.rs`: `load_review(select_id)` through `run` with the generation check, like
  `load_week`. Called from `switch_tab` (when not loaded), `set_shown_month`, `reload` (joined in
  like the report when the Review tab is shown) and after saves/deletes from the review.
- Selection by tab: `TimeChimpApp::selected_time_entry()` returns the review's selection on the
  Review tab and the week's otherwise. `edit_selected`, `copy_selected`, `ask_delete` (time
  entry on Week | Review, trip on Travel), `browse_command`'s `has_entry` and `click_entry`
  (selects in the review on the Review tab) go through it. `row_actions` shows Copy on
  Week | Review.
- Copy from the review: `seed::time_form` gets the date for a new/copied entry from its caller
  (`session.day()` on the Time tab, the original's date on the Review tab) instead of reading
  `session.day()` itself; `gap_form` passes its date.
- After a save or delete started on the Review tab (captured as `from_review` when it starts,
  not read when it ends): don't `select_day`; reload the week only if it holds the saved date or
  held the entry; `invalidate_report`; `load_review(Some(id))` (or `None` after a delete). Saves
  and deletes from the Time tab behave as now.
- Keymap (`browse_command`, unit-tested): `4` → Review; ←/→, h/l → `ShiftMonth` on Report |
  Review; on Review: ↑/↓, j/k → `MoveSelection`, `f` → `NextFinding(1)`, Shift+F →
  `NextFinding(-1)`, Enter / `e` → Edit, `c` → Copy, `d` / Delete → Delete, no A/N. Existing
  tests for the other tabs stay; "three tabs" becomes "four tabs".
- `on_key` routes `MoveSelection` to the review on that tab and handles `NextFinding`.

## Step 5: the Review view

New `view/review.rs`, `render_review(theme, cx)`:

- Not loaded: the `table_card` with `empty_message` ("Loading…" while busy, else
  `NOT_LOADED`), as other tabs do. Loaded but empty: "No time registered this month."
- Summary line above the card: counts from `domain::review` (`4 without notes · 2 with
  different tags`, zero counts left out) in the warning colour, or `No findings` in
  `theme.success()`.
- `table_card` → `entry_header` → `table_rows("review-rows", …)`: per day (`chunk_by` date) a
  `day_header` with the day's `hh_mm` total, then `entry_row` with `wrap: true`, `selected` from
  the review selection and `tag_note` from `tag_mismatches` (computed once per render).
- Footer hints for Review: ←/→ month, T this month, F finding, E edit, C copy, D delete;
  `1–3` → `1–4` tabs.

## Step 6: docs

- `CLAUDE.md`: the Review tab (`app/review.rs`, `view/review.rs`, `domain::review`), the shared
  `EntryRow` options and `day_header`, month generalisation, `selected_time_entry`, the
  `from_review` save path, `weeks_of_month` / `load_month`.
- `README.md` feature list: a **Review** bullet; `docs/release-readme.txt` kept in step.
- `features/time-review/plan.md`: mark done.

## Verification

- `cargo test`, `cargo clippy --workspace --all-targets` after every step.
- Visual check on Windows per CLAUDE.md (PostMessage + PrintWindow, real account, **no saving or
  deleting**; cancel forms with Esc): Time tab with an entry without notes; Review tab on the
  previous month (summary line, day headers, wrapped notes/tags, a tag mismatch if the data has
  one), ‹ › and ←/→ months, F / Shift+F, E opening the edit form over the review and Esc
  returning to it, C opening a copy dated on the original's day.

## Risks

- Wrapped rows make `table_rows` taller and variable in height; fine for a month (a few hundred
  rows at most), but if scrolling is slow, revisit with a `uniform_list`-style approach later.
- A month needs 5–6 week requests (`MAX_CONCURRENT_REQUESTS` isn't applied to `load_weeks`; it
  was 5 for the chart). Acceptable; the same endpoint the Time tab uses.
