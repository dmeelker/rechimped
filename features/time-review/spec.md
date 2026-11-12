# Time review — spec

Builds on [intent.md](intent.md).

## Summary

A fourth tab, **Review**, shows one calendar month of the user's time entries on a single
scrollable screen, grouped per day. It is drawn with the Time tab's own pieces (column header,
entry rows, row actions), so the only new layout is the grouping per day and a summary line.
Each row shows its notes and tags in full, so notes and WBSO tags can be checked by eye. Two
findings are marked: entries without notes, and entries whose tags differ from other entries with
the same project, task and notes. Entries can be edited, copied and deleted from the review, as
on the Time tab.

## Reuse first

Where the review needs something the app already has, it uses that component, generalised if
needed, rather than a copy. New code is limited to what doesn't exist yet: the review's state in
the session, the findings, the summary line, jumping between findings and the Review tab itself.

### Data: the report's request

The month is loaded with the request the Report tab already makes: `POST time/query`
(`Client::query_time`), the server-side grid the web app's report page uses. One paged request
returns the whole month (a month is ~100–150 rows; a page is 500), instead of one request per
week. Its rows (`TimeReportRow`) carry everything the list and the findings need: id, date,
start–end, hours, customer, project and task (names and ids), billable, notes, and the tags as a
comma-separated string of names (`"wbso_tournament"`, `null` for none).

The request gets a server-side user filter, `"UserId": {"values": [id], "filterType": "set"}`
next to the `Date` filter (verified against the live API: the user's id returns their rows,
id 0 returns none). The Report tab benefits too; the client-side `retain` on user id stays as a
safeguard.

The rows lack what the time form needs (tag ids, project-task id, pause details), so edit and
copy first fetch the full entry with `Client::time_entry(id)`, which exists already.

### From the Time tab

- The column header (`entry_header`) and entry row (`entry_row`): Time, Hours, Customer,
  Project / Task, Notes, Tags, row actions. The row draws a small display struct that both a
  week entry and a report row convert into, so the same row serves both tabs.
- The row actions (Edit / Copy / Delete in the selected row) and the click handling (click
  selects, double-click edits).
- The time form for edit and copy, and the shared delete confirmation.

### From the Travel and Report tabs

- **Day headers**: the Travel tab's per-day group header (weekday and date, then a muted
  summary) becomes a shared widget, used by Travel (`2 trips · 34 km`) and Review (the day's
  total, `08:00`).
- **Month stepping**: the Report tab's month toolbar ("This month", ‹ ›, title, Reload) and its
  month keys (←/→, h/l, T) are generalised over "the month this tab shows", so Review uses the
  same code with its own month.
- **Loading**: the time half of `net::load_report` becomes a shared function that both tabs use.
- **Scrolling to the selection**: the Settings window's `ScrollHandle` + `scroll_to_item`
  pattern.
- `table_card`, `table_rows`, `empty_message` and `NOT_LOADED` as on every other tab.

### Change to a shared piece

- **Empty-notes marker**: an entry without notes (missing or whitespace only) shows `No notes`
  on the row's findings line (see Layout). It is part of the shared row, so the Time tab shows
  it too. (The tags finding needs the whole month, so it shows on the Review tab only.)

Only on the Review tab (as options of the shared pieces, not separate versions):

- **Full notes and tags**: the Notes and Tags cells wrap instead of being cut off; the Time tab
  keeps its single-line rows.
- **No gap or overlap markers** between rows: those are noticed day to day on the Time tab.

## Tab and month

- The tab strip becomes Time, Travel, Report, **Review**; key `4` switches to it.
- The Review tab has its own month, separate from Report's.
- It opens on the **previous** month (the usual month to hand in), worked out when the session
  starts; an app left open across a month boundary keeps it until stepped. "This month" goes to
  the current month and is disabled there.
- The toolbar works as on Report: "This month", ‹ › one month at a time, title `October 2026`,
  Reload; ←/→ (and h/l) step a month too. There is no "+ Add" button.

## Data

- A month's rows are the user's time report rows for the 1st to the last day, sorted by
  (date, start).
- Loaded when the tab is shown and its month isn't loaded, when the month changes, on reload
  (R), and after a save or delete made from the review.
- The loaded month is forgotten (like the report) after **any** save or delete and on reload,
  wherever made, so the review never shows data older than an edit on the Time tab; it reloads
  when next shown.
- Results for a month no longer shown, or for an older session, are dropped (a generation
  counter, as for week loads, plus the session epoch).
- Switching to the tab before the startup reload has finished: the reload loads the review
  once the user is known, so the tab doesn't stay on "Not loaded".
- While loading with nothing to show: "Loading review…". After a failed load: "Not loaded" (as
  on the other tabs); the error goes to the toast and footer as usual. Loaded but empty: "No time
  registered this month."
- Nothing is stored; findings are worked out from the loaded rows.

## Findings

Pure functions over the month's rows, in `domain`, unit-tested.

### No notes

- An entry whose notes are missing or only whitespace: `No notes`.
- Except leave: entries of the excluded tasks (`excluded_tasks` in the config, the Billability
  page's unchecked tasks; Intern → Verlof and Ziek by default) usually have no notes and are
  never flagged, on the Time tab either. Matched by project and task name as
  `domain::week::is_excluded` does.

### Tags differ

Meant to catch a forgotten or misplaced WBSO tag without the app knowing which work qualifies.

- Rows of the month are grouped by **project id, task id and notes**. Notes match ignoring case,
  leading/trailing whitespace and runs of whitespace (`Refactor  login ` = `refactor login`);
  other differences count (`Single Sign On` and `Single sign-on` are different notes). Rows
  without notes are left out (they already have their own finding).
- A row's tags are its comma-separated tag names, trimmed, empty parts dropped, compared as a
  set ignoring case and order (no tags is a set too).
- In a group whose rows don't all have the same tags, the **odd ones out** are marked: the rows
  whose tags differ from the single most common set. When no set is more common than all others
  (a tie: 1/1, 2/2/1), every row of the group is marked.
- The marker names what the others have: `Tags differ: 3 others have wbso_tournament` (or
  `… have no tags`). On a tie it lists the variants, by count descending, then alphabetically
  with "no tags" last: `Tags differ: 1× wbso_tournament, 1× no tags`; several tags in one
  variant are joined with " + ".
- Only within the month shown; earlier months aren't compared.

Example from real data (October 2026): two entries with the notes "Test performance" on the
same project and task, one tagged `wbso_tournament` and one without tags: a tie, both marked.

## Layout

- A summary line above the table: `4 without notes · 2 with different tags` (warning colour,
  zero counts left out), or `No findings` (success colour).
- One table card (`table_card` + `table_rows`, scrolling under the fixed shared header), like the
  Time and Travel tabs.
- Per day with entries the shared day header: weekday and date and the day's total in `hh:mm`.
  Days without entries aren't listed.
- Under each day header, that day's entry rows, with notes and tags wrapping.
- A row with a finding gets a warning stripe at its left edge and, under the row's cells, one
  findings line spanning the whole row from its left edge: all of the row's findings in one
  label in the warning colour, joined with " · " (`No notes`,
  `Tags differ: 1× wbso_tournament, 1× no tags`). The Notes cell stays empty when there are no
  notes. Stripe and line stay visible on non-billable (muted, tinted) and selected rows, since
  they set their own colour. The Time tab shows the same line for `No notes`, as the row is
  shared.
- The selected row is kept in view: moving the selection with keys scrolls the list to it.

## Interaction

- Click selects a row; double-click opens its edit form.
- Keys (browse mode, Review tab):
  - ↑/↓ (j/k): previous/next entry (day headers are skipped).
  - F / Shift+F: next / previous entry with a finding, wrapping around.
  - Enter or E: edit the selected entry; C: copy; D or Delete: delete (with the usual
    confirmation).
  - ←/→ (h/l): previous/next month; T: this month; R: reload; 1–4: tabs; Shift+L: log out;
    Ctrl+, settings; Ctrl+Q quit.
  - Nothing else: `[`, `]`, Shift+←/→ and A/N do nothing on this tab (they must not reach the
    Time tab's week or open "add").
- Edit and copy fetch the full entry (busy label "Loading…"), then open the existing time form
  over the review:
  - Edit saves the entry as on the Time tab.
  - Copy opens a new entry for the **same date** as the original, filled from it, starting where
    that day's last entry ended (taken from the review's rows when the day isn't in the loaded
    week).
  - If the fetch fails, the error is shown as usual and no form opens.
- Saving an entry whose project is no longer in the user's project list works as on the Time tab:
  the customer is found from the review's rows as well as the week's.
- After a save or delete started on the review: the review month reloads, the selection stays on
  the saved entry (or moves to the nearest row after a delete), and the toast reports it as usual.
  The Time tab's day is **not** moved; its week is reloaded only if it contains the entry's old
  or new date. Saves and deletes started on the Time tab behave as now.
- Cancel/Escape on a form or dialog returns to the review unchanged.
- **Billed entries are locked** (on the Review and the Time tab alike). TimeChimp's `status` 3
  marks a billed entry (verified: older months have it on billable client work only, the
  current month on nothing; `statusIntern` matches on the full entry). Such a row shows a small
  lock icon with the tooltip "Billed"; its row actions offer only Copy; Edit (E, Enter on the
  review, double-click, the Detail popup's Edit button) and Delete (D) show a toast "Billed
  entries can't be changed." instead. Copying one as a new entry works as usual. Findings on
  billed entries still show and count (they may still matter, e.g. for WBSO).

## Not in this feature

Short days (hours per day; the Time tab shows those), gaps and overlaps (noticed day to day on
the Time tab), missing trips, deciding automatically which work qualifies for WBSO, note-quality
checks beyond empty notes, adding entries from the review, approval, and anything persisted
(dismissed findings, a "reviewed" mark). Edits made from the review don't feed the Recent
picker until the next reload.

## Tests

- `domain` unit tests:
  - no notes: none, empty, whitespace only; excluded (leave) tasks exempt;
  - note matching: case and whitespace ignored, punctuation not;
  - tag parsing: `null`, one, several, spaces, empty parts, case;
  - tags differ: grouping (same notes in another project or task is another group; rows without
    notes left out), tag order ignored, a single majority (3/1/1: only the 1s marked), ties
    (1/1 and 2/2/1: all marked), all-equal groups unmarked, the marker texts and their order;
  - the summary label;
  - the rows-to-lines layout (day headers and entries) used for drawing and scrolling.
- API: the time report request body with the user filter.
- Keymap: `nav::browse_command` on the Review tab (F / Shift+F, ←/→ month, Enter/E/C/D, `4`),
  and that `[`, `]`, Shift+←, A and N return nothing there while Shift+L still logs out.
- Session: default month (previous month), stepping clears the rows, stale generation and other
  month dropped, invalidation, selection kept / nearest after reload, finding navigation wraps
  both ways, next start time from review rows, customer lookup from review rows.
- The rule "reload the week after a review save/delete if it holds the old or new date".
- Seed: copy dated on the given date (the existing seed tests updated for the new parameter).
