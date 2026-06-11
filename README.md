# BundlerMD

Bundle multiple text files into a single Markdown document — useful for feeding a codebase or document set into an LLM, sharing review packages, or archiving project snapshots.

## Features

- **Add files** via dialog or folder import (shallow or recursive), with a preview before committing
- **Reorder** by drag-and-drop or right-click context menu
- **Project files** (`.bmd`) save and restore the file list, settings, and last export path
- **Smart Relative paths** in the bundle: files under the project folder are shown relative to it; others get the shortest unambiguous name
- **Size limits**: configurable per-file and total caps; oversized files are reported and skipped, not silently dropped
- **Missing-file highlighting**: files deleted from disk turn red within about a second
- **Multi-window**: one project per window; opening an already-open project focuses its window instead of loading it again
- **Recents** menu (12 entries, global, persisted across restarts)
- **Theme**: System, Light, or Dark — applies to all open windows
- **Optional in-window menu bar**: supplements the native menu bar

## Bundle format

````
# Project Title

[Introduction text if set]

## Table of Contents

- file1.rs
- lib/helper.py
- …

## File 1: file1.rs

```rust
… file content …
```

## File 2: lib/helper.py

…
````

- File content is wrapped in code fences sized to never conflict with content (min 3 backticks; one more than the longest run in the file)
- Newlines are normalized to Unix, Windows, or platform-default — your choice per project
- Table of Contents entries optionally link to their sections via GitHub-style anchors

## Keyboard shortcuts

| Action | macOS | Windows / Linux |
|--------|-------|-----------------|
| New project | ⌘N | Ctrl+N |
| New window | ⇧⌘N | Ctrl+Shift+N |
| Open… | ⌘O | Ctrl+O |
| Save | ⌘S | Ctrl+S |
| Save As… | ⇧⌘S | Ctrl+Shift+S |
| Export Bundle… | ⌘E | Ctrl+E |
| Settings… (macOS) | ⌘, | — |

## Building from source

**Prerequisites**: Rust (stable), Node.js 22+, platform WebView requirements for Tauri 2.

```sh
npm install
npm run tauri dev      # development (hot-reload)
npm run tauri build    # release bundle
```

See the [Tauri prerequisites guide](https://tauri.app/start/prerequisites/) for platform-specific setup (WebKit headers on Linux, etc.).

## Releasing

Push a tag matching `v*.*.*` and the GitHub Actions release workflow builds bundles for macOS (arm64 + x86_64), Windows, and Linux, then creates a draft release for review before publishing.

macOS code-signing and notarization require these repository secrets: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`. Without them the build succeeds but the app is unsigned.

## Project file format

`.bmd` files are plain JSON:

```json
{
  "__format__": {
    "name": "BundlerMD Project",
    "version": 1
  },
  "files": ["/abs/path/file1.rs", "/abs/path/lib/helper.py"],
  "last_export": "/abs/path/bundle.md",
  "settings": {
    "title": "",
    "introduction": "",
    "newlines": "unix",
    "path_presentation": { "mode": "smart" },
    "toc_links": false
  }
}
```

`path_presentation.mode` is `"smart"`, `"absolute"`, or `"fixed"` (with a `"location"` field).

## License

MIT
