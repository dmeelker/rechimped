# Time review — spec

Builds on [intent.md](intent.md).

## Summary

A fourth tab, **Review**, shows one calendar month of the user's time entries on a single
scrollable screen, grouped per day. It is drawn with the Time tab's own pieces (column header,
entry rows, row actions), so the only new layout is the grouping per day and a summary line.
Each row shows its notes and tags in full, so notes and WBSO tags can be checked by eye. Two
findings are marked: entries without notes, and entries whose tags differ from other entries with
the same project, task and notes. Entries can be edited, copied and deleted from the
review, as on the Time tab.

## Reuse first

Where the review needs something the app already has, it uses that component, generalised if
needed, rather than a copy. New code is limited to what doesn't exist yet: the month's entries
in the session, the findings, the summary line, jumping to the next finding (F / Shift+F) and the Review tab
itself.

### From the Time tab

Its table pieces are made to work on any list of entries, not just the loaded week:

- the column header (`entry_header`) and entry row (`entry_row`): Time, Hours, Customer,
  Project / Task, Notes, Tags, row actions;
- the row actions (Edit / Copy / Delete in the selected row) and the click handling (click
  selects, double-click edits);
- the time form for edit and copy, and the shared delete confirmation.

### From the Travel and Report tabs

- **Day headers**: the Travel tab's per-day group header (weekday and date, then a muted
  summary) becomes a shared widget, used by Travel (`2 trips · 34 km`) and Review (the day's
  total, `08:00`).
- **Month stepping**: the Report tab's month toolbar (Today, ‹ ›, title) and its month keys
  (←/→, h/l, T) are generalised over "the month this tab shows", so Review uses the same code
  with its own month.
- **Loading**: `net::load_weeks` (as used by the chart) fetches the weeks; the Mondays covering a
  month come from a helper next to `domain::week::weeks_up_to`. Loads go through `run` with the
  same stale-result rules as week loads.
- `table_card`, `table_rows`, `empty_message` and `NOT_LOADED` as on every other tab.

### Change to a shared piece

- **Empty-notes marker**: an entry without notes (missing or whitespace only) shows `No notes`
  in the warning colour in its Notes cell. It is part of the shared row, so the Time tab shows
  it too. (The tags finding needs the whole month, so it shows on the Review tab only.)

Only on the Review tab (as options of the shared pieces, not separate versions):

- **Full notes and tags**: the Notes and Tags cells wrap instead of being cut off (a flag on the
  shared row; the Time tab keeps its single-line rows).
- **No gap or overlap markers** between rows: those are noticed day to day on the Time tab.

## Tab and month

- The tab strip becomes Time, Travel, Report, **Review**; key `4` switches to it.
- The Review tab has its own month, separate from Report's.
- It opens on the **previous** month (the usual month to hand in). Today goes to the current month.
- The toolbar works as on Report: Today (disabled on the current month), ‹ › one month at a
  time, title `October 2026`; ←/→ (and h/l) step a month too.

## Data

- A month's entries are the time entries of the weeks covering it (the Mondays from the week of
  the 1st up to the week of the last day, fetched like the chart's weeks), filtered to dates in
  the month and sorted by (date, start). They're kept apart from the Time tab's week, with their
  own selection.
- Loaded when the tab is shown and its month isn't loaded yet, when the month changes, on reload
  (R), and after a save or delete. Results for a month no longer shown, or for an older session,
  are dropped (same rules as week loads).
- While loading with nothing to show: "Loading…". After a failed load: "Not loaded" (as on the
  other tabs); the error goes to the toast and footer as usual.
- Nothing is stored; findings are worked out from the loaded entries.

## Findings

Pure functions over the month's entries, in `domain`, unit-tested.

### No notes

- An entry whose notes are missing or only whitespace: `No notes`.

### Tags differ

Meant to catch a forgotten or misplaced WBSO tag without the app knowing which work qualifies.

- Entries of the month are grouped by **project, task and notes**. Notes match ignoring case,
  leading/trailing whitespace and runs of whitespace (`Refactor  login ` = `refactor login`).
  Entries without notes are left out (they already have their own finding).
- Tags are compared as sets (order doesn't matter; no tags is a set too).
- In a group whose entries don't all have the same tags, the **odd ones out** are marked: the
  entries whose tags differ from the most common set. When there is no single most common set
  (a tie, e.g. a group of two), every entry of the group is marked.
- The marker, in the warning colour under the tags, names what the others have:
  `Tags differ: 3 others have WBSO` (or `… have no tags`). On a tie it lists the variants:
  `Tags differ: 1× WBSO, 1× no tags`.
- Only within the month shown; earlier months aren't compared.

## Layout

- One table card (`table_card` + `table_rows`, scrolling under the fixed shared header), like the
  Time and Travel tabs.
- A summary line above the table: `4 without notes · 2 with different tags` (warning colour,
  leaving out a zero count), or `No findings` (success colour).
- Per day with entries the shared day header: weekday and date and the day's total in `hh:mm`.
  Days without entries aren't listed.
- Under each day header, that day's entry rows.

## Interaction

- Click selects a row; double-click opens its edit form.
- Keys (browse mode, Review tab):
  - ↑/↓ (j/k): previous/next entry (day headers are skipped).
  - F / Shift+F: next / previous entry with a finding (no notes or tags differ), wrapping
    around.
  - Enter or E: edit the selected entry; C: copy; D or Delete: delete (with the usual
    confirmation).
  - ←/→ (h/l): previous/next month; T: today's month; R: reload; 1–4: tabs.
  - No add (A/N) on this tab.
- Edit and copy use the existing time form over the review:
  - Edit saves the entry as on the Time tab.
  - Copy opens a new entry for the **same date** as the original, filled from it.
- After a save or delete from the review: the review month reloads, the selection stays on the
  saved entry (or moves to the nearest row after a delete), and the toast reports it as usual.
  The Time tab's day is **not** moved; its week is reloaded only if it contains the changed
  date. The report is invalidated as with any save.
- Cancel/Escape on a form or dialog returns to the review unchanged.

## Not in this feature

Short days (hours per day; the Time tab shows those), gaps and overlaps (noticed day to day on
the Time tab), missing trips, automatic WBSO or
note-quality checks, adding entries from the review, approval, and anything persisted
(dismissed findings, a "reviewed" mark).

## Tests

- `domain` unit tests:
  - no notes: none, empty, whitespace only;
  - tags differ: grouping (same notes in another project or task is another group; case and
    whitespace ignored; entries without notes left out), tag order ignored, majority vs odd
    ones out, ties marking all, the marker text, all-equal groups unmarked;
  - the summary counts.
- Month weeks: the Mondays covering a month (month starting on a Monday, ending on a Sunday,
  spanning six weeks).
- Keymap: `nav::browse_command` on the Review tab (F / Shift+F, ←/→ month, no A/N, `4`).
- Session: review month default (previous month), stepping, selection within the month,
  dropping a load for a month no longer shown.
