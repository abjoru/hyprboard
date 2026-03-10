# Winit Fork: Patches & Upstream Tracking

**Fork:** `abjoru/winit` branch `hyprboard-v0.30-dnd`
**Base:** winit `v0.30.13` (commit `e9809ef5`)
**Cargo.toml:** `[patch.crates-io] winit = { git = "...", branch = "hyprboard-v0.30-dnd" }`

## Summary

Winit v0.30.x has **no Wayland drag-and-drop support**. Our fork adds full
DnD via SCTK's `DataDeviceManagerState`, including browser-compatible MIME
handling and cursor tracking during drag operations.

---

## Patch 1: Wayland DnD via SCTK DataDeviceManager

**Commit:** `da7bf2e4`
**Files:** `data_device.rs` (new), `seat/mod.rs`, `state.rs`

### What it adds

- Binds `DataDeviceManagerState` and creates a `DataDevice` per seat
- Stores `dnd_offer` and `dnd_window` on `WinitState`
- Implements `DataDeviceHandler`, `DataOfferHandler`, `DataSourceHandler`
- Emits `WindowEvent::HoveredFile`, `HoveredFileCancelled`, `DroppedFile`
- Parses `text/uri-list` into file paths with percent-decoding
- Reads DnD pipe data on a background thread (non-blocking, no unsafe)
- Flushes Wayland connection after every protocol response for broad
  compositor compatibility (GNOME, KDE, wlroots)

### Upstream tracking

- **winit issue:** No open issue/PR for Wayland DnD as of v0.30.x
- **When to drop:** If winit adds `DataDeviceHandler` integration and emits
  `DroppedFile`/`HoveredFile` events on Wayland natively

---

## Patch 2: Browser DnD + Cursor Tracking

**Commit:** `b9994214`
**Files:** `data_device.rs`

### What it adds

- **MIME negotiation:** Accepts `text/uri-list` > `text/x-moz-url` > `text/plain`
  (Chrome/Firefox don't always offer `text/uri-list`)
- **HTTP URL passthrough:** `parse_uri_list()` passes `http://` and `https://`
  URLs as `PathBuf` pseudo-paths for app-level download
- **text/x-moz-url decoding:** Auto-detects UTF-16LE (Firefox) vs UTF-8 (Chrome)
- **CursorMoved during DnD:** Emits `WindowEvent::CursorMoved` in `enter()`
  and `motion()` so apps know the drop position (Wayland compositor grabs the
  pointer during DnD, so normal pointer events stop)
- **Debug logging:** Logs offered MIME types on DnD enter

### Upstream tracking

- **When to drop:** If winit's DnD implementation handles multiple MIME types,
  HTTP URLs, and emits cursor position during drag operations

---

## Quick Checklist for New Winit Releases

When evaluating whether to rebase or drop the fork:

1. Does winit emit `DroppedFile` on Wayland? → Patch 1 may be droppable
2. Does it handle `text/x-moz-url` and `text/plain` MIME fallbacks? → Part of Patch 2
3. Does it pass through HTTP/HTTPS URLs (not just `file://`)? → Part of Patch 2
4. Does it emit `CursorMoved` during DnD? → Part of Patch 2
5. Does it decode `text/x-moz-url` from both UTF-8 and UTF-16LE? → Part of Patch 2

If (1) is yes but (2-5) are no, we can reduce to just Patch 2 on top of upstream.
