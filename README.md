# Zoomify

A Windows screen overlay for presenting: zoom into the screen, draw on it, spotlight a
region, magnify with a loupe, and run a countdown timer, all from global hotkeys.

| Hotkey | Mode |
|---|---|
| `Ctrl+1` | Static freeze zoom (wheel to zoom, drag to pan, draw on top) |
| `Ctrl+2` | Draw / annotation mode |
| `Ctrl+3` | Spotlight |
| `Ctrl+4` | Live zoom |
| `Ctrl+5` | Presentation countdown timer |
| `Ctrl+6` | Magnifier loupe |

Press `F1` in an overlay mode for the full quick-reference guide. Annotations can be
copied, saved as PNG, exported as SVG or PDF, and saved as sessions.

## Install

Download `zoomify-windows-amd64.zip` from the
[releases page](https://github.com/iolitetech/Zoomify/releases), or build from source:

```
cargo install zoomify
```

Windows only (Direct2D, DirectWrite and Windows Graphics Capture). Requires Rust 1.88+
to build.

## Development

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Releases are cut with the **Publish Release** workflow; see
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
