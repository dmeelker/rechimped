---
name: architecture-review
description: Critical architecture review of this workspace (coupling, cohesion, layering, error handling, testability) by a high-effort subagent, then optionally carrying out the improvements one commit at a time. Use when asked to review the architecture, look for structural problems, or "repeat the architecture exercise".
---

# Architecture review

Two phases. Phase 1 always runs; phase 2 only when the user asks for the improvements to be made
("do all", "fix them", a selection of items).

## Phase 1: review (read-only)

Launch one `general-purpose` agent with `effort: "higher"`. It reports back; relay a condensed
version to the user (ranked findings plus roadmap), not the raw report. Brief it like this (adapt
the paths if the layout changed; read `CLAUDE.md` first so the brief matches the current modules):

> Do a critical architecture review of the Rust workspace in this repository (crates `timechimp-api`,
> the API library, and `rechimped`, the gpui desktop app). Read-only: don't modify files, don't
> run `cargo` builds that write to `target/` unless needed, don't commit. Read `CLAUDE.md`, the
> Cargo manifests and every source file.
>
> 1. Map it: modules and their responsibility, the dependency graph (who uses whom), entry points,
>    main data flow.
> 2. Assess: coupling (cycles, reaching into internals, shared mutable state, init-order or
>    temporal coupling, leaky abstractions); cohesion (god objects/files/functions, mixed concerns,
>    scattered logic); layering (core logic vs I/O/UI, dependency direction); abstraction
>    boundaries; error-handling strategy; ownership/state lifetime (e.g. what survives a logout);
>    testability (what can't be tested without a window or network, and why); duplication, dead
>    code, naming; extensibility.
> 3. Be concrete: cite `file:line` for every finding, separate real problems from taste, and size
>    suggestions to the project (a few thousand lines; no enterprise layering).
>
> Report: overview with module map and dependency sketch; brief strengths; findings ranked by
> impact (problem, evidence, why it matters, concrete fix with a code/structure sketch); a
> prioritized roadmap (quick wins, medium, larger restructurings) with rough effort.

Don't verify or redo the agent's work yourself while it runs. When relaying, say which findings
you didn't verify.

## Phase 2: carrying out the roadmap

Work through the roadmap in order: quick wins first, then medium items, then the larger
restructurings (later ones build on earlier ones, so don't reorder freely).

- One commit per item on the current branch, message in the repo's style (imperative subject,
  a body saying what was wrong and what changed). Commit only when the user asked for commits.
- After every item: `cargo build`, `cargo test --workspace` and
  `cargo clippy --workspace --all-targets`, all clean (CLAUDE.md: fix every clippy warning,
  pre-existing ones included). Add unit tests for logic that becomes testable.
- Keep `CLAUDE.md` in sync in the same commit when a module, type or path it mentions moves.
- Prefer moving code verbatim when splitting files (a small script that moves top-level items is
  less error-prone than retyping), then fix imports and visibility.
- Files are CRLF in the working tree (`core.autocrlf=true`). When editing with scripts, preserve
  each file's line endings, and avoid backslashes inside bash heredocs (they get mangled; use
  `chr(92)` in Python or write the script to a file first).
- Behaviour changes (not just restructuring) get called out in the commit message and in the
  final summary.
- Skip items that can't be checked (e.g. guesses about the undocumented API) or that are optional
  and change user-visible storage; list them as not done, with the reason.
- Running the app talks to the real account: a short launch to see it starts is fine, but don't
  save or delete anything.

Finish with a summary: the commits, what changed in behaviour, the test count before/after, and
what was left out and why.
