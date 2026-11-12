# Time review — intent

> Status: agreed. Next: specs, then a plan, then implementation.

## Problem

Before handing in a month's hours, mistakes in notes and tags slip through: entries without
notes, notes with a copy-paste mistake, and WBSO tags that are missing on qualifying work or
added to work that doesn't qualify. Today the only way to catch them is to page through the
Time tab day by day.

## Goal

A **Review** tab that shows a whole calendar month on one screen, so it can be checked once
before handing in, with the mistakes the app can detect marked and everything else laid out to
check by eye.

## What the review shows

- All of the month's time entries, grouped per day, with each row showing its notes and tags
  (so WBSO tags and odd notes can be checked by eye).
- Rows with a finding highlighted, with the reason.

## Findings (detected by the app)

- **Empty notes**: an entry without notes. Short notes are fine.
- **Tags differ**: entries with the same project, task and notes should carry the same tags.
  Where they don't, the odd ones out are marked. This catches a forgotten or misplaced WBSO tag
  without the app having to know which work qualifies.

## Left to the user's eyes (not automated)

- WBSO tags: only the user can tell which work qualifies. Apart from the consistency check
  above, the review makes tags visible on every row; it doesn't judge them.
- Pasted junk / copy-paste mistakes in notes: notes are shown in full; no heuristics.

## Acting on a finding

Fix without leaving the tab: an entry opens its edit form over the review, and after saving
(or cancelling) the review is back, recomputed.

## Scope

### In

- New Review tab next to Time, Travel and Report, one calendar month at a time, stepped with
  ‹ › and Today like the Report tab.
- The two findings above.
- Editing, copying and deleting entries from the review via the existing form and dialog.

### Out

- Short days and gaps between entries: missing time can't be filled in from memory at month's
  end anyway, and the Time tab already shows hours per day.
- Overlaps: noticed day to day on the Time tab.
- Missing trips.
- Deciding automatically which work qualifies for WBSO, and note-quality checks beyond empty
  notes.
- Submitting for approval, approving others' hours.
- Anything stored between sessions: no dismissing findings, no "month reviewed" mark. Findings
  are recomputed from TimeChimp every time.

Details are settled in [spec.md](spec.md).
