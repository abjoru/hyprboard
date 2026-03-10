# HyprBoard — Implementation Plan

PureRef/BeeRef alternative in Rust. egui + eframe (wgpu backend).

## Architecture

Scene graph, not ECS. SQLite-based file format (.hboard).

```
enum BoardItem {
    Image { texture: Option<TextureHandle>, original_bytes, original_size, transform, crop_rect, opacity, grayscale, flip_h, flip_v, labels },
    Text { content, font_size, color, transform },
}
```

Key crates: eframe, egui, image, rusqlite, rfd, arboard, flate2, imageproc

## Phases

### Phase 1: MVP Canvas ✅
- [x] Project scaffold with eframe + wgpu (Vulkan backend)
- [x] Board struct with Vec<BoardItem> scene graph
- [x] egui Scene widget as infinite canvas
- [x] Load images via file dialog (rfd)
- [x] Render images as egui textures on canvas
- [x] Click-drag to move images
- [x] Mouse wheel zoom (via Scene)
- [x] Middle-click pan (via Scene)
- [x] Paste via wl-paste (image data + URL fetch via ureq)
- [x] Click to select, visual indicator
- [x] Delete selected items
- [x] Right-click context menu (Paste, Open, Delete)
- [x] File DnD (via patched winit fork — see Phase 6e)

**Notes:**
- arboard/smithay can't read image MIME types on Wayland — using wl-paste directly
- egui-winit swallows Ctrl+V before app sees it — Ctrl+V shortcut blocked, using context menu instead
- Upstream fix needed: egui-winit `return` after failed paste prevents Key event propagation

### Phase 2: Selection + Manipulation ✅
- [x] Shift-click multi-select (HashSet-based)
- [x] Selection rectangle (drag on empty canvas)
- [x] Resize handles on selected items (uniform scaling)
- [x] Rotation handle (circle above selection)
- [x] Z-order: `]` raise, `[` lower
- [x] Undo/redo stack (command pattern: Move, Resize, Rotate, Delete, Add, ZOrder)
- [x] Undo/Redo in Edit menu + context menu (Ctrl+Z works via consume_key)

**Notes:**
- Pointer coords transformed from screen to scene space via `layer_transform_from_global`
- Interaction state machine (Idle, DraggingItems, SelectionRect, ResizingHandle, RotatingHandle)
- Free functions for rendering to avoid borrow-splitting issues with Scene closure

### Phase 3: Save/Load

#### 3a: Retain original bytes ✅
- [x] Add `original_bytes: Arc<Vec<u8>>` to `BoardItem::Image`
- [x] Plumb through all image creation paths (file dialog, paste, URL fetch, dropped files)

#### 3b: SQLite persistence (`persistence.rs`) ✅
- [x] Add `rusqlite` (bundled) + `dirs` crate
- [x] Schema: `meta` table (version, created) + `items` table (z_order, type, transform, image BLOB, text fields)
- [x] `save_board(path, items) -> Result<()>` — create/overwrite DB, write all items
- [x] `load_board(path, ctx) -> Result<Vec<BoardItem>>` — read rows, decode images into textures
- [x] Store original encoded bytes (PNG/JPEG/etc) as BLOB, not decoded RGBA

#### 3c: Save/Load UI wiring ✅
- [x] Add `current_file: Option<PathBuf>` + `dirty: bool` to app state
- [x] Ctrl+S → save to current_file (Save As if none)
- [x] Ctrl+Shift+S → always Save As dialog
- [x] Ctrl+O → detect .hboard vs image by extension
- [x] File menu entries for Save, Save As, Open
- [x] Set `dirty = true` on undo-stack push, `false` after save

#### 3d: Window title ✅
- [x] `"HyprBoard — filename.hboard •"` (dot when dirty, no dot when clean)
- [x] Update via egui ViewportCommand

#### 3e: Unsaved changes dialog ✅
- [x] Modal confirmation on quit if dirty
- [x] Modal confirmation on open-new-file if dirty
- [x] Options: Save + proceed, Discard, Cancel

#### 3f: Auto-save (debounced save-on-change) ✅
- [x] Track `last_change: Instant` — reset on each undo-stack push
- [x] If dirty + current_file.is_some() + 2s since last_change → auto-save
- [x] Save to `.autosave` sidecar, not main file

#### 3g: Recent files ✅
- [x] Store in `~/.config/hyprboard/recent.json` via `dirs` crate (XDG)
- [x] File menu submenu, cap at 10 entries
- [x] Clear undo history on load

**Decisions:**
- Original encoded bytes in DB (not RGBA) — much smaller, negligible decode cost
- Debounced save-on-change (2s after last edit) to `.autosave` sidecar
- `dirs` crate for XDG config path
- Clear undo stack on load
- Unsaved-changes dialog implemented in this phase

### Phase 4: Image Ops + Text

#### 4a: Flip horizontal/vertical ✅
- [x] Toggle `flip_h`/`flip_v` on selected images
- [x] Render: flip UV coords in `draw_item` mesh
- [x] Keyboard: `H` flip horizontal, `V` flip vertical
- [x] Undo command: `Flip { indices, horizontal: bool }`

#### 4b: Grayscale toggle ✅
- [x] Toggle `grayscale` on selected images
- [x] Re-decode from `original_bytes` with grayscale conversion, replace texture handle
- [x] Keyboard: `G` toggle
- [x] Undo command: `Grayscale { indices }`

#### 4c: Opacity slider ✅
- [x] Render: apply `opacity` to vertex color alpha in `draw_item`
- [x] Floating properties panel — auto-shows when image selected
- [x] Slider 0.0–1.0 in properties panel
- [x] Undo command: `Opacity { indices, old_values, new_values }`

#### 4d: Crop tool ✅
- [x] New interaction state: `Cropping { idx, start, current }`
- [x] Enter via `C` shortcut (single selected image)
- [x] Draw crop overlay rectangle on image
- [x] Confirm on mouse release: set `crop_rect`, adjust UV coords in render
- [x] Cancel (Escape): discard
- [x] Reset crop: `Shift+C`
- [x] Undo command: `Crop { idx, old_rect, new_rect }`

#### 4e: Text annotations ✅
- [x] `BoardItem::Text` — plain text only
- [x] Double-click empty canvas → create text item, enter edit mode
- [x] Double-click existing text → edit mode (inline `TextEdit`)
- [x] New interaction state: `EditingText { idx }`
- [x] Undo: `EditText { idx, old_content, new_content }`

#### 4f: Image-attached labels ✅
- [x] `Label { text, offset, font_size, color }` embedded in `BoardItem::Image`
- [x] `labels: Vec<Label>` field on Image variant
- [x] Labels move with parent image (offset is relative to image position)
- [x] Right-click image → "Add Label" (context menu)
- [x] Double-click label → edit mode (inline `TextEdit`)
- [x] Drag label → updates offset (DraggingLabel interaction state)
- [x] Label hit testing before image hit testing (labels render on top)
- [x] Persist in SQLite: `labels` table with `item_id` foreign key
- [x] Undo: `AddLabel`, `EditLabel`, `MoveLabel`, `DeleteLabel`

#### 4g: Fit/fill canvas ✅
- [x] `F` — fit all items in view (zoom/pan scene_rect)
- [x] `Shift+F` — fit selected items in view

**Decisions:**
- Floating properties panel (not side panel) — preserves canvas space
- Grayscale via re-decode from original_bytes (not dual textures)
- Crop reset: Shift+C
- Text: plain text only (rich text deferred to Phase 6)
- Labels embedded in BoardItem::Image (not standalone items with attachment ref)
- Labels embedded in BoardItem::Image (not standalone items with attachment ref)

### Phase 5: Clipboard ✅

#### 5a: Copy to clipboard ✅
- [x] `clipboard.rs` — pipe to `wl-copy --type image/png` or `text/plain`
- [x] `ensure_png()` — re-encode non-PNG images before copy
- [x] `Board::copy_selected()` — single image: copy original bytes as PNG; single text: copy content
- [x] Edit menu + context menu: "Copy"

#### 5b: Copy collage (multi-select) ✅
- [x] `render_collage()` — composite selected items into single PNG at 2x resolution
- [x] `apply_transforms()` — full pipeline: decode, crop, flip, grayscale, scale, rotate, opacity
- [x] Uses `imageproc` for rotation
- [x] Z-order preserved (bottom to top overlay)

#### 5c: Cut ✅
- [x] `Board::cut_selection()` = copy + delete
- [x] Edit menu + context menu: "Cut"

#### 5d–5f: Wayland DnD — Resolved in Phase 6e
- [x] Receive drops from file managers / browsers (via winit fork)
- [x] file:// URI paste from clipboard
- [ ] Drag source (cross-window) — not needed

**Previously blocked:** winit 0.30 had zero Wayland DnD support. Resolved by forking
winit and cherry-picking PR #4504 (SCTK DataDeviceManager integration). See Phase 6e.

#### 5g: Unit tests ✅
- [x] `clipboard::tests` — 15 tests: ensure_png, apply_transforms (identity, scale, crop, flip, grayscale, opacity, rotation, combined, errors)
- [x] `app::tests` — 13 tests: urlencoding_decode, hex_val

#### 5h: Context menu ✅
- [x] "Quit" at bottom of context menu (with separator, triggers unsaved dialog)

**Decisions:**
- Collage scale: 2x scene coords (double bounding_rect for quality)
- Copy shortcut: menu only (no Ctrl+C — egui-winit intercepts)
- New dep: `imageproc = "0.25"` for rotation in collage
- Wayland DnD resolved in Phase 6 via winit fork (not ashpd)

### Phase 6: Polish + Migration ✅

#### 6a: Performance ✅

##### 6a.1 Frustum culling ✅
- [x] Get `clip_rect()` from Scene UI in scene coords
- [x] Skip `draw_item` + hit-test allocation for items outside visible rect
- [x] Expand visible rect by handle size to avoid clipping selection handles

##### 6a.2 Lazy decode ✅
- [x] `texture` field changed to `Option<TextureHandle>` in `BoardItem::Image`
- [x] Added `img_width`/`img_height` columns (ALTER TABLE migration in persistence.rs)
- [x] `load_board` skips decode, stores bytes + dimensions only
- [x] `render_scene`: decode up to 2 visible items per frame
- [x] Gray "Loading..." placeholder rect for unloaded items
- [x] `image_dimensions()` for fast dimension probe without full decode

#### 6b: Import/Export ✅

##### 6b.1 .bee file import ✅
- [x] New `src/bee_import.rs`: `import_bee(path) -> Result<Vec<BoardItem>>`
- [x] BeeRef schema: `items` table + `sqlar` table (image BLOBs, possibly zlib-compressed)
- [x] Map: uniform `scale` → `Vec2::splat`, `rotation` degrees → radians, `flip -1` → flip_h
- [x] Parse JSON `data` field for opacity/grayscale/crop
- [x] Decompress sqlar blobs with `flate2` when `length(data) < sz`
- [x] File menu: "Import .bee..." (replaces current board)
- [x] New dep: `flate2`

##### 6b.2 Export canvas region ✅
- [x] New interaction state: `ExportingRegion { start, current }`
- [x] Trigger: `Ctrl+E` or menu "Export Region..."
- [x] Drag rect on canvas, on release → render items in region via `render_region()` → save dialog (PNG)
- [x] `render_region()` in clipboard.rs (shared logic from `render_collage`)

#### 6c: UI Polish ✅

##### 6c.1 Minimal toolbar ✅
- [x] `show_toolbar: bool` in app state, toggle via View menu / `Ctrl+T`
- [x] `TopBottomPanel::top` below menu bar
- [x] Buttons: Open, Save, Undo, Redo | Zoom Fit | Crop, Flip H/V, Grayscale | Export
- [x] Text labels (not icons)

##### 6c.2 Grid/snap ✅
- [x] Board fields: `show_grid: bool`, `snap_to_grid: bool`, `grid_size: f32` (default 50)
- [x] Render grid lines within visible_rect (skip if cells < 5px)
- [x] Snap item position to grid on drag release
- [x] `Ctrl+G` toggle grid, `Ctrl+Shift+G` toggle snap
- [x] View menu entries for grid/snap

#### 6d: Shortcuts + Docs ✅

##### 6d.1 Vim-style movement ✅
- [x] `h/j/k/l` nudge selected items by grid_size (or 10px) — only when items selected
- [x] `x` delete selected (alias for Delete)
- [x] `u` undo (alias for Ctrl+Z)
- [x] Remapped: `H` (flip-h) → `Alt+H`, `V` (flip-v) → `Alt+V`, `G` (grayscale) → `Alt+G`

##### 6d.2 Hyprland window rules docs ✅
- [x] `HYPRLAND.md`: float, pin, opacity, noborder rules for `class:^(hyprboard)$`

#### 6e: Wayland DnD via winit fork ✅

##### 6e.1 Fork winit with Wayland DnD ✅
- [x] Forked `rust-windowing/winit` → `abjoru/winit`
- [x] Cherry-picked PR #4504 onto v0.30.13: `hyprboard-v0.30-dnd` branch
- [x] Resolved conflicts: v0.31 module paths → v0.30, `foldhash` → `ahash`, `winit_core::event` → `crate::event`
- [x] Adapted events: `DragEntered/DragDropped` → v0.30's `DroppedFile/HoveredFile/HoveredFileCancelled`
- [x] Extended MIME support: `text/uri-list` > `text/x-moz-url` > `text/plain` (Chrome/Firefox compat)
- [x] HTTP/HTTPS URL passthrough in `parse_uri_list()` (not just `file://`)
- [x] `text/x-moz-url` decoding: auto-detects UTF-16LE (Firefox) vs UTF-8 (Chrome)
- [x] `CursorMoved` events emitted during DnD enter/motion for drop positioning
- [x] `[patch.crates-io]` in Cargo.toml points at fork
- [x] See `WINIT_PATCHES.md` for upstream tracking checklist

##### 6e.2 Wire dropped files into HyprBoard ✅
- [x] `handle_dropped_files` detects .hboard/.bee by extension, routes to `load_board`/`import_bee`
- [x] Image files loaded via existing `add_image_from_bytes` path
- [x] Drop highlight overlay when files hover over window (`show_drop_highlight`)
- [x] HTTP/HTTPS URL drops downloaded via ureq (Chrome/browser DnD)
- [x] Drop position: images placed at cursor location in scene coordinates

##### 6e.3 file:// URI paste ✅
- [x] `paste_from_clipboard` detects `file://` URIs in clipboard text
- [x] URL-decodes paths, loads as images
- [x] Handles multi-line URI lists (file manager "Copy" of multiple files)

**Decisions:**
- .bee import: replaces current board (matches Open semantics)
- Flip-H/V/G: remapped to Alt+H/Alt+V/Alt+G to free hjkl for vim movement
- Export: 2x resolution (same as collage), PNG only
- Wayland DnD: winit fork (not ashpd) — direct SCTK integration is cleaner than portal
- winit fork branch: `hyprboard-v0.30-dnd` based on v0.30.13

**Dependencies:**
- `abjoru/winit` git fork (branch `hyprboard-v0.30-dnd`) — patched via `[patch.crates-io]`
- `flate2` — sqlar decompression for .bee import

### Phase 7: Polish + Async ✅

#### 7a: Async network I/O ✅
- [x] `fetch_and_add_image_at` / `download_url` replaced with `spawn_download`
- [x] Background thread downloads via ureq, sends bytes back via `mpsc::channel`
- [x] `poll_downloads()` in update loop receives completed downloads
- [x] `ctx.request_repaint()` from download thread triggers UI update

#### 7b: Non-blocking export dialog ✅
- [x] `handle_export_region` stores rendered PNG in `pending_export: Option<Vec<u8>>`
- [x] File dialog shown in `update()` via `handle_pending_export()`, outside render pipeline

#### 7c: Context menu robustness ✅
- [x] Removed `context_menu_age` frame counter hack
- [x] Uses `area_resp.response.clicked_elsewhere()` + Escape for dismissal

**Decisions:**
- `mpsc::channel` (not `Arc<Mutex>`) for download results — simpler, supports multiple concurrent downloads
- Download thread clones `egui::Context` for `request_repaint()` (Context is Arc-based, Send+Sync)
- `replace_items` resets the download channel to discard in-flight downloads for replaced boards
