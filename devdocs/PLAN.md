# BundlerMD — Implementation Plan

Companion to [PRD.md](PRD.md). The PRD says *what*; this document says *in what order* and
records the architectural decisions the PRD leaves open.

## Architecture decisions

- **Stack**: Tauri 2 + Rust backend; React + TypeScript + Vite frontend.
- **Source of truth lives in Rust.** The project model (file list, order, settings,
  dirty flag, association with a `.bmdp` path) is owned by the Rust side, keyed by window.
  The frontend is a view/controller that calls commands and re-renders from state-change
  events. This keeps export, persistence, and dirty-tracking logic in one place and testable
  without a webview.
- **All file I/O in Rust**: reading/probing added files, binary detection, `.bmdp`
  load/save, export generation. The frontend never touches the filesystem directly (native
  drag'n'drop file paths come through Tauri's drop events).
- **Export is a pure function** in Rust: `(project state, file contents) → bundle string +
  problem list`. Fence sizing, newline normalization, path presentation, TOC/anchor
  generation are all deterministic and unit-testable with no UI involved.
- **Global state** (Recents, App Settings) is a small Rust-owned store persisted to the
  platform config directory, shared across windows.

## Phases

Each phase ends in a working, demoable app. Within a phase, items are roughly ordered.

### Phase 1 — Core: add files, export a bundle (MVP)

The PRD's "core workflow", no project persistence.

- Tauri scaffold, single window, CI running `cargo check` / `cargo test` / frontend build.
- Workarea list: add files via drag'n'drop and browse dialog; remove; reorder (drag'n'drop
  and right-click Move Up/Down/Top/Bottom); duplicate-add is a no-op.
- Text/binary detection on add (UTF-16 BOM rule, NUL-byte rule, UTF-8 fallback), with
  batched warnings.
- Export Bundle: save dialog → in-memory generation → write BOMless UTF-8.
  - Bundle format per PRD: H1 title (basename fallback), introduction slot, TOC (plain, no
    links yet), per-file H2 sections.
  - Backtick fence sizing (longest run + 1, min 3).
  - Newline normalization (hardcoded Always Unix in this phase).
  - Path presentation: bare basenames only in this phase (Smart Relative needs a `.bmdp`
    location anyway).
- Export error handling: best-effort generation, problems dialog with Save anyway / Cancel.

**Exit criteria**: drop in a handful of files — including one with a long backtick run, one
CRLF file, and one binary — reorder them, export, and get a correct bundle with the binary
file reported in the problems dialog.

### Phase 2 — Projects

- `.bmdp` JSON schema with version field; load/save in Rust with serde.
- Save / Save As / Open / New; dirty tracking; close-with-unsaved-changes prompt
  (typical document-editor UX per PRD).
- Project Settings dialog: Title, Introduction, Output newlines (Unix / Windows /
  Platform Default) — all wired into export.
- Path Presentation setting: Absolute, Relative-to-fixed-location, and **Smart Relative**
  (common-prefix-under-`.bmdp`-dir rule plus the progressive basename-disambiguation
  algorithm). Smart Relative is the algorithmically interesting piece — implement it as a
  pure function with table-driven tests before wiring it to the UI.
- TOC internal links (GitHub anchor rules), behind the project setting.

**Exit criteria**: full save/open/saveas/dirty round-trip; a project with colliding
basenames exports with correctly disambiguated headers and working TOC links on GitHub.

### Phase 3 — Robustness and workarea UX

- Missing-file red highlighting with ~1 s polling; re-verified state when files return.
- Folder drop/browse: preview dialog, immediate-children vs. recursive import, batched
  skip warnings.
- Size limits (per-file and total-export) enforced at add and export time, surfaced through
  the existing batched-warning and export-problems flows.
- Export-time re-checks hardened: file deleted, permission denied, became binary, over
  limit — each lands in the problems dialog rather than aborting.

**Exit criteria**: delete a file out from under an open project and watch it turn red;
restore it and watch it recover; drop a deep folder and import recursively; exporting with
two broken files offers Save anyway and the output is correct minus exactly those two.

### Phase 4 — Application shell

- Single-instance enforcement (second launch forwards to the running instance).
- Multi-window: one window per project, open-again focuses the existing window, operations
  that would double-attach a project are blocked with a message.
- Recents (12 entries, global, persisted).
- App Settings dialog: theme (Dark/Light/System), max total export size, max individual
  file size.

**Exit criteria**: two projects open in two windows; re-opening project A from project B's
recents focuses A's window; theme change applies to both windows.

### Phase 5 — Polish and release

- Icons, window titles (project name + dirty marker), keyboard shortcuts for
  Save/Open/Export.
- Packaging/signing for the target platforms; release builds in CI.
- Pass over error messages and dialog copy.
- README and a short user-facing usage doc.

## Testing strategy

- **Rust unit tests** carry most of the weight, targeting the pure core: fence sizing,
  newline normalization, binary detection, Smart Relative disambiguation, GitHub anchor
  generation, `.bmdp` (de)serialization including version handling, export assembly with
  injected problem cases.
- **Rust integration tests** for command-level flows against temp directories: add/export
  round-trips, missing-file and permission-denied scenarios, size-limit enforcement.
- **Manual test script** per phase (the exit criteria above), since Tauri e2e tooling is
  thin; revisit automated UI testing only if regressions justify it.
- Fixture set checked into the repo: CRLF file, long-backtick-run file, NUL-containing
  file, UTF-16 BOM file, BOMless UTF-16 file (expected: flagged binary), colliding-basename
  tree.

## Deferred / out of scope for v1

- Watching files with OS-level notifications instead of 1 s polling.
- Streaming export (current design reads files into memory; acceptable under the 200 MB /
  250 MB limits).
- Language tags on code fences inferred from file extension — nice-to-have; revisit
  post-v1.
