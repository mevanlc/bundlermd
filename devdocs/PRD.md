# BundlerMD — Product Requirements Document

BundlerMD is a desktop application (Tauri) that bundles multiple text files into a single
markdown file.

Frameworks, toolkits, libraries, and resource packs are implementer's choice.

## Core workflow

1. The user adds files to the workarea via a browse dialog. OS drag'n'drop to add
   files is deferred to an unspecified future version.
2. The user executes **Export Bundle**, which prompts for a save location for the generated
   markdown (`.md`) file.
3. BundlerMD reads the current contents of the added files and combines them into a single
   markdown file at the chosen location.

A project does not need to be created, saved, or opened in order to export a bundle.

## Projects

- A project can be saved-to / opened-from a `.bmd` project file.
- The `.bmd` format is JSON and uses an independent single whole-number schema
  version, visible in the schema filename (`project-v1.json`, `project-v2.json`,
  etc.). The schema is linked from project files by public URL in case it is
  useful, but it is mostly for internal validation rather than a guaranteed
  contract for hand-editing project files. A schema version bump should be
  treated as very likely to include breaking changes, and a particular schema
  version is not guaranteed to remain stable forever.
- A `.bmd` stores:
  - The paths of the files added to the workarea, in order. Files under the `.bmd` file's
    directory (or a descendant) are stored relative to it so the project travels with its
    files; files elsewhere are stored absolute.
  - The absolute path of the last export output file.
  - All project settings (see below). New project settings will be added over the course of
    development.
- Save/Open/Save As/dirty-state UX: typical document-editor behavior, including the
  save-changes prompt when closing a dirty workarea, with the create-new-`.bmd` variant when
  the workarea isn't yet associated with a file.

### Project settings

Accessed via a **Project Info...** button which opens a dialog.

- **Description** — multiline text. When **Include Description in Export** is checked,
  the description is emitted near the top of the bundle. When unchecked or empty, a blank
  line is left where the text would have appeared.
- **Path Presentation** — controls how file paths are both stored in the `.bmd` and rendered
  in the bundle's table of contents and per-file headers. Radio buttons:
  - **(o) Smart** (default):
    - Files living under the directory containing the `.bmd` file are stored and shown
      relative to that directory (so the project travels with its files).
    - All other files are stored absolute and shown as a bare basename — unless two or more
      files share a basename, in which case each member of the ambiguous set is disambiguated
      by progressively prepending path segments (deepest segment of the dirname first) until
      the set is unambiguous.
  - **( ) Absolute paths** — every file is stored and shown as a full absolute path.
- **Generate internal links in Table of Contents** — checkbox. Links use GitHub's
  anchor-generation rules for heading-to-fragment formatting/escaping.
- **Output newlines** — `(o) Always Unix` `( ) Always Windows` `( ) Platform Default`.
  All newlines in exported content are normalized to this setting; e.g. a source file
  containing CRLF does not cause CRLF to be emitted when the project is set to Always Unix.

## Workarea behavior

- Files can be reordered by drag'n'drop or via right-click → Move Up / Move Down /
  Move to Top / Move to Bottom.
- Right-click → **Remove File from Bundle**.
- Adding a file whose absolute path is already in the project is a no-op (the existing entry
  keeps its position).
- **Missing files**: entries whose underlying file no longer exists are highlighted in red.
  The app polls roughly once per second so the highlight tracks files leaving and returning.
  This is a usability aid only — the functional gate is at export time (see Export error
  handling).

### Adding folders

Browsing to a folder presents a dialog listing the files that would be added,
with an option to import only the folder's immediate children or to import recursively.
OS drag'n'drop of folders is deferred to an unspecified future version.
Import proceeds without stopping for errors: all files that pass the binary/text checks are
added, and any files that were skipped (binary, unreadable, over size limits, etc.) are
reported in a single batched warning.

## Export

### Bundle format

The exported file is always BOMless UTF-8.

Structure (pseudo-Mustache; heading depths are part of the format):

`````markdown
# {{ project_basename || output_basename }}

{{ project.description if include_description_in_export }}

## Table of Contents

{{ ordered markdown list of presented file paths, optionally linked per project settings }}

## File {{ n }}: {{ presented_path }}

{{ fence }}
{{ file content, newline-normalized }}
{{ fence }}
`````

- One `## File {{ n }}: ...` section per file, in workarea order, numbered from 1.
- `presented_path` follows the project's Path Presentation setting. File headers do not wrap
  the path in backticks (so GitHub-style TOC anchors resolve cleanly).
- `fence`: scan the file's content for its longest uninterrupted run of backticks; the fence
  uses that count plus one, with a minimum of 3. Headings or other markdown inside a file's
  content live within its code fence, which is acceptable.

### Export error handling

The bundle is generated in memory, best-effort, continuing past per-file problems (missing
file, read error, file became binary since add, over size limit, etc.). If any problems
occurred, the user is shown the list of affected files and asked whether to:

- **Save anyway** — the export is written, missing exactly the content of the listed files; or
- **Cancel** — no file is written, so the user can fix the issues and retry.

## Text/binary detection

Performed when adding files and again at export time (export re-reads files as they exist at
export time; a file that passed at add time may have changed).

1. UTF-16 BOM present → load as UTF-16 into a native string; strip the BOM.
2. Else, file contains NUL bytes → treat as binary and warn the user. When multiple files are
   added at once, warnings are batched into a single dialog; files that pass are still added.
3. Else → load as UTF-8 into a native string.

**Known limitation**: BOMless UTF-16 files contain NUL bytes and will be treated as binary.
Workaround: add a BOM, or convert the file to UTF-8 / 7-bit ASCII.

## Application behavior and settings

- Single-instance application with multiple OS windows; Recents and App Settings are global
  state shared across windows.
- Each open project gets its own top-level window. Only one window may be attached to a given
  project: opening an already-open project focuses its existing window, and any operation that
  would associate one project with more than one window is blocked with an informative
  message.
- The app remembers the 12 most recently opened/saved `.bmd` files.

### App settings

- **Theme** — Dark / Light / System (default).
- **Maximum total export size** — 250,000,000 bytes.
- **Maximum size of any one individual file** — 200,000,000 bytes.
