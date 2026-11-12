# Time review — plan

**Done**: all six steps are implemented (steps 1–5 as commits on `time-review`, step 6 the docs).
What follows is kept as the record. Where the code ended up differing from it:

- The findings are one line under the row's cells (all of a row's findings joined by " · ",
  `No notes` included), not a marker in the Notes cell plus a note under the tags; the spec's
  Layout was changed to match before step 5.
- `EntryRow` is `{ selected, wrap, tags_differ: Option<String> }`: no `actions` flag (the row
  actions are always on) and no `tag_note`/`findings` list; the row works out No notes itself.
- Leave is exempt from No notes (`domain::review::lacks_notes` via
  `domain::week::is_excluded_task`), added to the spec during step 4, on the Time tab too.
- Also added on the way: `Session::selected_review`, `reload_after_review_change` (app/save.rs),
  and `open_time_form_on` taking an `Origin`.

Implements [spec.md](spec.md). One commit per step. `rechimped` is a binary crate, so an item
added before anything calls it is a dead-code warning: **each step only adds what it also uses**,
and leaves `cargo test` and `cargo clippy --workspace --all-targets` clean. Paths are under
`rechimped/src/` unless they start with `timechimp-api/`.

## Findings from reading the code

- `entry_row` (view/week.rs) reads the selection from `Session::selected_entry_id`, and
  `click_entry` (app/nav.rs) calls `Session::select_entry`, which also moves the Time tab's day.
- `edit_selected` and `add` (app/time_form.rs) test `Tab::Travel` and otherwise use the week's
  selection, so on a new tab they'd act on the week's entry. `copy_selected` doesn't look at the
  tab. `ask_delete` (app/delete.rs), `row_actions` (view/chrome.rs) and `browse_command`
  (app/nav.rs) test `Tab::Week`.
- `submit_form` (app/save.rs) calls `select_day(date)` and reloads the week after every save;
  `delete_remote` (app/delete.rs) only reloads the week. Both call `invalidate_report`, as does
  `reload` (app/loading.rs).
- `seed::time_form` dates a new or copied entry on `session.day()`, and `seed::new_slot` →
  `Session::next_start_time` looks only at the week's entries.
- `validate::date_and_customer` falls back to `session.entries()` for the customer when the
  project isn't in `projects()`.
- `browse_command` returns `None` for every tab but Week/Travel at
  `(_, _) if !matches!(tab, Tab::Week | Tab::Travel)`; the arms after it include `[`/`]`/Shift+←/→
  (`ShiftDay(±7)`) and A/N (`Add`).
- Report's month handling is spread over `switch_tab`, `step_period`, `go_today`, `shift_month`
  (app/nav.rs), the toolbar and footer matches (view/chrome.rs) and `reload` (app/loading.rs).
- `reselect(selected, wanted, old_ids, new_ids)` (session.rs) is a free function giving "keep
  the selection, else the nearest row"; it needs the old ids, so call it before replacing rows.
- `cell` is single-line with an ellipsis. The Travel tab's day header is inline in
  `render_travel`. Scroll-to-selection exists only in the Settings window (`ScrollHandle`,
  `track_scroll`, `scroll_to_item` in forms/settings.rs and view/settings.rs).
- `query_time` filters on date only; `load_report` drops other users' rows client-side.

## Step 1: user filter on the time report, shared loader

- `timechimp-api/src/client.rs`: `report_query(from, to, user_id: Option<i64>, rows)` adds
  `"UserId": {"values": [id], "filterType": "set"}` to `filterModel` when given;
  `query_time(user_id, from, to, rows)` passes it; `query_mileage` passes `None` (its filter is
  unverified). Unit test of the body with and without the user. Update `examples/smoke.rs`.
- `net.rs`: `load_time_rows(client, user_id, from, to) -> Vec<TimeReportRow>` (paged, keeps the
  user-id `retain` as a safeguard, sorted by (date, start)); `load_report` uses it.

The Report tab behaves as before, with less data over the wire.

## Step 2: shared row pieces and the No-notes marker

- `domain/review.rs` (new): `has_notes(Option<&str>) -> bool`, tested.
- `view/week.rs`:
  - `EntryLine`: the display fields of a row (id, start–end, hours, customer, project, task,
    notes, tag names, billable), `From<&TimeEntry>`.
  - `entry_row(theme, index, &EntryLine, EntryRow { selected }, cx)`: no session reads. Notes
    cell shows `No notes` in `theme.warning()` when `!has_notes`; a row with a finding gets a
    3px warning stripe at its left edge (the `junction_row` bar), with the warning text setting
    its own colour so it survives the muted non-billable text and the selected background.
  - `render_day` / `entry_rows_with_gaps` build `EntryLine`s and pass the week's selection.
- `view/widgets.rs`: `day_header(theme, id, date, summary)` moved out of `render_travel`, which
  uses it (unchanged on screen).

Visible change: the Time tab marks entries without notes.

## Step 3: a read-only Review tab

- `domain/review.rs`: `tag_names(Option<&str>) -> Vec<String>` (split on commas, trim, drop
  empty); `Line` (`Day { date, total }` / `Entry(index)`) and `lines(&[TimeReportRow])`, used for
  drawing and for scrolling to a row. Tests.
- `EntryLine: From<&TimeReportRow>` (tags via `tag_names`).
- `session.rs`: private `Review` state: `month` (first of the month before `today` at
  `Session::new`), `loaded: Option<(NaiveDate, Vec<TimeReportRow>)>`, `selected: Option<i64>`,
  `generation`. Methods: `review_month`, `set_review_month` (clears loaded rows at once),
  `begin_review_load`, `apply_review(generation, month, rows, select_id)` (drops stale ones,
  `reselect` before replacing), `review_rows() -> Option<&[TimeReportRow]>` (None unless loaded
  for the month shown), `invalidate_review`, `selected_review_id`, `select_review`,
  `move_review_selection`. Tests as in the spec.
- `app/mod.rs`: `Tab::Review` ("Review"), `Tab::ALL` of four; `TimeChimpApp.review_scroll:
  ScrollHandle`.
- `app/review.rs` (new): `load_review(select_id)` through `run` ("Loading review…"), like
  `load_week`.
- `app/nav.rs`:
  - `shown_month() -> Option<NaiveDate>` (Report / Review) and `set_shown_month(day)` (sets it,
    loads that tab); `step_period`, `go_today`, `shift_month` and `switch_tab` (load when not
    loaded) use them instead of testing `Tab::Report`.
  - `click_entry` and `MoveSelection` select in the review on that tab, then
    `review_scroll.scroll_to_item(line index)`.
  - `browse_command`: a Review block placed after Shift+L / T and before the
    `!matches!(tab, Week | Travel)` guard: ←/→, h/l → `ShiftMonth`; ↑/↓, j/k →
    `MoveSelection`; everything else `None`. Tests, including `[`, `]`, Shift+←, A, N → `None`
    and Shift+L → `Logout` on Review; "three tabs" → "four tabs".
- `app/loading.rs`, `app/save.rs`, `app/delete.rs`: `invalidate_review` next to every
  `invalidate_report`. `reload`, once the base is in: if the Review tab is shown and its month
  isn't loaded, `load_review(None)` (also covers pressing 4 during startup).
- `view/chrome.rs`: the toolbar's Report arm becomes `Report | Review` (reads `shown_month`,
  "This month", Reload); `render_main` dispatches to `render_review`; footer hints for Review
  (←/→ month, T this month, ↑/↓ select) and `1–4` tabs.
- `view/review.rs` (new): `table_card` → `entry_header` → `table_rows("review-rows", …)
  .track_scroll(&self.review_scroll)` with a `day_header` (day total) per `Line::Day` and an
  `entry_row` per `Line::Entry`. `EntryRow` gains `wrap` (Notes and Tags in a wrapping cell,
  `wrap_cell` in widgets.rs) and `actions` (false here: the row buttons would act on the week's
  selection until step 5). States: "Loading review…", `NOT_LOADED`, "No time registered this
  month."

## Step 4: tags differ, summary line, F / Shift+F

- Leave is exempt from No notes: `domain::week::is_excluded` gets a name-based core
  (`is_excluded_task(project, task, excluded)`) that both `TimeEntry` and report rows use;
  `entry_row` only flags a row when its task isn't excluded (Time tab too), and the No-notes
  count uses the same rule.

- `domain/review.rs`: `note_key`, `TagMismatch` (`Majority { others, tags }` / `Tie(variants)`)
  with `label()`, `tag_mismatches(&[TimeReportRow]) -> HashMap<i64, TagMismatch>`,
  `Findings { no_notes, tags_differ }` with `of(rows, mismatches)` and `label()`,
  `has_finding(row, mismatches)`. Tests as in the spec (majority 3/1/1, ties 1/1 and 2/2/1, label
  order, grouping, case/whitespace/punctuation).
- `EntryRow.tag_note: Option<String>`, drawn under the tags in the warning colour; the stripe
  shows for it too.
- `view/review.rs`: summary line above the card; mismatches computed once per render.
- `session.rs`: `next_review_finding(direction)` (wraps), tested.
- `app/nav.rs`: `Command::NextFinding(isize)`: `f` + Shift → `-1`, `f` → `1` in the Review
  block; scrolls to the row. Footer hint "F finding".

## Step 5: edit, copy and delete from the review

- First, the findings line (spec, Layout): `EntryRow.tag_note` becomes `findings: Vec<String>`
  (or one label); `entry_row` becomes a column: the existing cells in a row, then, when there
  are findings, one full-width line with them joined by " · " in the warning colour, starting at
  the row's left edge. "No notes" moves from the Notes cell to that line.

- `seed::time_form(session, entry, copy, date, now)`: the date for a new or copied entry comes
  from the caller (`session.day()` on the Time tab, the original's date on the review);
  `gap_form` passes its own. Existing seed tests updated.
- `session.rs`: `next_start_time` uses the review rows when `date` is outside the loaded week.
  Test.
- `validate.rs`: `date_and_customer` also looks in the review rows. Test.
- `domain/week.rs`: `touches_week(week_start, dates: &[NaiveDate]) -> bool`, tested.
- `forms.rs`: `Form.origin: Origin` (`Week` / `Review { date: Option<NaiveDate> }`, the
  original's date for an edit), set when the form opens.
- `app/review.rs`: `open_review_entry(copy)`: `run("Loading…", client.time_entry(id))`, then
  opens the time form (edit, or copy dated on the entry's date) with `Origin::Review`, unless an
  overlay opened meanwhile.
- `app/time_form.rs`: `edit_selected` / `copy_selected` go to `open_review_entry` on Review;
  `add` returns on Review.
- `app/save.rs`: after a save with `Origin::Review`: no `select_day`; `load_week` only if
  `touches_week(week_start, [old, new])`; `load_review(Some(id))`. Origin `Week`: as now.
- `app/delete.rs`: `DeleteTarget::Time { id, review_date: Option<NaiveDate> }`; `ask_delete` on
  Review builds the label from the selected row; on success the same branching as saves.
- `app/nav.rs` Review block: Enter / `e` → Edit, `c` → Copy, `d` / Delete → Delete. Tests.
- `view`: drop `EntryRow.actions` (always on); `row_actions` shows Copy on Week | Review. Footer
  hints E edit, C copy, D delete.

## Step 6: docs

- `CLAUDE.md`: the Review tab (`app/review.rs`, `view/review.rs`, `domain::review`), `EntryLine`
  / `EntryRow` and `day_header`, `shown_month`, `Form.origin` and the review save/delete path,
  `load_time_rows` and the UserId filter, `invalidate_review`.
- `README.md` feature list: a **Review** bullet; `docs/release-readme.txt` kept in step.
- `features/time-review/plan.md`: mark done.

## Verification

- `cargo test` and `cargo clippy --workspace --all-targets` after every step.
- Visual check on Windows per CLAUDE.md (PostMessage + PrintWindow, real account, **no saving or
  deleting**; cancel forms with Esc), after steps 2, 3, 4 and 5:
  - step 2: Time tab with an entry without notes (stripe, marker), Travel day headers unchanged;
  - step 3: Review on October 2026 (day headers, wrapped notes and tags), ‹ › and ←/→ months,
    ↑/↓ scrolling the selection into view, `[` / `]` / A doing nothing;
  - step 4: summary line, the "Test performance" pair on 1 October marked as a tie, F / Shift+F;
  - step 5: E opening the edit form over the review and Esc returning to it, C opening a copy
    dated on the original's day with the start after that day's last entry.

## Risks

- Wrapped, variable-height rows in a plain scroll div: fine for a month (~150 rows); if scrolling
  gets slow, revisit with a list that only draws what's visible.
- Billed entries (status 3) are locked in the app since step 7, so TimeChimp's own refusal is
  never reached; untested against the live API, since testing doesn't save.
- The UserId filter on `time/query` was verified once against the live API; `query_mileage`
  keeps filtering client-side.

## Step 7: lock billed entries (added after step 6)

- `domain`: `is_billed(status: i32) -> bool` (`status == 3`), tested.
- `EntryLine.billed` (from `TimeEntry.status` and `TimeReportRow.status`); `entry_row` draws a lock
  icon with a "Billed" tooltip; `row_actions` gets whether the selection is billed and then only
  offers Copy.
- `edit_selected` and `ask_delete` refuse a billed selection (Time tab: `selected_entry().status`;
  Review: `selected_review().status`) with a toast "Billed entries can't be changed."; this covers
  E, Enter, D, double-click and the Detail popup's Edit button. `copy_selected` is unchanged.
- CLAUDE.md and README.md: a line each.

Later: the lock was widened to the web app's edit rule (its approval status enum: -1 Rejected,
0 Open, 1 Pending approval, 2 Approved, 3 Invoiced, 4 Written off; locked when the internal or
client status is 1 or more), with `domain::ApprovalStatus` / `lock_status` replacing
`is_billed`, the status named in the lock's tooltip and the refusal toast, and a "Rejected"
finding for entries that need fixing.
