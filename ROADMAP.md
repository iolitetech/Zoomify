# Zoomify roadmap

Running list of what is built, what is deliberately not, and what is queued.
Effort labels are rough: **S** a sitting, **M** a few sittings, **L** a project.

Agreed order of work: **B (diagramming) → A (recording) → the rest.**

---

## Shipped

| Area | Commit |
|---|---|
| 24 audited bug fixes, custom colour picker, multi-line text | `828aa3a` |
| Editable annotations — select, move, resize, delete | `44e6ec1` |
| Save & reload sessions (autosave, folder, PNG, keep-last-N) | `2981f36` |
| Pen/touch with pressure, Windows.Graphics.Capture | `7a00b90` |
| Snap to other shapes while drawing and dragging | `e4755f0` |
| Stable annotation identity (`Annotation { id, shape, … }`) | `04047de` |
| Text bound inside a shape, centred and reflowing | `4d465b6` |
| Arrows anchored to the shapes they point at | `8fcefde` |
| Capture pipeline cached and pre-warmed (201ms → 119ms) | `4002f3b` |
| Arrow rendering rebuilt (was falling apart above ~8px) | `6ecfd08` |
| Restack (`Ctrl+[` / `Ctrl+]`) and duplicate (`Ctrl+D`) | `af6fa3c` |
| Multi-select: marquee, Shift+click, group edit | _pending_ |

### Design decisions worth not re-litigating

- **Derived, not stored.** A container's label is laid out from the container's
  bounds every frame, so it follows moves and resizes with no bookkeeping.
  Bound arrow endpoints use a variant: a settle pass rewrites the stored
  geometry after any mutation, so hit-testing, bounds, snapping and export all
  keep reading plain shapes and need no idea bindings exist.
- **Identity, not indices.** Relationships between annotations have to survive
  deletion, undo and reordering. A position in a `Vec` survives none of those.
- **Autosave defaults off.** It writes files on every Esc; that should be asked
  for, not assumed.
- **No always-on capture session.** It would remove the last ~18ms of overlay
  open time but keeps the GPU compositing frames nobody asked for, and can
  raise the system screen-capture indicator.

---

## Known gaps and constraints

- **Toolbar is at 13 tools** and crowded. New tools need a rethink of the bar
  (customisation, or overflow) rather than another icon.
- **Canvas settings tab is full** — cards reach 468px against a 504px footer.
  Anything new needing a setting forces a reflow of that page. Snapping already
  had to settle for a config key plus the Alt gesture.
- **Overlay open is ~119ms** with WGC, ~101ms without. The residue is one
  session start plus one compositor frame.
- **In-overlay keys are not rebindable.** Only the six global hotkeys are.
- **Text is one flat `String`** per annotation. Rich text needs a run model.
- **One monitor at a time.** `Tab` cycles; there is no all-displays canvas.

---

## A. Recording & capture

The WGC work already gives a D3D11 device, a cached per-monitor capture item and
a frame pool. Recording is keeping the session running instead of closing it and
feeding each `FrameArrived` texture to a Media Foundation `IMFSinkWriter`.

| Item | Effort | Notes |
|---|---|---|
| MP4, video only | **L** | Sink writer + H.264, frame pacing, dropped frames, stop hotkey, recording indicator |
| …with annotations composited in | **M** on top | The offscreen export target already exists. ZoomIt cannot do this |
| …with audio | **L** again | WASAPI loopback + mic, device pick, A/V sync |
| Animated GIF of a short clip | **M** | Same frame source, simpler encoder |
| Region snip → clipboard | **S** | Was removed in `65b5fe7`; could return |
| Scrolling capture | **L** | Scroll injection and stitching |

## B. Diagramming — **next up**

| Item | Effort | Notes |
|---|---|---|
| Arrow endpoint re-targeting | **S** | Today a grip *scales* a bound arrow instead of re-binding the end. Most obviously missing piece |
| Arrow labels | **S** | Text at the midpoint; container text does the hard part already |
| Sticky notes | **S** | Filled box + label in one gesture |
| Align & distribute | **S** | Natural now multi-select exists |
| Arrowhead variants | **S** | Open "V", circle, diamond, bar. `arrow_head_points` is already isolated |
| Per-shape opacity | **S** | One field, one brush alpha |
| Curved / elbow arrows | **M** | Multi-point arrows with routing |
| Grouping (`Ctrl+G`) | **M** | Persistent groups, not a transient selection |
| Connectors routing around shapes | **L** | Real pathfinding |
| Layers | **L** | New model concept |

## C. Text

Rich text (per-run bold/italic/colour), alignment inside a container, auto-fit
font size, bullet lists. All **M** — each needs `Shape::Text` to carry runs
rather than one flat string.

## D. Export & interop

| Item | Effort |
|---|---|
| Paste an image from the clipboard onto the canvas | **S** — conspicuously missing |
| Drag-and-drop an image file | **S** |
| Export above screen resolution | **S** |
| SVG export | **M** — makes the vector work reusable elsewhere |
| PDF export | **M** |

## E. Workflow

| Item | Effort |
|---|---|
| Eyedropper — pick a colour off the screen | **S** |
| First-run onboarding | **S** |
| Toolbar customisation (which tools show) | **M** — see the crowding constraint |
| Multiple boards / pages to tab between | **M** — highest value here for presenting |
| Command palette (`Ctrl+K`) | **M** |
| Infinite canvas past screen bounds | **M** |
| Rebindable in-overlay keys | **M** |

## F. Multi-monitor

Annotate **all displays at once** instead of `Tab` cycling. **L**: the overlay
is one window sized to one monitor, so this is either a window per display or
one spanning the virtual desktop, plus per-monitor DPI on a single canvas.

## G. Robustness & distribution

Logging and crash reports (**S**), portable mode with config beside the exe
(**S**), installer via MSI or winget plus code signing so SmartScreen stops
warning (**M**), auto-update (**M**). Individually small; together the
difference between a personal tool and one other people install.

---

## Testing notes

Features here are verified by driving the real app from PowerShell and
measuring pixels — see `probe_*.ps1` in the session scratchpad. Things that
bit more than once:

- Set a colour **before** opening the text editor; inside it a bare letter is
  typed, not interpreted as a hotkey.
- The selection chrome is light blue and snap guides are pink, so use **green**
  when a probe needs to tell annotation ink apart from app furniture.
- Shell dialogs need ~3s to take focus before typing a path, or the keystrokes
  land in the overlay as tool hotkeys.
- PowerShell variable names are case-insensitive: a `-Alt` switch shadows an
  `$ALT` constant.
- Running several probes back-to-back can flake on app-ready timing; re-run a
  single failure before believing it.
