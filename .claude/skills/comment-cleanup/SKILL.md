---
name: comment-cleanup
description: Aggressively cut code comments in this workspace down to the non-obvious "why", using parallel subagents over disjoint file sets plus a final stricter pass. Use when asked to clean up, cut or trim comments, or to "repeat the comment exercise".
---

# Comment cleanup

The rule, from the user: comments only say **why** something happens, and only when the code
doesn't make that clear. Never **what**. When in doubt, no comment.

Scope: every `//`, `///`, `//!`, trailing comment and commented-out code in the repo
(`.rs` files, plus `#` comments in the Cargo manifests and `rechimped/assets/make-icon.ps1`).
**Out of scope:** string literals. `store/config.rs` writes a starter `config.toml`
(`write_template`, `TRIPS_COMMENT`, `TIME_ENTRIES_COMMENT`, the `with_settings` key comments), and
those `#` lines are file content, not code comments.

## 1. Size it

Count comment lines per file to balance the split:

```sh
grep -rcE '^\s*//' --include=*.rs timechimp-api rechimped | sort -t: -k2 -n
```

## 2. Fan out

Launch four `general-purpose` agents in one message, each on a **disjoint** set of files so they can
edit the same working tree at once. Last time's split (rebalance it if the counts have shifted):

1. API library: `timechimp-api/src/*`, `timechimp-api/examples/`, `timechimp-api/tests/`, both `Cargo.toml`s
   except the GUI one.
2. State and data: `session.rs`, `store/*`, `seed.rs`, `validate.rs`.
3. Behaviour: `forms.rs` and all of `app/`.
4. The rest: `view/`, `domain/`, `fx.rs`, `main.rs`, `net.rs`, `password.rs`, `test_support.rs`,
   `build.rs`, `rechimped/Cargo.toml`, `assets/make-icon.ps1`.

The brief for each (fill in the file list):

> Repo: <the repo root>. Aggressively cut code comments in YOUR files only (below).
> Other agents are editing other files at the same time, so touch nothing outside your list. Don't
> run cargo (I build afterwards) and don't commit.
>
> Rule: comments only say WHY something happens, and only when the code doesn't make that clear.
> Never WHAT. When in doubt, no comment. This covers `//`, `///`, `//!`, trailing comments and
> commented-out code.
> - Delete: comments that restate the code, docs that name or describe an item, "Returns X" docs,
>   section banners, test narration that repeats the test name, unit or colour notes, usage examples.
> - Keep, shortened to just the reason: workarounds for gpui/gpuikit/API quirks, ordering
>   requirements, invariants the types can't express, why a simpler approach doesn't work, and
>   observations about the undocumented API.
> - If a comment mixes what and why, rewrite it to only the why, tersely.
> - Don't change code, attributes or string literals (`config.toml` template text included).
>   Files are CRLF; preserve each file's line endings.
>
> Report: the comment lines left per file, and every comment you kept with a one-line
> justification. Mark the borderline ones.

Don't redo their work while they run.

## 3. Stricter pass

The agents keep too much. Go over what is left with "when in doubt, no comment" and delete:
- every keep the agents marked borderline;
- unit and format notes, even observed ones (`/// UTC.`, `/// Kilometres.`);
- "shape not observed, hence raw JSON" notes, since the `Value` type already says that;
- doc examples and usage notes;
- comments that restate a condition the next line makes obvious ("They handle their own keys").

List what is left with `grep -rnE '^\s*//' --include=*.rs ...`. Don't filter that output with
`grep -v '://'` or similar: an earlier filter like that hid half of the comments. Delete with a
small Python script (patterns per file, CRLF preserved) written to the scratchpad. Avoid
backslashes in bash heredocs.

## 4. Verify

From the repo root:
- Only comments changed: every `+` line in `git diff -U0` must be a comment, or a code line whose
  trailing comment was removed (compare it against its `-` line):
  `git diff -U0 | grep -E '^\+[^+]' | grep -vE '^\+\s*(//|#)'`.
- `cargo clippy --workspace --all-targets` and `cargo test --workspace` both clean.
- Don't run `cargo fmt`. The repo isn't rustfmt-clean (e.g. `smoke.rs`), so it would add unrelated
  changes.

## 5. Report

Give the before/after comment-line count and the diff size, say what kind of comments went, list
the kinds that stayed, and give the clippy/test status. Leave it uncommitted until the user says
"commit", then make one commit (last time: "Cut comments down to the why").
