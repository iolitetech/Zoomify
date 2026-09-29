# Zoomify bug-fix & performance roadmap

**Read this first.** This document is a work queue. It was produced by a full audit of the
codebase on 2026-09-28 (commit `9a6d260`). Every finding was verified against the actual
code, but **line numbers will drift as fixes land** — always locate code by searching for
the quoted identifiers/snippets (use Grep), never by line number alone. If a quoted snippet
no longer exists, the item may already be fixed; verify before changing anything.

**Working rules — follow these exactly:**

1. Work **one item at a time**, in the order given (P0 → P6). Do not batch unrelated items
   into one change.
2. After every item: `cargo build --release` must succeed with no new warnings. Then do the
   item's **Verify** step.
3. One commit per item, message format: `fix(P2-1): <short description>`.
4. Do **not** refactor beyond what the item says. If a fix seems to require restructuring
   not described here, stop and flag it instead of improvising.
5. Items marked **⚠ verify first** were found with medium/low confidence: reproduce the bug
   (or prove it by reading the code path end-to-end) before writing the fix. If you cannot
   reproduce or confirm it, skip the item and note why.
6. `ROADMAP.md` (the feature roadmap) has a "Design decisions worth not re-litigating"
   section — never undo anything listed there.
7. This is a Rust/Win32/Direct2D app. `f32::clamp(min, max)` **panics** if `min > max` —
   whenever you add a clamp, floor the upper bound with `.max(0.0)` first.

**Priority meaning:** P0 crashes · P1 the reported zoom-bounds bug · P2 memory/resource
leaks · P3 data loss & undo corruption · P4 performance · P5 DPI/rendering correctness ·
P6 hardening & polish.

---

## Status (updated 2026-09-29)

Every item below has been worked through and merged to `master` (PR #2 for P0-P6 up to
P4-1, PR #3 for P4-4); the commit for each is in the index's Status column. The build has
no warnings and the unit tests pass. **Nothing here has been checked by running the app**,
so treat the "Verify" steps as still owed, especially the ones below.

**Deviations from the plan as written:**

- **P4-1** does not track a `scene_revision`. There were ~60 places that mutate shapes, so
  the layer instead keeps a snapshot of the committed shapes and compares against it every
  frame (`Arc`ed image pixels compare by pointer first). A missed invalidation can then only
  cost speed, never show a stale picture. The layer is bypassed while a select-drag is
  rewriting shapes each frame, and skipped entirely when there are no shapes. Annotation text
  drawn into it uses grayscale smoothing, not ClearType.
- **P4-4** replaces the segment-by-segment drawing with one winding-filled union of the same
  capsules, for committed *solid* strokes only. The live stroke and dashed/dotted patterns keep
  the old path. A translucent highlighter with pressure no longer double-darkens at overlaps.
- **P4-5** caches layouts by content (text, font, wrap width) rather than by annotation
  id + revision, and draws the caret as a line from `HitTestTextPosition`.
- **P4-10** swaps the on-screen caches out for empty ones around `render_to_capture` and
  restores them, rather than keeping a second permanent set.
- **P4-11** was done except for: caching selection bounds (cheap enough after P4-5, and
  risky around arrow bindings during a drag); `laser_trail` as a `VecDeque` (160 points, no
  measurable cost); the StepBadge `number.to_string()` (trivial).
- **P3-2 / P3-8** share one commit (`eac325c`), as do several P1 and P2/P3 pairs.

**Still worth a manual pass:** the scene layer with text, blur, a pasted image, zoom/pan and a
Ctrl+C export; the text-editor caret mid-text; pressure strokes with a pen; cancelling a pen
stroke; Ctrl+Wheel in Live Zoom; unplugging a monitor; and one run at 150% scaling.

**Noticed, not changed:** a few long cheat-sheet rows overlapped their neighbours in a
screenshot (fixed-width text boxes, predates these changes); `render_contained_text` used
`w.max(wrap)` as its draw width, which is `f32::MAX` for labels that ride on a shape.

---

## Quick index

| ID | One-liner | Files | Status |
|---|---|---|---|
| P0-1 | Re-entrant `RefCell` borrow aborts the process while a file dialog is open | `main.rs` | Done `75bad45` |
| P0-2 | Heap over-read exporting after switching monitors | `overlay.rs`, `renderer/mod.rs` | Done `7d20c58` |
| P1-1 | **Live Zoom shows past the screen edge on non-primary monitors (the reported bug)** | `live_zoom.rs` | Done `27c18f2` |
| P1-2 | Zooming out shows out-of-bounds view for ~200 ms | `live_zoom.rs` | Done `27c18f2` |
| P1-3 | Wheel zoom snaps an infinite-canvas pan back to the screen | `types.rs`, `overlay.rs` | Done `6d6c21e` |
| P1-4 | Moving the mouse erases a zoomed whiteboard pan | `overlay.rs` | Done `6d6c21e` |
| P1-5 | Minimap ignores the infinite canvas | `types.rs` | Done `6d6c21e` |
| P1-6 | Wheel changes zoom invisibly in Spotlight mode | `overlay.rs` | Done `6d6c21e` |
| P2-1 | Geometry cache: unbounded growth every frame + stale-draw bug | `renderer/mod.rs`, `renderer/shapes.rs` | Done `5e328a1` |
| P2-2 | Image pixels deep-copied everywhere; undo history unbounded | `types.rs`, `overlay.rs` | Done `48c4459` |
| P2-3 | GPU image cache never evicts; weak cache key shows the wrong image | `renderer/mod.rs`, `types.rs` | Done `291d1bb` |
| P2-4 | Clipboard `HGLOBAL` leaks and unsafe DIB read | `clipboard.rs` | Done `b4b097f` |
| P2-5 | WGC capture rigs accumulate across display changes | `capture_wgc.rs`, `main.rs` | Done `2f91c6b` |
| P2-6 | No teardown at process exit | `main.rs`, `overlay.rs`, `settings_window.rs` | Done `59e106a` |
| P3-1 | Save/autosave writes only the active board; others are lost on exit | `overlay.rs`, `session.rs` | Done `5e29af7` |
| P3-2 | Exiting while editing text deletes the text | `overlay.rs` | Done `eac325c` |
| P3-3 | Manual saves silently deleted by autosave pruning | `overlay.rs`, `session.rs` | Done `6f35e71` |
| P3-4 | Settings window and overlay overwrite each other's config | `settings_window.rs`, `overlay.rs` | Done `0a10d03` |
| P3-5 | Toolbar Undo during a text edit duplicates the text | `overlay.rs` | Done `9a85312` |
| P3-6 | Arrow re-anchoring is invisible to undo | `overlay.rs`, `types.rs` | Done `08e68e2` |
| P3-7 | Duplicate copies group membership and arrow bindings | `overlay.rs` | Done `218be7a` |
| P3-8 | Re-edited text gets a new id, losing bindings/group/opacity | `overlay.rs` | Done `eac325c` |
| P3-9 | Loading a session lets group ids collide | `session.rs` | Done `5e29af7` |
| P3-10 | Ctrl+W destroys a board with no undo | `overlay.rs` | Done `0ec7153` |
| P4-1 | Retained scene layer (biggest perf win) | `renderer/mod.rs`, `overlay.rs` | Done `524071f` (needs manual check) |
| P4-2 | Select-drag deep-clones the selection twice per mouse move | `overlay.rs` | Done `4df5f3c` |
| P4-3 | Pen-down clones the whole canvas for snap anchors it then discards | `overlay.rs` | Done `d603909` |
| P4-4 | Pressure strokes: one `DrawLine` per segment per frame | `renderer/shapes.rs` | Done `1797a64` (needs a pen to check) |
| P4-5 | Text layouts rebuilt every frame (incl. on hover) | `renderer/shapes.rs`, `renderer/mod.rs`, `overlay.rs` | Done `22b83b3` (check the editor caret) |
| P4-6 | Toolbar allocates ~25 brushes + ~35 text layouts per frame | `renderer/ui_toolbar.rs` | Done `da914d9` |
| P4-7 | Idle 60 fps repaints (caret, toast, timer) | `overlay.rs`, `types.rs` | Done `3e5d40f` |
| P4-8 | Brush cache self-flushes under picker/laser/toast | `renderer/mod.rs`, `renderer/ui_picker.rs` | Done `6f331b5` |
| P4-9 | Blur re-renders its mosaic every frame; single-slot cache thrashes | `renderer/shapes.rs`, `renderer/mod.rs` | Done `4ac22a7` |
| P4-10 | Export wipes every render cache → hitch after each Copy/Save | `renderer/mod.rs` | Done `7e7be55` |
| P4-11 | Misc per-frame allocations & linear scans | several | Mostly done, see notes |
| P4-12 | Release profile options | `Cargo.toml` | Done `7e64945` |
| P5-1 | Settings window clicks land on the wrong control above 100% DPI | `settings_window.rs` | Done `58d6795` |
| P5-2 | Loupe magnifies the wrong area above 100% DPI | `renderer/ui_loupe.rs` | Done `402ec63` |
| P5-3 | Blur samples the wrong region above 100% DPI | `renderer/shapes.rs` | Done `402ec63` |
| P5-4 | Blur reveals the hidden desktop on Whiteboard/Blackboard | `renderer/mod.rs`, `overlay.rs`, `svg_export.rs` | Done `2b99901` |
| P5-5 | Shared text formats mutated (centred/no-wrap bleeds into other text) | `renderer/shapes.rs`, `ui_timer.rs`, `ui_toolbar.rs` | Done `8c9c82e` |
| P5-6 | Oversized exports/pastes silently blank or invisible | `renderer/mod.rs`, `clipboard.rs` | Done `3aaec65` |
| P5-7 | Emoji cannot be typed (UTF-16 surrogates dropped) | `overlay.rs` | Done `7a30a51` |
| P5-8 | Export mismatches (spotlight position, SVG pan, editor chrome) | `renderer/mod.rs`, `svg_export.rs` | Done `9a80ef3`, `405146d` |
| P6-1 | Session/config file robustness | `types.rs`, `renderer/mod.rs`, `svg_export.rs`, `config.rs`, `main.rs` | Done `4a81a74` |
| P6-2 | Input & device edge cases (stuck pen, Live Zoom Ctrl, unplugged monitor) | `overlay.rs`, `live_zoom.rs` | Done `314377c` |
| P6-3 | Small UX consistencies, `read_dib` V4/V5 parsing | `main.rs`, `overlay.rs`, `clipboard.rs` | Done `b2b0041` |

---

# P0 — Crashes

## P0-1 · Re-entrant `RefCell::borrow_mut` aborts the process while a modal dialog is open

**Files:** `src/main.rs` (the tray window procedure, `tray_wnd_proc`).

**Problem.** The overlay and settings window procs defend themselves with
`try_borrow_mut` (`overlay.rs` ~3871, `settings_window.rs` ~368), but `tray_wnd_proc`
calls `ctx.overlay.borrow_mut()` directly in many arms (search `main.rs` for
`.borrow_mut()` — sites around lines 98, 178, 191, 220, 237–312, 399, 425–437) and
`ctx.settings_window.borrow_mut().show()` (~169, ~322).

Two places open a **modal shell dialog while still holding a borrow**:
- `overlay.rs` ~3200: `self.with_topmost_suspended(|| crate::session::pick_session_file(hwnd, &dir))` — reached from Ctrl+O inside the overlay wndproc's `this` borrow.
- `settings_window.rs` ~826: `crate::session::pick_folder(self.hwnd, &start)` inside `handle_click`.

`IFileDialog::Show` runs its own message loop. That loop dispatches `WM_HOTKEY`,
`WM_TRAY_ICON`, `WM_DISPLAYCHANGE` etc. to `tray_wnd_proc`, whose `borrow_mut()` panics
with "already borrowed" — inside an `extern "system"` callback, so the process **aborts**.

**Repro:** Draw mode → Ctrl+O (file dialog opens) → press Ctrl+2 or double-click the tray
icon. Process dies.

**Fix (both halves):**
1. In `tray_wnd_proc`, replace every `ctx.overlay.borrow_mut()` with
   `let Ok(mut overlay) = ctx.overlay.try_borrow_mut() else { return <appropriate default>; }`.
   For messages that must not be lost (e.g. `WM_SETTINGS_APPLIED`), re-post the message
   with `PostMessageW` on borrow failure instead of dropping it. Do the same for
   `ctx.settings_window.borrow_mut()`.
2. Where practical, release the borrow before opening a dialog: copy out what the dialog
   needs (hwnd, directory string), drop the guard, run the dialog, then re-borrow to apply
   the result. Do this at least for the settings `pick_folder` call, which is the easy one.

**Verify:** repeat the repro; the hotkey press during the dialog must be ignored or queued,
never crash. Also press Ctrl+, while the settings Browse… dialog is open.

## P0-2 · Heap over-read when exporting after the overlay moves to a different-sized monitor

**Files:** `src/overlay.rs`, `src/renderer/mod.rs`. **⚠ verify first** (needs two monitors
of different resolutions; alternatively confirm by reading the path end-to-end).

**Problem.** Mode-entry functions (`enter_static_zoom`, draw/spotlight/timer/loupe — search
for `if self.background_bitmap.is_none()`) call `target_monitor_under_cursor()` →
`set_active_monitor`, which **always** overwrites `screen_width`/`screen_height`
(`overlay.rs` ~1029–1032) — but they only recapture when `background_bitmap.is_none()`.
So after switching monitors the old capture (old size) is paired with the new dimensions.
`get_composite_capture` (`overlay.rs` ~3038) then passes the new width/height with the old
`pixels` into `renderer/mod.rs` ~1118–1136, where `dc_rt.CreateBitmap(D2D_SIZE_U{width,height}, pixels.as_ptr(), width*4, …)`
reads `width*height*4` bytes from a smaller `Vec` — an out-of-bounds read (potential access
violation) on Ctrl+C / Ctrl+S / Ctrl+P.

**Repro:** Draw mode on a 1080p monitor → move the mouse to a 4K monitor → global Ctrl+1 →
Ctrl+C. Even without exporting, the wrong monitor's screenshot is visibly stretched.

**Fix:**
1. In `set_active_monitor`, if the monitor rectangle actually changed, set
   `self.background_bitmap = None;` and `self.background_capture = None;` so the next mode
   entry recaptures.
2. Defensively, in the `render_to_capture` path make the bitmap creation use the capture's
   **own** stored `width`/`height` (from `background_capture`), and bail out (log + return
   error) if `pixels.len() < width as usize * height as usize * 4`.

**Verify:** the repro shows the correct screen and exports fine. Single-monitor behaviour
unchanged (Tab-cycling monitors already recaptures — confirm it still does).

---

# P1 — The reported bug: zoom escapes the screen bounds

## P1-1 · Live Zoom pans off the desktop on any non-primary monitor ⭐ root cause of "zoom goes out of the left bound"

**File:** `src/live_zoom.rs`.

**Problem.** `update_target_from_cursor` (~280–281) computes:

```rust
self.target_x_offset = mon_x + target_x.clamp(0.0, max_x);
self.target_y_offset = mon_y + target_y.clamp(0.0, max_y);
```

and `apply_transform` (~327–331) / `start()` (~186–190) pass that to
`MagSetFullscreenTransform` unchanged. Per Microsoft's docs, the fullscreen magnifier maps
screen point `P` (virtual-desktop coords, primary top-left = origin) to source pixel
`offset + P / zoom`. To show source column `src_left` at the left edge of a monitor whose
left edge is `mon_x`, the offset must be **`mon_x + src_left − mon_x / zoom`** — the code
omits the `− mon_x / zoom` term. On the primary monitor `mon_x = 0`, so the bug is
invisible there; on a monitor left of the primary (`mon_x` negative, e.g. −1920) the view
shifts left by `|mon_x| / zoom` — half a screen at 2×, showing black past the desktop's
left edge. Mirror-image error on right/top/bottom-placed monitors.

**Repro:** two monitors, secondary placed LEFT of primary in Windows display settings →
cursor on the left monitor → Ctrl+4. The view sits far left with dead space at the edge.

**Fix — keep offsets monitor-relative, convert in exactly one place:**
1. In `update_target_from_cursor`, store monitor-relative values (delete the `mon_x`/`mon_y`
   addition):
   ```rust
   self.target_x_offset = target_x.clamp(0.0, max_x);
   self.target_y_offset = target_y.clamp(0.0, max_y);
   ```
2. Rewrite `apply_transform` to do the conversion:
   ```rust
   fn apply_transform(&self) {
       if self.is_active && let Some(set_fn) = self.mag_set_transform {
           let z = self.zoom_level;
           let mx = self.monitor_x as f32;
           let my = self.monitor_y as f32;
           // MagSetFullscreenTransform: screen point P shows source (off + P/z),
           // P relative to the primary monitor's top-left. For this monitor's
           // top-left to show source (mx + cur), off = mx + cur - mx/z.
           let off_x = mx + self.current_x_offset - mx / z;
           let off_y = my + self.current_y_offset - my / z;
           unsafe { let _ = set_fn(z, off_x.round() as i32, off_y.round() as i32); }
       }
   }
   ```
3. In `start()`, replace the inline `set_fn(zoom, off_x, off_y)` call (~185–191) with
   `self.apply_transform();` — it must run after `is_active = true` (it already does).
4. `stop()` keeps `set_fn(1.0, 0, 0)` as-is.
5. The `offsets()` getter now returns monitor-relative values; nothing calls it today, but
   check with Grep before assuming.

All view changes funnel through this formula (`tick_smooth_pan` → `update_target_from_cursor`,
`adjust_zoom` → `set_zoom_level`, `start()`), so this one change fixes every path.

**Verify:** on a single monitor Ctrl+4 behaves exactly as before (the correction term is 0).
On a left-placed secondary monitor, the magnified view stays inside that monitor's content
at every zoom level and cursor position — check all four screen edges.

## P1-2 · Zooming out shows a stale out-of-bounds offset for ~200 ms

**File:** `src/live_zoom.rs`, `set_zoom_level` (~237–244).

**Problem.** `set_zoom_level` updates the *target* offsets, then calls `apply_transform()`
while `current_*_offset` still holds the old-zoom values; the smoothing (factor 0.25 per
16 ms tick) takes ~10 ticks to catch up. At 4× with the cursor at the right edge, the
offset can be 0.75×width but the max at 2× is 0.5×width — so zooming out briefly displays
past the right/bottom edge.

**Fix.** After the existing `update_target_from_cursor()` call inside `set_zoom_level`,
clamp the *current* offsets to the new zoom's valid range before applying (do this after
P1-1, using monitor-relative values):
```rust
let mon_w = if self.monitor_w > 0 { self.monitor_w as f32 } else { unsafe { GetSystemMetrics(SM_CXSCREEN) as f32 } };
let mon_h = if self.monitor_h > 0 { self.monitor_h as f32 } else { unsafe { GetSystemMetrics(SM_CYSCREEN) as f32 } };
let max_x = (mon_w - mon_w / self.zoom_level).max(0.0);
let max_y = (mon_h - mon_h / self.zoom_level).max(0.0);
self.current_x_offset = self.current_x_offset.clamp(0.0, max_x);
self.current_y_offset = self.current_y_offset.clamp(0.0, max_y);
self.apply_transform();
```

**Verify:** at 4×, cursor at the bottom-right corner, Ctrl+Wheel down to 2× — no flash of
content beyond the edge.

## P1-3 · Wheel zoom snaps an infinite-canvas pan back inside the screen

**Files:** `src/types.rs` (`set_zoom_centered`, ~1334–1338), `src/overlay.rs` (callers:
wheel handler ~5152, arrow-key zoom ~6187 and ~6226).

**Problem.** `set_zoom_centered` unconditionally clamps to screen bounds. On a
Whiteboard/Blackboard panned past the screen edge (middle-drag), the first wheel tick
yanks the view back inside `[0, max]`.

**Fix.** Add an `infinite: bool` parameter to `set_zoom_centered`. When true, compute the
new viewport then call the existing infinite clamp (`clamp_viewport_infinite`) instead of
`clamp_viewport`. At each call site pass
`self.mode == AppMode::Draw && self.background_type != CanvasBackground::Transparent`
(match however the existing infinite-pan code — search `tick_smooth_pan` and its `infinite`
flag — computes the same predicate, and reuse that expression exactly).

**Verify:** whiteboard → middle-drag left past the edge → scroll wheel → the view zooms
around the cursor without jumping back. Transparent background still clamps to the screen.

## P1-4 · Moving the mouse erases a zoomed whiteboard pan

**File:** `src/overlay.rs` ~4381–4389.

**Problem.** The mouse-move handler runs `update_target_from_cursor` in Draw mode whenever
`level > 1.001`, including on Whiteboard/Blackboard, overwriting an infinite-canvas pan
with a screen-bounds position.

**Fix.** Add `&& this.background_type == CanvasBackground::Transparent` to the Draw-mode arm
of that condition (only the Draw part — StaticZoom keeps its behaviour).

**Verify:** whiteboard → zoom to 2× → middle-drag past the edge → release → wiggle the
mouse. The view must not move.

## P1-5 · Minimap ignores the infinite canvas

**File:** `src/types.rs` — `center_on_canvas_point` (~1343–1351) and `get_viewport_rect`
(~1634–1643).

**Problem.** `center_on_canvas_point` always uses the screen-bounds clamp, so a minimap
drag on the whiteboard cannot reach panned-out regions; `get_viewport_rect` clamps the
indicator to 0..1, hiding the out-of-bounds state instead of showing it.

**Fix.** Thread the same `infinite` flag from P1-3 into `center_on_canvas_point` (choose
`clamp_viewport_infinite` when true). In `get_viewport_rect`, when infinite, let the
indicator position go outside 0..1 (or clamp only for drawing, but compute from unclamped
values) so the indicator reflects reality.

**Verify:** whiteboard, panned out → the minimap indicator shows the true position and a
minimap drag can navigate the extended canvas.

## P1-6 · Wheel changes zoom invisibly in Spotlight mode, corrupting exports

**File:** `src/overlay.rs` ~5134–5137 (wheel branch), cross-check `renderer/mod.rs` ~721–727
and `render_to_capture` ~1077–1085.

**Problem.** The wheel-zoom branch includes `AppMode::Spotlight`, but rendering only applies
zoom in StaticZoom and Draw — so the zoom level changes with no visible effect. Exports
(`render_to_capture`) ignore the mode, so a Copy/Save from Spotlight after scrolling
produces a zoomed image the user never saw.

**Fix.** Remove `AppMode::Spotlight` from the wheel-zoom condition (in Spotlight the wheel
already resizes the spotlight radius elsewhere — confirm and keep that).

**Verify:** Spotlight → scroll → Ctrl+C. The exported image matches the screen.

---

# P2 — Memory & resource leaks

## P2-1 · Geometry cache: unbounded per-frame growth, cross-session leak, and stale-draw bug

**Files:** `src/renderer/mod.rs` (`geometry_cache`, ~line 83; cleared only in
`recover_if_device_lost` ~585–588), `src/renderer/shapes.rs` (strokes ~94–140, arrow heads
~233–272). This is the **worst leak in the app** — three audits independently flagged it.

**Problem.**
- The cache key is `(points.as_ptr() as u64, points.len(), width.to_bits(), …)` — a **heap
  address**. There is no eviction and no cap; the map survives `exit_overlay` for the whole
  process lifetime.
- While drawing a freehand stroke, every mouse move adds a point → new `len` → cache miss →
  a new `ID2D1PathGeometry` containing *all points so far* is inserted. A 1,000-point
  stroke leaves ~1,000 geometries holding ~500k Bézier segments — O(n²) memory, forever.
- Dragging a stroke clones its `Vec` every mouse move (`overlay.rs` ~1898–1905) → new
  pointer → one more cached geometry per move. Arrow heads are keyed on absolute float
  coordinates (~238–244), so dragging a bound arrow adds an entry per frame.
- **Stale-draw bug:** because freed `Vec` allocations get reused, a *different* stroke with
  the same length/width can collide with a dead key and draw the **old geometry at the old
  position** while hit-testing uses the real points.

**Fix (in this order):**
1. Add a `revision: u32` field to `Annotation` (`src/types.rs`), defaulting to 0 and
   `#[serde(default)]` so old sessions still load. Bump it in every place that mutates the
   shape: `translate_shape`/resize application sites, nudge, undo/redo restore, text commit.
   The safest catch-all: bump it wherever `shapes[i].shape` is assigned or mutably borrowed
   for editing — search `overlay.rs` for `annotation_index` and `annotation_mut` call sites.
2. Change the stroke geometry cache key to `(annotation_id, revision, width.to_bits(), …)`.
   The render path must therefore pass the id+revision down — `render_single_shape` already
   receives the `Annotation`; confirm and plumb through where only `&Shape` is passed.
3. **Never cache the in-progress stroke** (the one being drawn) or shapes mid-drag: if the
   caller has no committed annotation id, build the path uncached each frame (perf for the
   live stroke is handled separately in P4).
4. Replace the arrow-head coordinate key the same way (id + revision + head type), or —
   simpler and acceptable — stop caching arrow heads entirely: a 3–4 point path per frame
   is cheaper than hashing plus unbounded growth. Prefer the simpler option.
5. Bound the cache: on insert, if `len > 512`, clear it (or implement a simple LRU if a
   crate-free one already exists in the codebase; a full clear at 512 is fine).
6. Clear the cache in `exit_overlay` and whenever shapes are deleted/cleared (search for
   `HistoryAction::Clear`).

**Verify:** draw one long continuous scribble for ~20 seconds while watching the process in
Task Manager — memory must stay flat (before the fix it climbs continuously). Drag a stroke
around rapidly — it must always render at the position where it is hit-testable.

## P2-2 · Image pixels are deep-copied on every operation; undo history is unbounded

**Files:** `src/types.rs` (`ImagePixels` ~605–610), `src/overlay.rs` (history fields
~140–141; `push_shape` ~1299; drag ~1881/~1898–1905; drag release ~2058–2074;
`nudge_selection` ~2145–2169; `HistoryAction::Clear` ~1494).

**Problem.** `ImagePixels { bgra: Vec<u8> }` derives `Clone`, so every `Shape::clone()` of a
pasted image copies the full buffer (a 4K paste ≈ 33 MB; a 1080p one ≈ 8 MB). Consequences:
- Dragging a selected image deep-copies it **twice per mouse move** (~16 MB/move for 1080p).
- Every move/resize pushes `TransformShapes { (id, before, after) }` — two more full copies
  into undo history, which has **no cap** (no `truncate`/`MAX_UNDO` anywhere).
- Holding an arrow key on a selected image (`nudge_selection` fires per key repeat) grows
  history by hundreds of MB per second.
- `push_shape` clones on insert, doubling paste cost.

**Fix (in this order):**
1. Change `bgra: Vec<u8>` to `bgra: std::sync::Arc<[u8]>`. Serde: serialize through the
   existing base64 path — adjust the (de)serialize impls/annotations so the wire format is
   **unchanged** (old session files must still load; add a round-trip check by loading a
   session saved before the change). After this, `Shape::clone()` on images is O(1) and
   most of the memory blow-ups disappear without touching call sites. Fix the handful of
   places that mutate pixels in place, if any (search for `.bgra`) — use `Arc::make_mut`
   semantics or rebuild the Arc.
2. Cap undo: after every push to `undo_history`, if `len > 200`, `remove(0)` (or use a
   `VecDeque`). Same for `redo_history` (it is naturally bounded by undo, but cap anyway).
3. Merge key-repeat nudges: in `nudge_selection`, if the last history entry is a
   `TransformShapes` for the same id set pushed within the last ~500 ms, update its `after`
   shapes instead of pushing a new entry. Store an `Instant` alongside or on the entry.
4. Optional (only if easy after 1–3): store translations as `(id, dx, dy)` deltas in a new
   `HistoryAction::Translate` variant instead of before/after shape pairs.

**Verify:** paste a full-screen screenshot, hold an arrow key for 5 seconds — memory stays
flat, undo still steps back sensibly (merged nudges = one undo step per burst is fine).
Dragging the image is visibly smoother. Old session files still load.

## P2-3 · GPU image cache never evicts; weak content key can show the wrong image

**Files:** `src/renderer/mod.rs` (`image_cache` ~87, `image_bitmap` ~376–415,
`recover_if_device_lost` ~570–597), `src/types.rs` (`cache_key` ~617–639).

**Problem.** One GPU bitmap per distinct pasted image, never removed on delete/undo/overlay
exit; `recover_if_device_lost` clears every cache **except** this one (and if the new
render target allocates at the same address, `cache.0 == rt_id` matches and dead-device
bitmaps get reused). Separately, `cache_key` samples only ~4096 bytes, so two screenshots
of the same window differing in unsampled bytes collide — the second image renders **and
exports** as the first.

**Fix:**
1. Compute a full content hash **once** when an `ImagePixels` is created (paste/session
   load), store it as a field (e.g. `content_hash: u64`), and use it as the cache key.
   Combine naturally with P2-2's `Arc` change (hash at construction).
2. Clear `image_cache` in `recover_if_device_lost` and in `exit_overlay`.
3. After deletions/undo/board close, evict entries whose hash no longer appears among live
   shapes **or** history (simplest: clear the whole cache when a `Shape::Image` is deleted;
   re-upload on next frame is cheap enough).

**Verify:** paste screenshot A of a window, change one small thing, paste screenshot B —
both render correctly. Delete both, confirm (debug log or debugger) the cache empties.

## P2-4 · Clipboard: `HGLOBAL` leaks on error paths, clipboard wiped on failure, unsafe DIB read

**File:** `src/clipboard.rs`. Note `GlobalFree` is currently not even imported.

**Problems & fixes (all in one pass):**
1. ~62–66: if `GlobalLock(h_global)` returns null → `CloseClipboard(); return false;` leaks
   `h_global`. Add `let _ = GlobalFree(h_global);` before returning.
2. ~106: `SetClipboardData(CF_DIB, …)` — the system owns the handle **only on success**.
   Change to `if SetClipboardData(...).is_err() { let _ = GlobalFree(h); }`. At 3× export
   scale on 4K this leak is ~300 MB.
3. ~132–141: same for the PNG path — free `h_png` when `png_ptr.is_null()` and when
   `SetClipboardData(cf_png, …)` fails (its result is currently discarded with `let _ =`).
4. ~48: `EmptyClipboard()` runs before the allocation can fail, wiping the user's clipboard
   on failure. Reorder: allocate and fill both `HGLOBAL`s first, then
   `OpenClipboard` → `EmptyClipboard` → `SetClipboardData`.
5. ~27: `OpenClipboard(None)` followed by `EmptyClipboard` is documented to make
   `SetClipboardData` unreliable — pass the overlay `HWND`.
6. `read_dib` (~222): it dereferences the `BITMAPINFOHEADER` **before** checking size.
   Add up-front: `GlobalSize(h) >= size_of::<BITMAPINFOHEADER>()` and `biSize >= 40`, and
   move the `pixels` pointer computation (~246) after the existing `needed` check.
   (Crafted-clipboard crash, low likelihood, cheap fix.)

**Verify:** Ctrl+C and Ctrl+Shift+E exports still paste into Paint/Word as both DIB and
PNG. Ctrl+V of an image still works.

## P2-5 · WGC capture rigs accumulate across display changes; frame-pool callback race

**Files:** `src/capture_wgc.rs` (`RIGS` ~57, keyed by raw `HMONITOR` ~158; `FrameArrived`
~189–195; `CaptureSignal::drop` ~330), `src/main.rs` (`WM_DISPLAYCHANGE` ~219–230).

**Problem.** `HMONITOR` values are reissued after dock/undock/resolution changes. Rigs
(each holding a monitor-sized `Direct3D11CaptureFramePool`, 8–33 MB GPU) are only replaced
when the *same* key changes size — rigs for vanished monitors live forever. Also, the
`FrameArrived` handler is never unregistered, and dropping a `Rig` closes the event handle
while the free-threaded pool may still fire the callback into a closed/reused `HANDLE`.

**Fix:**
1. Add `pub fn reset()` to `capture_wgc` that clears `RIGS` (keep `DEVICE`), and call it
   from the `WM_DISPLAYCHANGE` arm in `main.rs`.
2. Keep the `EventRegistrationToken` returned when registering `FrameArrived`; implement
   `Drop for Rig` that calls `pool.RemoveFrameArrived(token)` then `pool.Close()` **before**
   the event handle is closed.

**Verify:** build passes; change display resolution while the app runs, then open the
overlay — capture still works. (GPU-memory verification is optional.)

## P2-6 · No teardown at process exit — ⚠ verify first, low priority

**Files:** `src/main.rs` (~493 mutex handle, ~587 `OleUninitialize`), `src/overlay.rs`
(`Rc::into_raw` in `GWLP_USERDATA` ~466, reclaimed only in `WM_NCDESTROY` ~6267),
`src/settings_window.rs` (same pattern ~266; its `Drop` ~2890 can never run because the
userdata `Rc` keeps it alive — a cycle).

**Problem.** Nothing calls `DestroyWindow` on the overlay or settings window, so their
state (renderer, `LiveZoomEngine`, `FreeLibrary`) never drops; `OleUninitialize` runs while
D2D/DWrite factories and WGC objects are alive. The OS reclaims it all at exit, so this is
about clean shutdown ordering (and correct magnifier reset), not a runtime leak.

**Fix.** Before leaving `main`: `DestroyWindow` the overlay and settings windows (which
triggers `WM_NCDESTROY` → `Rc` reclaim), drop the app context, then `OleUninitialize()`.
Close the single-instance mutex handle on the early-exit path (~493).

**Verify:** app exits cleanly from the tray menu with no crash-on-exit, and if Live Zoom
was active the screen returns to 1× (already handled by `stop()`, confirm still true).

---

# P3 — Data loss & undo corruption

## P3-1 · Save/autosave writes only the active board; exit destroys the rest

**Files:** `src/overlay.rs` (`save_session` ~3123–3132, `exit_overlay` ~996),
`src/session.rs`.

**Problem.** `save_session` early-returns if the **active** board's `shapes` is empty and
serializes only `self.shapes` (the active board). `exit_overlay` then resets
`self.boards = vec![Board::default()]`. With autosave on: draw on board 1, Ctrl+T, draw on
board 2, exit → board 1 is gone; if the active board is empty, **nothing** is saved.

**Fix.**
1. Extend the session format: add a `boards: Vec<BoardData>` field (each with its shapes)
   plus `active_board: usize`, with `#[serde(default)]` so **old single-board files still
   load** (map a legacy file to one board). Bump the session format version field if one
   exists (check `session.rs`).
2. In `save_session`, first park the live state into `boards[active_board]` (the same sync
   `switch_board` does — reuse that code), then serialize all boards. The "empty" early-out
   must check *all* boards.
3. In `load_session_from`, restore all boards and the active index.

**Verify:** autosave on → draw on two boards → exit → reopen → load the session: both
boards present, correct one active. An old session file (make one before the change) still
loads.

## P3-2 · Exiting or cancelling while editing text deletes the text

**File:** `src/overlay.rs` — `exit_overlay` (~988), `reopen_selected_text` (~2183),
`edit_container_label` (~2972), right-click cancel (~4978–4980), `load_session_from` (~3178).

**Problem.** Re-opening a text for editing **removes the annotation from `shapes`** and
holds it only in the editor. `exit_overlay` does `self.text_editor = None;` without
committing — and `save_session` runs *before* that, so the autosave is missing the text
too. Trigger: double-click a text, then press global Ctrl+2 / toolbar Close / Timer close.
Right-click during an edit and `load_session_from` drop the edit the same way, so "cancel"
on a re-opened text means "delete".

**Fix.**
1. At the **start** of `exit_overlay` and `load_session_from`: if `self.text_editor` is
   `Some`, call `self.commit_text_editor()` (commit, don't discard).
2. For explicit cancel (right-click, Esc-as-cancel if it exists): keep the original
   `Annotation` in `TextEditorState` when re-opening (see P3-8 — same storage), and on
   cancel re-insert the original instead of dropping it.

**Verify:** double-click an existing text → type → press Ctrl+2 → reopen overlay: the
edited text is there. Double-click → right-click: the *original* text is back, unchanged.

## P3-3 · Manual Ctrl+Shift+S saves are silently pruned

**Files:** `src/overlay.rs` (~3155), `src/session.rs` (`prune` ~215–231).

**Problem.** `prune(&dir, cfg.session_keep_last)` runs on **every** save, manual included
(`session_keep_last` defaults to 10). A user who never enabled autosave loses their oldest
manual session at the 11th save, silently.

**Fix.** Pass a flag into `save_session` (or split the function): prune **only** on the
autosave path. Manual saves never prune.

**Verify:** with keep-last = 2, make 3 manual saves — all 3 files exist. Trigger 3
autosaves — pruned to 2.

## P3-4 · Settings window and overlay overwrite each other's `config.json`

**Files:** `src/settings_window.rs` (snapshot at `show()` ~335, full write on Save ~757),
`src/overlay.rs` (`save_config` ~941).

**Problem.** Settings snapshots the whole config when opened and writes the whole snapshot
on Save — reverting anything the overlay wrote in between (`toolbar_custom_position`,
`recent_custom_colors`, `export_scale`, `toolbar_collapsed`, `timer_duration_mins`).
Conversely the overlay's `save_config` writes `default_stroke_width = self.stroke_width`
(and `default_color`, `default_fill_mode`, `default_stroke_pattern`) on every exit — i.e.
whatever tool state was last active clobbers the user's configured defaults.

**Fix.**
1. In Settings Save: **reload** the config from disk, apply only the fields the dialog owns
   onto the freshly-loaded value, save that. (List the dialog-owned fields explicitly.)
2. In the overlay: stop writing tool state into the `default_*` fields. If "remember last
   used tool state" is wanted, add separate `last_*` config fields with
   `#[serde(default)]`; the defaults remain user-owned. Simplest correct fix: overlay's
   `save_config` also reload-then-merge, writing only the fields the overlay owns
   (`toolbar_custom_position`, `recent_custom_colors`, `export_scale`,
   `toolbar_collapsed`, `timer_duration_mins`) and **not** the `default_*` ones.

**Verify:** set default stroke width 6 in Settings → open overlay, use the highlighter,
exit → reopen Settings: still 6. Move the toolbar, then open Settings and Save → toolbar
position survives.

## P3-5 · Toolbar Undo during a text edit duplicates the text

**File:** `src/overlay.rs` — `handle_fluent_action` (~4529–4531 dispatch, Undo arm ~3632),
`undo()`/`redo()` (~1396).

**Problem.** Toolbar clicks bypass the editor: Undo pops the `DeleteShape` recorded when
the text was re-opened, restoring the original while the editor still holds its copy —
Esc then commits a second copy.

**Fix.** At the top of `handle_fluent_action`, `undo()` and `redo()`: if a text editor is
open, `commit_text_editor()` first (matching what keyboard shortcuts presumably do —
verify Ctrl+Z during edit and make the behaviours identical).

**Verify:** double-click a text → click toolbar Undo → Esc. Exactly one copy of the text
exists.

## P3-6 · Re-anchoring an arrow endpoint is invisible to undo

**Files:** `src/overlay.rs` (`select_release` → `rebind_endpoint` ~2059–2061, mutation
~2725–2729; `undo()` calls `settle_bindings()` ~1396), `src/types.rs` (`TransformShapes`
~1011).

**Problem.** `TransformShapes` records only the `Shape` geometry, not
`start_bound`/`end_bound`. Undo restores the geometry, then `settle_bindings` snaps the
arrow right back to the **new** binding — undo appears to do nothing (or, for a detach,
the arrow stops following its old target).

**Fix.** Snapshot bindings alongside the transform: extend the history item pushed by
`select_release` (and any other path that calls `rebind_endpoint`) to carry
`(start_bound, end_bound, group, container)` before/after per id — either widen
`TransformShapes` items or add a paired `SetBindings` action pushed in the same user step
(if history entries are applied one-per-undo, widening `TransformShapes` is the right
call; check how `undo()` pops). Apply them in `undo()`/`redo()` before `settle_bindings()`.

**Verify:** drag an arrow end from box A to box B → Ctrl+Z → the arrow is bound to A again
(move A to confirm it follows). Detach an end → Ctrl+Z → it follows again.

## P3-7 · Duplicate copies group membership and arrow bindings

**File:** `src/overlay.rs` ~2629–2630 (`let mut copy = source.clone(); copy.id = ShapeId::fresh();`).

**Problem.** The copy keeps `group`, `start_bound`, `end_bound`. A duplicated grouped shape
joins the original's group (drag one → both move). A duplicated bound arrow gets settled
back onto the same targets, landing exactly on top of the original — invisible.

**Fix.** In the duplicate routine: clear `copy.start_bound = None; copy.end_bound = None;`.
For groups: if the duplication is of a whole group's members in one action, remap to one
fresh group id shared by the copies; otherwise `copy.group = None`. (Check whether Ctrl+D
duplicates a full group selection — handle both cases.)

**Verify:** group two boxes, Ctrl+D with one selected → dragging the copy does not move the
originals. Ctrl+D on a bound arrow → the copy is visible at the duplicate offset and does
not snap onto the original.

## P3-8 · Re-edited text is a new annotation — bindings, group and opacity are lost

**File:** `src/overlay.rs` — `commit_text_editor` (~2225 → `push_shape` → `Annotation::new`).

**Problem.** Editing an existing text removes the original and commits a **fresh**
annotation: new id, `group = None`, `opacity = 1.0`. Any arrow bound to the text stops
following; group membership and opacity silently reset.

**Fix.** Store the full original `Annotation` in `TextEditorState` when re-opening
(`reopen_selected_text`, `edit_container_label`). On commit, restore id/group/opacity/
container onto the committed annotation and record a `TransformShapes`-style edit in
history instead of Delete+Add. (This also provides the "original" that P3-2's cancel path
re-inserts — implement the storage once, in whichever of the two items you do first.)

**Verify:** bind an arrow to a text, set its opacity to 50%, group it with a box.
Double-click, edit, Esc → the arrow still follows, opacity still 50%, still grouped, and a
single Ctrl+Z reverts just the text change.

## P3-9 · Loading a session lets group ids collide with new ids

**File:** `src/session.rs` ~183–185.

**Problem.** After load, the id counter is reserved above the highest **annotation** id
only — but group ids (and container/binding ids) come from the same counter and are
usually higher. Fresh ids then collide: grouping two new shapes can reuse a loaded group's
id, merging unrelated shapes into one group.

**Fix.** Reserve above the max across `a.id`, `a.group`, `a.container`, `a.start_bound`,
`a.end_bound` (each where `Some`).

**Verify:** group shapes, save, restart, load, group two other shapes → the two groups stay
independent.

## P3-10 · Ctrl+W destroys a board with no undo and no confirmation

**File:** `src/overlay.rs` ~1582.

**Problem.** `boards.remove(active_board)` drops the board's shapes and history outright;
Ctrl+W is a reflexive "close" key.

**Fix (pick the lighter one):** keep the last N closed boards in a
`closed_boards: Vec<Board>` and let Ctrl+Shift+T restore the most recent; **or** show a
toast "Board closed — Ctrl+Z to restore" and push a history action. Do not add a modal
confirmation dialog (this app avoids them).

**Verify:** draw on a board, Ctrl+W, restore — the shapes are back.

---

# P4 — Performance

Context that applies to every item here: drawing happens only in `WM_PAINT`
(`overlay.rs` ~3928); every trigger is a full-screen `InvalidateRect(hwnd, None, false)`
(`request_repaint`, ~1263), so per-frame cost is O(all shapes) and every "per frame" item
also runs **per mouse move**. A 16 ms `SetTimer` runs while the overlay is visible.

Do these in order — P4-1 alone removes most per-frame cost of committed content, which
shrinks several later items to "nice to have".

## P4-1 · Retained scene layer — the biggest single win

**Files:** `src/renderer/mod.rs` (`render_frame` ~763–797), `src/overlay.rs`.

**Change.** Render all **committed** shapes into a cached offscreen target and blit it,
instead of re-rendering every shape every frame:
1. Add to the renderer: `scene_layer: Option<ID2D1BitmapRenderTarget>` (created via
   `rt.CreateCompatibleRenderTarget`) plus `scene_layer_revision: u64`.
2. Add a `scene_revision: u64` counter on the overlay state; increment it on **any** shape
   mutation (add/delete/move/resize/undo/redo/board switch/property change), and on
   zoom/pan change, background change, and DPI change. Route mutations through the same
   choke points as P2-1's revision bumps.
3. In `render_frame`: if `scene_layer_revision != scene_revision` (or the layer is None /
   wrong size), re-render committed shapes into the layer once, under the current
   zoom/pan transform. Then per frame draw: background bitmap → one
   `DrawBitmap(scene_layer)` → the in-progress shape (live stroke / drag preview) →
   selection chrome, toolbar, HUD.
4. During a select-drag, at drag start render the **unselected** shapes into the layer once
   and draw only the dragged selection live each frame; on release, invalidate.
5. Recreate the layer on `recover_if_device_lost` and clear it in `exit_overlay`.

**Pitfalls:** blur shapes sample the background — render order inside the layer must stay
identical to today's `render_frame`; text editor content and the shape being drawn are NOT
committed and stay live; the layer must be recreated when the render target is (device
loss), and it is a device resource — do not let it leak into the export path (export keeps
rendering shapes directly).

**Verify:** with ~50 shapes including text and a pasted image, drag-select and move a
shape — visibly smoother, and everything still renders identically (compare a Ctrl+C export
before/after the change pixel-by-pixel if unsure). Blur, spotlight and zoom modes still
correct.

## P4-2 · Select-drag deep-clones the selection twice per mouse move

**File:** `src/overlay.rs` — drag move (~1881, ~1898–1905, ~1994–2003), release (~2058),
`settle_bindings` (~2662–2695).

**Change** (P2-2's `Arc` already removed the worst cost for images; this removes the rest):
1. Stop `sel.originals.clone()` per move — iterate by reference, or `mem::take` and put
   back.
2. Reuse allocations: `shapes[i].shape.clone_from(&orig)` then translate in place, instead
   of building a fresh `Vec<(id, Shape)>` of clones.
3. Maintain a `HashMap<ShapeId, usize>` id→index map, rebuilt whenever `shapes` order/len
   changes; replace `annotation_index` linear scans in the drag path.
4. `settle_bindings`: process only arrows whose `start_bound`/`end_bound` is in the moved
   id set; borrow targets instead of cloning; write back only when endpoints changed.
5. On release (~2058): drop the full `PartialEq` shape compare (an 8 MB memcmp for an
   image before P2-2; still O(points) for strokes) — track a `moved: bool` flag from the
   gesture instead.

**Verify:** drag a selection of a long stroke + an image + two bound arrows: smooth, arrows
follow, undo of the drag still works.

## P4-3 · Pen-down clones the whole canvas for snap anchors, then discards them

**File:** `src/overlay.rs` — pen-down (~4811–4816), `rebuild_snap_anchors_excluding`
(~3435–3447), `begin_drag` (~1769).

**Problem.** Pen-down calls `rebuild_snap_anchors(None)` — which does
`.map(|a| a.shape.clone()).collect()` over **all** shapes (all stroke points, all image
pixels pre-P2-2) — and then immediately `clear_snap()`s it for Pen/Highlighter. This sits
on the first-point latency of every freehand stroke.

**Fix.**
1. Skip the rebuild entirely when `current_tool` is Pen or Highlighter.
2. In `rebuild_snap_anchors_excluding`, build anchors from `&Annotation` without cloning:
   iterate `self.shapes.iter().filter(…)` and compute the 9 anchor points per shape
   directly.
3. Make the `skip` lookup a `HashSet<ShapeId>` (it is `contains` on a Vec today).

**Verify:** first point of a pen stroke lands with no hitch even with many shapes; snapping
while drawing rectangles/arrows still works.

## P4-4 · Pressure strokes issue one round-capped `DrawLine` per segment per frame

**File:** `src/renderer/shapes.rs` ~76–92.

**Problem.** 50 pressure strokes × 500 points ≈ 25k `DrawLine` calls per frame.

**Fix.** P4-1 already removes the per-frame cost for committed strokes. On top of that, at
stroke commit build a single filled outline polygon (offset each point along its normal by
the half-width for its pressure; join the two sides into one closed `ID2D1PathGeometry`)
and cache it per annotation id+revision (the P2-1 cache). For the **live** stroke, keep a
cached geometry of the committed prefix and draw only the new tail segments with
`DrawLine`.

**Verify:** draw pressure strokes with a pen/tablet — identical appearance (compare
end caps and width transitions), smooth while many strokes exist.

## P4-5 · Text layouts rebuilt every frame — and on every mouse move via hover

**Files:** `src/renderer/shapes.rs` (free text ~484–595, labels ~947/~983, editor ~1280),
`src/renderer/mod.rs` (`measure_text_block` ~469–510, label owner lookup ~772–780),
`src/overlay.rs` (`shape_bounds_exact` ~1624–1646 reached from `WM_SETCURSOR` ~4146).

**Fix.**
1. Cache an `IDWriteTextLayout` + metrics per annotation, keyed by
   `(id, revision, font key, wrap width)` — a small HashMap on the renderer, cleared with
   the other caches. Draw with `DrawTextLayout` (not `DrawText`, which builds an internal
   layout each call), and read measurement from the same cached layout so measure+draw is
   one layout, not two/three.
2. Label owner lookup (~772–780) uses the P4-2 id→index map instead of `iter().find`.
3. Text editor: rebuild the layout only when the text/caret **content** changes, not per
   frame; draw the caret as a line from `HitTestTextPosition` instead of inserting `'|'`
   into a copied string.
4. `shape_bounds_exact` on hover: return cached metrics (from 1) instead of calling
   `measure_text_block`.

**Verify:** open the text editor on a long text — CPU while idle-blinking drops. Text still
wraps, centres and hit-tests identically.

## P4-6 · Toolbar: ~25 brush creations and ~35 implicit text layouts per frame

**File:** `src/renderer/ui_toolbar.rs` (brush sites: ~45, 76, 79, 84, 117, 129, 189–194,
318, 338, 365, 473, 491, 502, 741, 752; plus `renderer/mod.rs` ~868 timer dim brush).

**Fix.**
1. Replace every direct `rt.CreateSolidColorBrush` with the existing cached helper
   `self.solid_brush(rt, &color)` — mechanical.
2. The glyph/label strings are static: pre-build `IDWriteTextLayout`s per item in
   `update_layout` and reuse.
3. Stop mutating the shared `text_format_toolbar_small` per item (see P5-5).
4. (Optional, after 1–3, only if the toolbar still shows in profiles): render the toolbar
   to a cached bitmap invalidated on hover/active/layout change.

**Verify:** toolbar renders identically in expanded, collapsed, sub-bar and tooltip states.

## P4-7 · Idle 60 fps full-screen repaints: caret, toast, timer

**Files:** `src/overlay.rs` (animation tick ~4027–4115), `src/types.rs` (toast timings
~1964–1980), `src/renderer/ui_timer.rs`.

**Problem.** The 16 ms tick forces `needs_paint = true` for: an open text editor (60 fps
for a caret that flips every 500 ms), a live toast (1.8 s at 60 fps though opacity only
changes during 0.15 s fade-in/0.35 s fade-out — and toasts fire on nearly every action),
and Timer mode (60 fps for digits that change once per second).

**Fix.** In the tick, compute the next time each active animation actually changes its
pixels and repaint only when due:
- caret: repaint on the 500 ms toggle boundary;
- toast: repaint every tick **only** during fade phases; once fully opaque, schedule a
  single repaint at fade-out start;
- timer: repaint when `(minutes, seconds, arc_segment_index, paused, hovered)` changes —
  compute `arc_segment_index = elapsed * 128 / total`;
- when nothing is animating and no input arrived, skip `request_repaint` entirely, and
  kill the 16 ms timer when no animation is active (it is already killed on hide; extend
  to idle — make sure smooth pan/zoom and the laser re-arm it).

**Verify:** open the overlay, open a text editor, leave the mouse still — GPU/CPU usage in
Task Manager drops to near zero between caret blinks. Toasts, laser trails, smooth zoom
and the timer all still animate correctly.

## P4-8 · Brush cache flushes itself under the picker, laser and toast fades

**Files:** `src/renderer/mod.rs` (`solid_brush` ~328–357: full `clear()` above 256
entries), `src/renderer/ui_picker.rs` (~72–96), `src/renderer/shapes.rs` (laser
~1459–1560), `src/renderer/ui_hud.rs` (toast ~289–423).

**Problem.** The picker's hue drag generates ~128 new colours per frame (3 bars × 64
strips), overflowing the 256-entry cache and wiping **all** brushes — including every
shape brush — every couple of frames. Laser alpha fades and toast fades cause slower
churn of the same kind.

**Fix.**
1. Picker: draw each bar with one `ID2D1LinearGradientBrush`, rebuilding gradient stops
   only when the hue changes (hue bar's stops are constant).
2. Laser/toast: keep one scratch `ID2D1SolidColorBrush` on the renderer and call
   `SetColor`/`SetOpacity` before each draw (D2D captures brush state per draw call) —
   don't go through the cache for time-varying alphas.
3. Replace the hard `clear()` at 256 with dropping ~half the entries (or a proper LRU if
   trivially available).

**Verify:** drag the hue slider — no hitching; shapes on the canvas don't flicker.

## P4-9 · Blur re-renders its mosaic every frame; two blurs thrash a single-slot cache

**Files:** `src/renderer/shapes.rs` (~796–848), `src/renderer/mod.rs` (`blur_rt_cache` ~82).

**Problem.** Each blur does an offscreen `BeginDraw/DrawBitmap/EndDraw` pass per frame even
though its source (the frozen background) never changes; the cache holds one (size, RT)
pair, so two blurs of different sizes each `CreateCompatibleRenderTarget` every frame.
Also **⚠ verify first:** two same-size blurs share one `tiny` bitmap while the main RT's
`DrawBitmap` is deferred — the second mosaic may overwrite the first before flush; check
visually with two overlapping-size blurs.

**Fix.** Cache the **finished mosaic bitmap** per blur annotation, keyed by
`(annotation id, revision, background generation)`. Rebuild only when the blur rect/block
size changes or the background is recaptured; the per-frame draw becomes one `DrawBitmap`.
This subsumes the single-slot cache (remove `blur_rt_cache` once nothing uses it). Note the
DPI fix in P5-3 changes the source rect — do P5-3 first or together.

**Verify:** place 3 blur regions of different sizes — all correct, both on screen and in a
PNG export; frame cost no longer scales with blur count.

## P4-10 · Every export wipes all render caches → hitch after Copy/Save/PDF

**File:** `src/renderer/mod.rs` (`render_to_capture` ~1039; each cache is keyed by
`rt.as_raw()`).

**Problem.** Export builds a fresh DC render target; the first draw on it sees a new RT
pointer and clears the brush/geometry/image caches, and the next on-screen frame clears
them again and re-uploads every image — a visible hitch after every Copy/Save/PDF/autosave
PNG.

**Fix.** Give the export path its own cache instances (simplest: a second set of cache
fields used only by `render_to_capture`, cleared after the export finishes), so the
on-screen caches keyed to the hwnd RT are never touched. Alternatively make each cache a
map keyed by RT pointer with per-RT storage — choose whichever is less invasive given how
the caches are accessed (they live on `self` and take `rt` as a parameter; the second
cache-set option is usually mechanical).

**Verify:** with many shapes + a pasted image, press Ctrl+C repeatedly — no frame hitch
after each copy.

## P4-11 · Smaller per-frame allocations and linear scans — one sweep, low risk

Apply after everything above; skip any that P4-1 already made irrelevant in profiles.

- Curved lines/arrows: `stroke_polyline` (`renderer/shapes.rs` ~1088–1112) creates a path
  geometry per shape per frame; `sample_curve` runs twice per arrow. Cache per
  id+revision via the P2-1 cache.
- Hexagon badge (~688), snap-marker diamond (~1028), timer arc + play triangle
  (`ui_timer.rs` ~619, ~815): cache the path per parameters, or build the timer arc with
  `D2D1_ARC_SEGMENT`s keyed by segment count.
- StepBadge `number.to_string()` + snap-badge `format!` per frame (~714–752, ~1716, ~1782,
  ~1848): cache the strings/layouts on state change.
- Loupe `CreateBitmapBrush` every frame (`ui_loupe.rs` ~44): create once per background
  bitmap, update only its transform per frame (combine with the P5-2 DPI fix).
- HUD `format!` ×3 + `DrawText` per frame (`ui_hud.rs` ~200–249): cache on state change.
- Cheat sheet: ~130 static `DrawText` calls per frame while open (`ui_hud.rs` ~630–744):
  render once to a bitmap on open/DPI change, then blit.
- Selection bounds (`overlay.rs` ~1650–1664) recomputed from `WM_SETCURSOR` per mouse move
  and per paint, with an O(n) `annotation(id)` per selected id: cache the bounds,
  invalidate on selection/shape change; during a Move drag use `original_bounds + (dx,dy)`.
- Eraser drag (~4342–4349) tests every stroke segment per move: add a cached bounding-box
  prefilter per shape (bbox from the P4-5/P2-1 revision-keyed caches).
- `laser_trail.remove(0)` (~4312): make it a `VecDeque`.
- `validate_selection` (~1743) and marquee release (~2011–2046): use `HashSet`s for the
  `contains` checks.

**Verify:** behaviour identical; spot-check each touched feature (badges, snapping, loupe,
HUD, cheat sheet, eraser, marquee).

## P4-12 · Release profile

**File:** `Cargo.toml` — add:

```toml
[profile.release]
lto = "thin"
codegen-units = 1

[profile.dev]
opt-level = 1
```

**Verify:** `cargo build --release` succeeds; binary still runs.

---

# P5 — DPI & rendering correctness

Background for P5-2/P5-3: the frozen screenshot bitmap is created at 96 DPI
(`capture.rs` ~210) so its DIP size equals its pixel size, while the hwnd render target
runs at the monitor's DPI (`renderer/mod.rs` ~538). Any code that treats "1 bitmap unit ==
1 screen DIP" is wrong at scaling above 100%. The scale factor to use is
`let s = bg_bitmap.GetSize().width / screen_w_dips;` (equivalently `dpi/96`).

## P5-1 · Settings window: clicks land on the wrong control above 100% DPI

**File:** `src/settings_window.rs` (~394–406 mouse handling; layout in DIPs; `rt.SetDpi`
~325).

**Problem.** Hit-testing uses raw physical mouse coordinates against DIP layout rects. At
125%, clicking the visible **Cancel** actually hits **Save** (applies settings!); at 150%
most controls are unreachable.

**Fix.** In every mouse message handler, divide x/y by `GetDpiForWindow(hwnd) as f32 / 96.0`
before hit-testing. Handle `WM_DPICHANGED`: resize the window to the suggested rect and
update the render target DPI.

**Verify:** at 150% display scaling, every tab, checkbox, slider and button responds
exactly under the cursor; Cancel cancels.

## P5-2 · Loupe magnifies the wrong area above 100% DPI

**File:** `src/renderer/ui_loupe.rs` (~51–58, brush transform).

**Problem.** The bitmap-brush matrix assumes one bitmap unit per screen DIP; at 150% the
loupe shows content from `cursor/1.5`, drifting further from the cursor toward the
bottom-right of the screen.

**Fix.** With `let s = bg.GetSize().width / screen_w;` set `M11 = M22 = m / s` (where `m`
is the magnification) and keep `M31 = cx*(1.0-m)`, `M32 = cy*(1.0-m)`.

**Verify:** at 150% scaling, the loupe shows exactly what is under the cursor at all four
screen corners. At 100% behaviour is unchanged (`s == 1`).

## P5-3 · Blur samples the wrong region above 100% DPI

**File:** `src/renderer/shapes.rs` (~837–842).

**Problem.** The source rectangle passed for the mosaic is in canvas DIPs, but the bitmap's
units are physical pixels — at 150% the mosaic shows content from ~2/3 of the way toward
the origin. (SVG export applies `dpi_scale` and is correct, so screen and SVG currently
disagree.)

**Fix.** Multiply `l, t, r, b` by the same `s` from P5-2 before using them as the bitmap
source rect. Coordinate with P4-9 (mosaic caching) — do this first.

**Verify:** at 150%, a blur placed over a recognizable region pixelates exactly that
region, and PNG/SVG exports agree with the screen.

## P5-4 · Blur reveals the hidden desktop on Whiteboard/Blackboard

**Files:** `src/renderer/mod.rs` (`render_frame` ~770 passes `bg_bitmap` regardless of
background type), `src/overlay.rs` (`export_svg` ~3308–3311), `src/svg_export.rs`
(`write_blur` ~894).

**Problem.** On a Whiteboard/Blackboard the frozen screenshot is retained and blur happily
mosaics it — showing a pixelated copy of the desktop the user deliberately covered. On
screen and in SVG it leaks the desktop; in PNG it renders a grey box. Three outputs, two of
them privacy leaks.

**Fix.** Wherever blur receives its source bitmap/pixels, pass `None` when
`background_type != CanvasBackground::Transparent` — in `render_frame`, in the PNG export
path, and in `export_svg`'s `SvgExportInput` (its doc comment already says it should be
None for Whiteboard/Blackboard). Blur with no source should render as a flat block in the
board's background colour.

**Verify:** whiteboard → draw a blur → it shows a flat block, not desktop content — on
screen, in PNG and in SVG.

## P5-5 · Shared cached `IDWriteTextFormat`s are mutated in place

**Files:** `src/renderer/shapes.rs` (StepBadge ~742–744), `src/renderer/ui_timer.rs`
(~710–712, ~740–742), `src/renderer/ui_toolbar.rs` (sub-bar ~684–689 sets NO_WRAP).

**Problem.** `SetTextAlignment`/`SetParagraphAlignment`/`SetWordWrapping` are called on
formats shared through the format cache — after a step badge draws, any Text annotation of
the same rounded size renders centred; the toolbar leaks NO_WRAP into tooltips/toasts.

**Fix.** Include alignment + wrapping in the format-cache key (extend
`get_text_format`/`get_custom_text_format` with those parameters), or set alignment on the
per-draw `IDWriteTextLayout` instead of the shared format. Pick one approach and apply it
at all three sites.

**Verify:** draw a step badge, then create a text annotation at a similar size — it is
left-aligned as normal.

## P5-6 · Oversized exports come out blank; oversized pastes are invisible

**Files:** `src/renderer/mod.rs` (`EndDraw` result discarded ~1203; `CreateBitmap` ~404–411),
`src/clipboard.rs` (~235 allows pastes up to 32768 px).

**Problem.** A 3× export of a very wide display exceeds the GPU bitmap limit; `EndDraw`
fails but the result is ignored and a blank image is saved as success. A pasted image
taller than `GetMaximumBitmapSize()` (~16384) fails bitmap creation with `.ok()?` and
renders as an invisible-but-selectable shape.

**Fix.**
1. Check the `EndDraw` HRESULT in `render_to_capture`; on failure return an error and show
   the existing toast/error path instead of writing the file.
2. Clamp the export supersample factor so `dim * scale <= rt.GetMaximumBitmapSize()`.
3. On paste, if either dimension exceeds `GetMaximumBitmapSize()` (query it, don't hardcode
   16384), downscale the pixels before creating the shape (simple box filter is fine) and
   toast "Image was scaled down".

**Verify:** paste a very tall screenshot (e.g. a full-page browser capture) — it appears,
scaled. (The giant-monitor export case can only be verified by clamping logic review.)

## P5-7 · Emoji and other non-BMP characters cannot be typed

**File:** `src/overlay.rs` ~5175 (`char::from_u32(wparam.0 as u32).unwrap_or('\0')`).

**Problem.** `WM_CHAR` delivers UTF-16 surrogate pairs as two messages; each half fails
`char::from_u32`, becomes `'\0'` and is dropped — the Win+. emoji picker types nothing.

**Fix.** Keep a `pending_high_surrogate: Option<u16>` on the overlay state. In the
`WM_CHAR` handler: if the code unit is a high surrogate (0xD800–0xDBFF), stash it and
return; if a low surrogate (0xDC00–0xDFFF) and a high is pending, combine
(`0x10000 + ((hi-0xD800)<<10) + (lo-0xDC00)`) into a char; otherwise clear the pending
stash and process normally.

**Verify:** in a text annotation, insert 😀 via Win+. — it appears, saves, and survives a
session round-trip and SVG export.

## P5-8 · Export mismatches — spotlight position, missing SVG pan, editor chrome baked in

**Files:** `src/renderer/mod.rs` (~1182–1188), `src/svg_export.rs` (~69–70), plus SVG text
robustness (`esc()` ~1044–1056).

Three small fixes in one pass:
1. **Spotlight exported at the wrong place when zoomed:** `render_to_capture` (~1187–1188)
   converts `spotlight.x/y` with `canvas_to_screen`, but those values are already screen
   DIPs (set at `overlay.rs` ~4370). Remove the conversion.
2. **SVG drops a 1× infinite-canvas pan:** `svg_export.rs` ~69–70 applies the view
   transform only when `zoom > 1.001`; PNG applies it always. Change the condition to also
   apply when the pan offset is nonzero.
3. **PNG/PDF bake in text-editor card chrome** (~1182–1184) while SVG omits the text being
   edited entirely: commit or hide the editor for exports — simplest is to have the export
   path draw the editor's *text content* without the card chrome, or (simpler still and
   consistent with P3-2) commit the editor before any export. Choose committing.
4. **SVG text robustness:** in `esc()`, strip XML-illegal control chars
   (U+0000–U+001F except tab/LF/CR); add `xml:space="preserve"` on text elements; make the
   number formatter guard non-finite values (write `0` and log).

**Verify:** spotlight + static zoom → Ctrl+C matches the screen. Whiteboard panned at 1× →
SVG matches PNG. Export during a text edit → the text appears exactly as committed, no
card. Text containing a pasted control character still yields an SVG that opens in a
browser.

---

# P6 — Hardening & polish

Small items, batched by file. Each still gets its own commit if it changes behaviour.

## P6-1 · Session/config file robustness (⚠ crafted-input class)

- `types.rs` ~741: `raw.width as usize * raw.height as usize * 4` — use `checked_mul`,
  reject the image on overflow, and cap dimensions at load (e.g. 32768) so a crafted
  session cannot make `want == 0` and accept an empty buffer.
- `renderer/mod.rs` ~408: pitch `pixels.width * 4` computed in u32 — widen to usize with
  checked math.
- `svg_export.rs` ~953: `vec![0u8; (cols * rows * 4) as usize]` is u32 math that can wrap →
  later out-of-bounds indexing. Use usize + `checked_mul`, skip the shape on overflow.
- `config.rs` (~149): add `#[serde(default)]` **at the struct level** on `AppConfig` so a
  hand-edited file missing one field doesn't reset every setting. Clamp loaded values:
  `default_stroke_width` to `>= 0.5`, `timer_duration_mins >= 1`, zoom bounds sane.
- `main.rs` ~402: `timer_seconds = cfg.timer_duration_mins * 60` — guard with `.max(1)` and
  saturating math (already guarded at `overlay.rs` ~287; make the two agree).

## P6-2 · Input & device edge cases

- **`pen_active` sticks** (`overlay.rs` ~3897–3925): reset it in `exit_overlay` and handle
  `WM_POINTERCAPTURECHANGED` / `POINTER_FLAG_CANCELED`; a stuck flag blocks all mouse input.
- **Live Zoom Ctrl+Wheel** (`live_zoom.rs` ~44): `GetKeyState` inside the low-level mouse
  hook reads the hook thread's stale state; use `GetAsyncKeyState(VK_CONTROL)`.
- **Monitor unplugged** (`main.rs` ~219–229): after `refresh_monitors`, if the overlay's
  current monitor no longer exists, re-bind it to the primary (recompute `screen_x/y/w/h`,
  clear capture per P0-2's rule); also re-validate `current_monitor_index` since the list
  re-sorts.

## P6-3 · Small UX consistencies

- Reset Toolbar Position (`main.rs` ~315–317) passes physical pixels to `update_layout`;
  use `logical_w()`/`logical_h()` — mis-centred above 100% DPI.
- Tray Laser/Eraser/Blur (`main.rs` ~275/283/291) and the K key (`overlay.rs` ~5802) set
  `current_tool` without `sync_tool_to_toolbar` — toolbar highlights the wrong tool.
- Draw-mode Up/Down (`overlay.rs` ~6195/6234) and the Settings default width (`main.rs`
  ~400) change `self.stroke_width` but pen strokes use `pen_settings.stroke_width`
  (hardcoded 3.0 at `overlay.rs` ~361) — make pen width follow the same setting, or exclude
  the pen from the toast text.
- Timer card `custom_pos` never clamped/reset (`exit_overlay`) — clamp into the monitor on
  show so it can't be stranded off-screen.
- `read_dib` (`clipboard.rs`): V4/V5 headers with `BI_BITFIELDS` are misparsed (adds 12
  bytes although the masks live inside V4/V5 headers) and `biClrUsed` palettes are ignored
  — fix offset computation per header size, and reject unsupported palettized formats
  cleanly rather than reading garbage.

---

## Done criteria

Work is complete when every P0–P3 item is fixed and verified, P4-1..P4-10 are done, and
P5 is done. P4-11/P4-12 and P6 are best-effort. After each phase, do a manual smoke pass:
draw each tool, select/move/resize/undo, switch boards, save+load a session, export
PNG/SVG/PDF/clipboard, run once at 150% display scaling, and once with a second monitor if
available.
