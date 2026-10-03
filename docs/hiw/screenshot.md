# Screenshot: configuration, controls and implementation

## Status and scope

This document describes the implemented built-in screenshot editor, its KDL settings,
bindings and IPC API. The editor uses Cairo for strokes and the toolbar, the existing
GPU blur, and a shared composition for the preview, magnifier and export.

Select, Draw, Arrow, Rectangle and Blur are implemented. Text input is not implemented
and is outside the current contract.

The goal is to customize appearance, bindings and behavior without a separate UI
framework, a universal theme engine or a second binding system.

Agreed decisions:

- The root block is named `screenshot`, not `screenshot-ui`.
- All shortcuts belong in the existing root `binds` block, not inside `screenshot`.
- There is no `editor` section: the initial tool and captured-pointer visibility are
  arguments of the opening `screenshot` action.
- Brush, Arrow and Rectangle have independent colors, widths and palettes, without
  inheriting brush settings. A stripe at the bottom of each button shows its color;
  a number at the top right shows its width. Reactivating the selected button toggles
  its palette only with `open-palette-on-click=true`; `screenshot-tool` never opens it.
- The magnifier has independent initial zoom, visibility and grid thresholds.
  There is no separate "show magnifier" toggle or binding.
- The zoom adjustment step is an action argument in the binding.
- Margins are specified independently for all four sides.
- `screenshot-path` and `screenshot-format` remain root settings;
  `screenshot-ui-open` stays in the existing animations section.
- Blur shows its level using three bottom segments filled from left to right.
  `screenshot-context` adjusts the keyboard-focused tool, falling back to the tool
  under the pointer, without selecting it or wrapping values; for Blur it adjusts
  strength. The tooltip names the level.
- The shared axes mode applies to Brush, Arrow and Rectangle. Its separate indicator
  is before Copy/Save. Both dashed guides have the same color and opacity.
- Before a drag, the intersection follows the pointer. During a drag it is fixed;
  `switch-threshold` defines the ray-switching zone in logical pixels. Drawing
  coordinates are constrained, not the system pointer.
- Releasing and pressing hold again during a drag moves the intersection to the
  current drawing point without a jump. The shape's origin stays unchanged; only
  releasing the mouse completes the edit. A rectangle can therefore be drawn in one
  drag: width, reanchor, height.
- `screenshot-context` first uses keyboard focus from `screenshot-focus-next` /
  `screenshot-focus-previous`, then the pointer. A focused tool adjusts its parameter
  without becoming selected; Select and other UI controls block the command. Pointer
  movement clears focus. All pointer-routing rules below apply only without keyboard
  focus. Explicit `screenshot-magnifier-zoom` is independent of focus.
- `screenshot.axes` settings: `color "#80c7ff"`, `opacity 0.4`, `width 1`,
  `dash-length 6`, `dash-gap 4`, `switch-threshold 8`. All dimensions are positive
  logical pixels. Guides and the brush outline belong only to Overlay UI, not export.
- One axes command serves both modes: `screenshot-toggle-axes` toggles the state;
  `screenshot-toggle-axes hold=true` temporarily enables it until the physical control
  is released. `hold=false` is the default; IPC supports only toggle. The separate
  `screenshot-axes-hold` command no longer exists.
- The block is named `axes`, not `axis`, and has no `enabled` setting. Each session
  starts with axes disabled.

## Implementation map

| File | Responsibility |
| --- | --- |
| `niri-config/src/lib.rs` | `Config::screenshot`, decoding and defaults |
| `niri-config/src/screenshot.rs` | Typed schema, defaults and range validation |
| `niri-config/src/binds.rs` | Action decoding, the `tool` argument and editor-local action classification |
| `niri-ipc/src/lib.rs` | Corresponding IPC/CLI actions and argument types |
| `src/input/mod.rs` | Local binding priority, keyboard, wheel and physical ownership of move/axes holds |
| `src/ui/screenshot_ui.rs` | Session, selection, settings application, commands and composition |
| `src/ui/screenshot_editor.rs` | Strokes, arrows, rectangles, blur, independent tool parameters, palette and toolbar caches |
| `src/ui/screenshot_axes.rs` | Toggle/hold, independent anchor and ray projection |
| `src/ui/screenshot_guides.rs` | Cached guide strips and brush outline |
| `src/render_helpers/shaders/screenshot_magnifier.frag` | Grid-threshold and cross-highlight uniforms |
| `src/niri.rs` | Opening arguments and preservation of the existing export path |
| `resources/default-config.kdl`, `resources/knit-desktop.kdl` | Explicit examples of the new bindings |
| `src/tests/screenshot_editor.rs`, `src/tests/screenshot_shapes.rs`, `src/tests/binds.rs` | Behavior, pixels, shapes, context and input ownership boundaries |

Reuse existing configuration primitives for colors, numbers and size changes rather
than introducing an alternative parser.

## Main configuration example

This example uses the current defaults. UI dimensions are logical pixels; brush
width and selection offsets/sizes use frozen-frame pixels. Eight-digit HEX uses
`#RRGGBBAA`. Color alpha is multiplied by the element's opacity.

```kdl
screenshot-path "~/Pictures/Screenshots/%Y-%m-%d_%H-%M-%S.{ext}"
screenshot-format "png" // png, webp, avif.

screenshot {
    toolbar {
        // Screen edge [top, bottom].
        position "top"
        // Alignment along the selected edge [left, center, right].
        align "center"

        // Independent minimum margins from the four screen edges.
        margin top=8 right=8 bottom=8 left=8

        // Button and SVG icon sizes; toolbar height is calculated automatically.
        button-size 40
        icon-size 24

        // Shared toolbar, palette and tooltip theme.
        background-color "#12121a"
        border-color "#ffffff29"
        border-width 1
        accent-color "#80c7ff"
        icon-color "#ffffffeb"
        disabled-icon-color "#ffffff4d"
        hover-color "#ffffff1f"
        pressed-color "#00000059"

        // Opacity of the entire toolbar [0.0–1.0].
        opacity 1.0
        // During gestures; 1.0 with opacity=1.0 disables fading.
        drag-opacity 0.5
    }

    tooltips {
        // Show tooltips on hover or focus [true, false].
        enabled true
        font "Sans"
        font-size 11
    }

    selection {
        border-color "#ffffff"
        border-width 2
        // Dimming applies only to the frozen frame outside the selection.
        dim-color "#000000"
        dim-opacity 0.5
        // Restore the previous selection [true, false].
        remember-selection true
    }

    brush {
        // Width of new strokes, in captured-frame pixels.
        width 4
        // Color of new strokes when the editor opens.
        color "#ff0000"
        // One to seven opaque colors; indices follow list order.
        palette "#ff0000" "#ff8000" "#ffe000" "#00c040" "#0080ff" "#ffffff" "#000000"
        // Reactivating the selected brush toggles its palette [true, false].
        open-palette-on-click true
        // Close the palette after selecting a color [true, false].
        close-palette-on-select true
    }

    arrow {
        width 4
        color "#ff0000"
        palette "#ff0000" "#ff8000" "#ffe000" "#00c040" "#0080ff" "#ffffff" "#000000"
        open-palette-on-click true
        close-palette-on-select true
    }

    rectangle {
        width 4
        color "#ff0000"
        palette "#ff0000" "#ff8000" "#ffe000" "#00c040" "#0080ff" "#ffffff" "#000000"
        open-palette-on-click true
        close-palette-on-select true
    }

    axes {
        color "#80c7ff"
        opacity 0.4
        width 1
        dash-length 6
        dash-gap 4
        switch-threshold 8
    }

    blur {
        // Blur strength [low, medium, high].
        strength "medium"
    }

    magnifier {
        diameter 192
        // Inward border width in logical pixels; 0 disables it.
        border-width 2
        border-color "#ffffff"
        // Grid color with alpha, independent of the row/column highlight.
        grid-color "#0000008c"
        pointer-gap 16
        // Zoom on each opening [1, 2, 4, 8, 16, 32].
        initial-zoom 1
        // The lens is visible from this zoom level [2, 4, 8, 16, 32].
        show-from 2
        // Maximum zoom [2, 4, 8, 16, 32].
        max-zoom 32
        // Show the pixel grid [true, false].
        grid true
        // Independent grid threshold [2, 4, 8, 16, 32].
        grid-from 4
        // Highlight the center pixel's row and column [true, false].
        cross-highlight true
        cross-highlight-color "#a6d9f259"
    }
}
```

`top` uses the top margin; `bottom` uses the bottom margin. `left` and `right` use
the corresponding side margins. Centering uses the available rectangle between the
margins, so unequal `left`/`right` values shift its center. On small outputs the
toolbar scales down uniformly rather than wrapping buttons. Palettes and tooltips
open toward the screen center: below a top toolbar and above a bottom toolbar.
Their positions are constrained to the available area.

`drag-opacity` is an absolute opacity, not a multiplier of `opacity`. It applies
while selecting, moving the selection, drawing or blurring. Button presses and
color selection do not enable it. Release/cancel restores normal opacity.

## Separate bindings example

This is a root-level sibling of `screenshot`. The initial tool belongs to a specific
opening action, not general appearance. `show-pointer` already exists; `tool` and
the local actions below support KDL, CLI and IPC, except physical hold modes, which
are available only in bindings.

```kdl
binds {
    // Open: [select, draw, arrow, rectangle, blur], pointer [true, false].
    Alt+U { screenshot tool="select" show-pointer=false; }
    Alt+Shift+U { screenshot tool="draw" show-pointer=false; }

    // The following actions apply only while the screenshot editor is open.
    // Selecting the brush with a key does not open its palette.
    V { screenshot-tool "select"; }
    B { screenshot-tool "draw"; }
    A { screenshot-tool "arrow"; }
    D { screenshot-tool "rectangle"; }
    X repeat=false { screenshot-toggle-axes; }
    Shift_L repeat=false { screenshot-toggle-axes hold=true; }
    MouseRight repeat=false { screenshot-toggle-axes hold=true; }
    R { screenshot-tool "blur"; }
    C { screenshot-toggle-palette; }

    // Select a color by position; only works while the palette is open.
    "1" { screenshot-palette-color 1; }
    "2" { screenshot-palette-color 2; }
    "3" { screenshot-palette-color 3; }
    "4" { screenshot-palette-color 4; }
    "5" { screenshot-palette-color 5; }
    "6" { screenshot-palette-color 6; }
    "7" { screenshot-palette-color 7; }

    // Keyboard focus first, otherwise the pointer: a number is absolute,
    // a signed string is an adjustment. Brush/Arrow/Rectangle width: 1–128;
    // Blur level: 1–3. Select and other UI are no-ops without zoom;
    // the canvas changes lens zoom. The selected tool does not change.
    Minus { screenshot-context "-1"; }
    Equal { screenshot-context "+1"; }

    Ctrl+Z { screenshot-undo; }
    Ctrl+Shift+Z { screenshot-redo; }
    Ctrl+Y { screenshot-redo; }
    P { screenshot-toggle-pointer; }

    // Copy without a file / save and copy.
    Ctrl+C { confirm-screenshot write-to-disk=false; }
    Ctrl+S { confirm-screenshot write-to-disk=true; }
    // One-shot format override: does not change the config or future exports.
    Ctrl+Alt+P { confirm-screenshot format="png"; }
    Ctrl+Alt+W { confirm-screenshot format="webp"; }
    Ctrl+Alt+A { confirm-screenshot format="avif"; }
    // Activate the focused button; without focus, save and copy.
    Return { screenshot-confirm; }
    // Close palette → cancel the current gesture → close editor.
    Escape { screenshot-cancel; }
    Ctrl+Escape { cancel-screenshot; }

    // Selection position adjustments, in frozen-frame pixels.
    Left { screenshot-move-selection x=-1 y=0; }
    Right { screenshot-move-selection x=1 y=0; }
    Up { screenshot-move-selection x=0 y=-1; }
    Down { screenshot-move-selection x=0 y=1; }
    // A larger step is specified directly in another binding.
    Ctrl+Left { screenshot-move-selection x=-20 y=0; }
    Ctrl+Right { screenshot-move-selection x=20 y=0; }

    // Size adjustments; the top-left corner stays in place.
    Shift+Left { screenshot-resize-selection width=-1 height=0; }
    Shift+Right { screenshot-resize-selection width=1 height=0; }
    Shift+Up { screenshot-resize-selection width=0 height=-1; }
    Shift+Down { screenshot-resize-selection width=0 height=1; }

    // Hold while dragging the selection; Space does not save the screenshot.
    Space repeat=false { screenshot-move-selection-hold; }

    // Keyboard focus first; otherwise a tool button adjusts its parameter,
    // and the canvas steps zoom through 1× → 2× → 4× → 8× → 16× → 32×.
    // "+2"/"-2" skip a step; there is no separate scroll-step setting.
    WheelScrollUp { screenshot-context "+1"; }
    WheelScrollDown { screenshot-context "-1"; }
    // Explicit zoom is independent of focus and hover.
    Ctrl+WheelScrollUp { screenshot-magnifier-zoom "+1"; }
    Ctrl+WheelScrollDown { screenshot-magnifier-zoom "-1"; }

    Tab { screenshot-focus-next; }
    Shift+Tab { screenshot-focus-previous; }
}
```

### Action semantics and priorities

1. Commands use the existing `Action`, KDL decoding, IPC conversion and CLI.
   `Tool` and blur strength are enums; command and stroke parameters use named payloads.
2. `screenshot tool=… show-pointer=…` opens the editor. Omitting `tool` means Select;
   the existing `show-pointer=true` default is preserved. The example above explicitly
   hides the pointer. An already open session is not reopened.
3. Other screenshot editor actions are local: when the editor is closed, they are
   excluded from binding lookup **before** selecting a match. They do not consume
   client press/release events. While the editor is open, local bindings take
   precedence over ordinary bindings.
4. A `Key` may occur once in each of two categories: ordinary and screenshot actions.
   A duplicate within one category is an error. With the UI closed, the ordinary
   binding is selected.
5. Preserve permitted compositor actions, such as volume controls, and the routing
   of existing selection move/resize commands. Do not redirect them to the desktop
   while the editor is open.
6. There is no hardcoded interception of V/B/R/C/Enter/Space. Standard shortcuts are
   defined in the supplied configs; existing user configs need the local bindings
   from the example above. No binding means no action.
7. Buttons and bindings use the same domain commands. Tooltips show the actual
   configured shortcuts, or just the action name when no binding exists. Labels must
   not continue advertising old `B` or `C` shortcuts after remapping.
8. Wheel input uses the same binding lookup and existing scroll-direction rules.
   The existing ScrollTracker accumulates high-resolution deltas: one normalized
   wheel step executes one action. The supplied wheel binding is `screenshot-context`:
   keyboard focus takes priority, otherwise pointer hit-testing determines the target.
   A tool button adjusts its own parameter; Select and other UI controls absorb the
   command. Without focus the canvas changes zoom; without focus or a pointer it is
   a no-op. Pointer movement clears keyboard focus. Over the canvas, `+N/-N` specifies
   the number of zoom steps. Explicit `screenshot-magnifier-zoom` is independent of
   both focus and hit-testing. Do not execute an old direct scroll handler alongside
   the new binding.
9. While the editor is open, wheel input neither changes desktop zoom nor reaches
   applications, even without a local wheel binding. Desktop zoom is unchanged on exit.
10. Selection-move hold belongs to the physical key: store its keycode on press and
    clear it on release even after modifier changes, reload or event suppression.
    Closing/canceling the session also clears the hold. IPC does not synthesize an
    endless key hold: use movement commands, not a hold action.
11. `screenshot-tool` selects a tool without opening its palette. Repeated activation
    or Enter on the active Brush, Arrow or Rectangle respects its
    `open-palette-on-click`. The shared `screenshot-toggle-palette` command opens the
    active colored tool's palette.
12. With `close-palette-on-select=false`, the palette stays open and focus stays on the
    selected swatch; otherwise it closes and focus returns to its tool. A click outside
    the palette only closes it, without starting a stroke. A color index absent from
    the current palette does not change state.
13. Confirm/export during an unfinished gesture does not save a partial edit. Cancel
    first closes the palette, then cancels the gesture and restores the crop, then
    closes the UI. Undo during a stroke cancels that stroke, not the previous edit.
14. `screenshot-toggle-axes hold=true` tracks the hold owner: a key, mouse button or
    stylus button. An unrelated release does not end the hold; with multiple owners,
    axes stay enabled until the last release. Hold does not repeat even with
    `repeat=true`. Wheel/touchpad scroll triggers have no release and are rejected.
    Without `hold`, or with `hold=false`, the command toggles the persistent state.

## Editor IPC contract

The editor uses the existing `Request::Action` → `niri_ipc::Action` →
`niri_config::Action` → `niri msg action` chain. KDL, IPC and buttons invoke the same
domain commands; there is no separate socket, protocol or namespace.

### Commands and arguments

The common `niri msg action` prefix is omitted below. Kebab-case names refer to
CLI/KDL; JSON retains the existing PascalCase variants and snake_case fields.

| CLI | Contract |
| --- | --- |
| `screenshot --tool draw --show-pointer false --path /tmp/shot.png` | Open the editor. `tool`: `select`, `draw`, `arrow`, `rectangle`, `blur`; defaults to `select`. An already open session is unchanged. |
| `screenshot-tool draw` | Select a tool without opening its palette. Close the previous tool's palette. |
| `screenshot-toggle-palette` | Toggle the active Brush, Arrow or Rectangle palette. |
| `screenshot-set-palette true` | Explicitly open/close the active colored tool's palette; opening in Select/Blur is a no-op. |
| `screenshot-palette-color 3` | Select a one-based swatch index; only while the palette is open. Apply `close-palette-on-select`. |
| `screenshot-brush-color "#0080ff"` | Set opaque RGB directly, independently of palette and selected tool. Do not toggle the palette or switch tools. |
| `screenshot-arrow-color "#00ff00"` / `screenshot-rectangle-color "#0080ff"` | Set the specified tool's own opaque color. |
| `screenshot-context "-1"` / `screenshot-context 8` | Keyboard focus first, then the pointer. Draw/Arrow/Rectangle adjust their own width (1–128), Blur its strength (1–3), without selecting the tool. Select and other UI are no-ops without zoom. With no focus the canvas changes lens zoom; without focus or a pointer it is a no-op. A number sets an absolute value from 1–128; a signed string adjusts. The result clamps to the target range; zoom rounds down to a power of two within `max-zoom`. Pointer movement clears focus. |
| `screenshot-toggle-axes` / `screenshot-set-axes true` | Shared axes mode. Does not complete a gesture; switching tools does not reset it. |
| `screenshot-toggle-axes hold=true` (KDL only, not CLI/IPC) | Hold a key, mouse button or stylus button. Releasing the last owner restores toggle state; pressing again reanchors the current gesture. |
| `screenshot-undo` / `screenshot-redo` | Shared edit history; an empty stack is a no-op. Undo during a canvas gesture cancels only that gesture, not the previous edit. |
| `screenshot-toggle-pointer` | Toggle the captured pointer in the image, not the editor's system pointer. |
| `screenshot-set-pointer false` | Explicitly set the same state without toggling. |
| `screenshot-move-selection --x -20 --y 0` | Move the crop in physical captured-output pixels; omitted axes default to 0. Preserve size and clamp position to the frame. |
| `screenshot-resize-selection --width 20 --height 0` | Adjust crop size; omitted axes default to 0. Keep the top-left corner fixed, with a minimum of 1×1 and a maximum at the frame boundary. |
| `screenshot-magnifier-zoom 4` | Set 4× zoom; absolute values are 1, 2, 4, 8, 16, 32. `+1`/`-1` adjust the number of steps, not the coefficient; clamp to the current `max-zoom`. |
| `screenshot-focus-next` / `screenshot-focus-previous` | Navigate available controls; include swatches in the focus order while the palette is open. |
| `screenshot-confirm` | Equivalent to Enter: activate the focused control, or save and copy without focus. Do not use as unconditional saving from a script. |
| `confirm-screenshot --write-to-disk true [--format FORMAT] [--quality QUALITY]` | Export the crop and copy regardless of focus; `false` means PNG clipboard only. Format precedence: explicit argument → current `screenshot-format` → PNG. Supported formats: png, webp, avif. The path comes from session opening or `screenshot-path`. |
| `screenshot-cancel` | One Escape step: close palette → cancel gesture and restore crop → close UI. |
| `cancel-screenshot` | Immediately close the editor without export and discard edits, unlike the stepwise `screenshot-cancel`. Available through KDL, CLI and IPC. |

`screenshot-screen` and `screenshot-window` retain their existing arguments and
semantics: they do not open the editor, export its Canvas or accept `tool`.
`screenshot-move-selection-hold` and the `hold=true` property of
`screenshot-toggle-axes` are **not exposed through IPC**: IPC has no paired physical
release. Use increments for movement and toggle or `screenshot-set-axes` for axes.
Magnifier visibility follows zoom and `show-from`, with no separate show/hide action.
Appearance is configured in KDL; current blur strength changes via `screenshot-context`.

### Repeatability and state transitions

- All local commands are no-ops when the editor is closed: they neither open the UI
  nor retain settings for the next session. This preserves the Undo/Redo contract.
- `set-pointer`, `set-palette`, `set-axes`, absolute `screenshot-context`, absolute zoom
  and RGB are idempotent: repeating them does not toggle state back. Toggle,
  increments, Undo/Redo and stepwise cancellation are not idempotent.
- Export and `screenshot-confirm` are no-ops during unfinished gestures. Do not save
  partial strokes or unfinished crops. A rendering error leaves the editor open and
  shows the existing error message.
- During a gesture, tool/color/parameter/crop/focus/palette changes and Redo are no-ops:
  `screenshot-context` does not change tool parameters mid-gesture. Cancellation,
  Undo for canvas gestures, closing, lens zoom, axes state and captured-pointer
  visibility changes remain available; canvas zoom through `screenshot-context`
  also works during a drag. Undo during a selection gesture or control press is a
  no-op; use cancel to undo such a gesture.
- Current color, tool parameter, zoom and pointer visibility belong to the session:
  IPC does not rewrite the config or create hidden persistent settings. Each tool
  has its own current parameter value. Color/parameter changes do not alter completed
  strokes.
- Commands act on the current selection output. They do not accept an arbitrary
  output or move the crop between monitors. Screen logical coordinates and desktop
  zoom do not participate in increment calculations.
- Session locking and permissions use the existing action dispatcher; IPC has no
  separate route around capture restrictions.

### Types, argument validation and replies

`ScreenshotTool` is the shared KDL, clap and serde enum with
`select`/`draw`/`arrow`/`rectangle`/`blur` variants. Adjustable values use the shared
`ScreenshotValueChange` enum: `Set(value)`/`Adjust(delta)`. On the wire, absolute
values are numbers and adjustments are signed strings. For zoom, a number denotes
the coefficient. `screenshot-context` resolves its target through focus, then the
pointer: Draw/Arrow/Rectangle width, Blur level or canvas zoom. Lens zoom rounds down
to a power of two and clamps to `max-zoom`. RGB uses `#RRGGBB`, without alpha.

Boolean CLI arguments explicitly accept `true`/`false`. For negative numbers and
adjustment strings, clap must allow a leading hyphen so that `-1` is not interpreted
as an option name. Overflowing numbers and invalid enums, RGB, adjustment formats
or absolute zoom values are rejected before execution. An absolute
`screenshot-context` value outside 1–128 is an error: validation does not know the
session context; runtime clamps to the target range (Blur 1–3, canvas zoom rounded
down to a power of two within `[1, max-zoom]`). Relative adjustments stop at the target
bounds without wrapping. Palette indices outside 1–7 are errors; valid indices absent
from the current palette are no-ops. Calculate sums without integer overflow.

Retain the existing `Reply = Result<Response, String>`:

- Invalid request/arguments → existing error reply and nonzero CLI exit code;
  validation is identical for raw JSON and CLI.
- Valid action → `Response::Handled` after event-loop processing, including no-ops.
  This confirms **neither** a state change **nor** a file written to disk.
- Capture completion uses the existing `ScreenshotCaptured` event; `path: null` means
  no saved-file path is available. The event has no request ID and is not a correlated
  reply to a specific command when multiple clients are active.

This contract adds no state query, new events, session ID, file wait or transactional
batch commands. An external GUI or guaranteed save confirmation would require a
separate Request/Response extension, not a redefinition of `Handled`.

Wire-format examples:

```json
{
  "Action": {
    "Screenshot": {
      "tool": "draw",
      "show_pointer": false,
      "path": "/tmp/shot.png"
    }
  }
}
```

```json
{ "Action": { "ScreenshotSetPointer": { "show_pointer": false } } }
```

```json
{ "Action": { "ScreenshotContext": { "change": "+1" } } }
```

```json
{ "Action": { "ConfirmScreenshot": { "write_to_disk": true } } }
```

In `Screenshot`, an omitted `tool` deserializes as Select so existing opening requests
continue to work; do not rename the existing `show_pointer` and `path` fields. JSON
fields for the new commands are `tool`, `open`, `index`, `color`, `change`,
`show_pointer`, `enabled`, `x`/`y`, `width`/`height`, `zoom`, `write_to_disk`, `format`
and `quality`. Argument-free variants retain `{}`.

### Wiring and verification

`Action::validate()` checks ranges identically for CLI and raw JSON before dispatch;
KDL uses the same argument types. State-dependent no-ops remain in the editor.
Local commands pass through the existing screenshot input filters.

Editor GPU regressions and a live X11/Mesa scenario have covered toolbar settings,
RGB/width, Undo/Redo, remapping without hidden fallbacks, magnifier thresholds,
palette, reload, PNG pixels, closed-UI no-ops and rejection of invalid CLI/JSON
arguments.

### Encoding and save paths

`ScreenshotFormat` is the shared PNG/WebP/AVIF enum in config, CLI and IPC. Only
`confirm-screenshot` accepts an override; the Save button and `screenshot-confirm`
use the config. The opening action has no `format` argument, and the session has no
separate format state. `screenshot-screen` and `screenshot-window` use the config.

`{ext}` in `screenshot-path` is replaced with the selected extension; a legacy template
extension is also normalized to the format. Explicit `--path` remains literal: an
extension conflict is rejected, leaving the editor and path available for a corrected
export.

PNG is lossless. WebP defaults to lossless; `confirm-screenshot quality=…` from 0–100
selects lossy encoding. AVIF uses `quality` 1–100 (default 90), speed 8, 8-bit, 4:4:4.
Quality is fixed in `FileExport` when preparing that export; the next export returns
to defaults. Clipboard data is always PNG. Encoding runs on the existing background
thread; PNG data is reused for the clipboard and PNG file. There is no silent
AVIF/WebP fallback to PNG. AVIF uses ravif/rav1e; optimized x86 builds require NASM.
GNOME screenshot D-Bus saves PNG regardless of the setting.

### Internal guarantees

The internal request is `ExportRequest::ClipboardOnly` or
`Save { path, format, quality }`. `ExportPlan::resolve` validates the path and resolves
the config before closing the UI; the encapsulated `FileExport` binds the destination
and selected format for the worker. `execute_action` returns only
`EditorCommand::Export` or `Close`, not an arbitrary compositor action. External
KDL/CLI/JSON contracts stay unchanged.

An open session owns a single `Interaction`: Idle, Selection, Canvas, Toolbar or
Overlay. Selection requires the original crop for cancellation and distinguishes
creation from movement through `SelectionMode`. Editor has no second gesture state.
Export readiness remains a dynamic check without typestate tokens.

Each D-Bus request has its own `Result<PathBuf>` reply channel: a write error returns
to its caller without taking the next request's reply. The PNG writer explicitly
finishes the stream and checks flush before reporting success. Without a configured
path, D-Bus creates `niri-screenshot-<random>.png` in temp_dir using `create_new`,
retrying only on name collisions. A successful file remains available to the client;
user-provided paths are not randomized.

## Configuration and reload model

- `ScreenshotConfig` contains toolbar, tooltips, selection, brush, arrow, rectangle,
  blur, axes and magnifier; arrow/rectangle are independent `ScreenshotBrush` instances.
  Configuration is separate from `Editor`/`Interaction` state.
- Keep configuration defaults separate from current color, width and zoom. Opening
  arguments take precedence only for `tool`/`show-pointer`.
- Read config on opening and handle changes through the existing reload mechanism.
  Do not clone palettes/strings or compare the whole structure every frame.
- Reload updates the open UI's theme, geometry and palette. The current RGB selection
  survives removal from the palette, with no highlighted swatch in that case. Focus
  on a removed swatch returns to the active colored tool.
- Brush/arrow/rectangle colors and widths and `magnifier.initial-zoom` are defaults
  for the next opening. Reload does not reset the current tool, independent colors
  and widths, history or crop. `remember-selection` affects the next opening.
- A new `max-zoom` clamps current zoom; lens thresholds and theme apply immediately.
  Changing `blur.strength` updates strength for new operations; unrelated reloads
  preserve the selected level.
- Binding changes rebuild displayed shortcuts. During an active gesture, defer
  toolbar geometry/hit-test changes until release/cancel; do not change a button's
  target between press and release.

### Ranges and errors

- Geometry is finite: button, icon, font and lens sizes are strictly positive;
  margins and border widths are non-negative. The icon must fit inside the button.
- Opacities are in `[0, 1]`; `drag-opacity <= opacity`.
- Brush, Arrow and Rectangle widths are `[1, 128]` pixels; Blur levels are `[1, 3]`.
  `screenshot-context` accepts absolute values 1–128 and clamps to the focused target,
  otherwise the pointer target (canvas: a power of two within `[1, max-zoom]`);
  increments clamp without wrapping. Tool and palette colors are opaque.
- A palette contains `[1, 7]` colors. The upper bound preserves a compact row and the
  existing seven-key controls without a new layout engine.
- Zoom levels are the powers of two shown in the example. `initial-zoom <= max-zoom`
  and `show-from <= max-zoom`. `grid-from` may exceed `max-zoom`: the grid then stays
  hidden until the maximum is raised. `show-from=1` is not accepted.
- Selection size is at least `1×1` and stays within the frozen output. Move/resize
  increments stop at the edge without changing image scale.
- Invalid config produces the existing diagnostics with error locations. Do not
  silently replace typos with defaults; retain the previous working config according
  to the normal reload policy.

## Magnifier: exact conditions

Session zoom is `2^exponent`. An adjustment of `+N` increases the exponent by N and
clamps it to `[0, log2(max-zoom)]`; `-N` decreases it. On each opening the exponent is
initialized from `initial-zoom`, not the previous session.

```text
lens_visible = current_zoom >= show_from
grid_visible = lens_visible && grid && current_zoom >= grid_from
```

Example: `initial-zoom=2`, `show-from=4`, `grid-from=8`. The lens starts hidden; one
step up shows it at 4× without a grid, the next shows the grid at 8×, and reducing
to 2× hides the lens.

Equality enables visibility. There is no separate closing threshold or hysteresis:
discrete steps need no additional state. Cross-highlight width remains one source
pixel; the center pixel retains its color. Nearest sampling and the circular shape
remain fixed. Do not prepare an offscreen texture while the lens is hidden; wheel
input continues to adjust zoom.

## Canvas, blur and performance

Preserve the composition order:

1. Layer 2: toolbar, palette, tooltips, selection frame, guides, brush outline and magnifier.
2. Layer 1: strokes, arrows, rectangles and blur, without dimming.
3. Layer 0: immutable frozen frame; dimming outside the crop applies only in the UI.

Export and lens contents use the same Layer 0 + Layer 1 composition without UI.
Preserve separate source textures for output/screencast/screen-capture and the
`block-out-from` rules: do not replace a protected variant with the ordinary image.

Capture `Stroke { points, color, width }` at gesture start. Bounding box, damage,
points and line rendering use the captured width, not the current global width.
Changing width neither adds a history entry nor alters old strokes. Undo/Redo replays
the original payload. Tool parameter changes mid-gesture do not alter a stroke that
has already started.

Blur operates on the frozen frame beneath drawings, not sequentially on the entire
composition. Overlapping regions have fixed strength priority: high covers medium,
and medium covers low, regardless of insertion order. Undo/Redo remains chronological.
Each blur operation stores its `BlurStrength`. Presets map to `BlurOptions` in one place:

| Preset | Implemented passes / offset | Effect |
| --- | --- | --- |
| low | 2 / 2 | Light smoothing; text may remain readable |
| medium | 3 / 3 | Medium strength, the default |
| high | 4 / 4 | Broader blending of details and color patches |

These are the current presets from `blur_options` in `src/ui/screenshot_editor.rs`,
not a future implementation plan. No level guarantees safe redaction of secrets:
blur is a visual effect.

Cache GPU blur lazily by captured-source variant and strength: at most three presets
per source, not a new texture for every rectangle. Update strokes using damage rather
than a full CPU raster and GPU upload on every pointer movement.

The toolbar cache key must account for theme, geometry and tooltip changes, but
opacity remains a render-element property: fading alone does not rerasterize SVGs.
Preserve size-keyed SVG masks with correct A8 stride and cheap ImageSurface cloning.
Icons remain embedded from `resources/icons/`; loading external SVGs and changing
button order are outside this implementation.

## Demo configuration

`resources/knit-desktop.kdl` contains a complete set of test bindings and separate
tool settings. These are not defaults: a red 4 px brush, green 8 px arrow and blue
6 px rectangle. The arrow palette stays open after selection; repeated activation
of Rectangle does not open its palette—use C/F1. The magnifier supports 32×.

- Alt+U/Print opens Select; Alt+Shift+U opens Draw without the captured pointer.
- X toggles axes; Shift_L and MouseRight enable hold.
- F1/F2 opens/closes the palette, F3/F4 shows/hides the pointer, F5/F6 enables/disables axes.
- F7/F8/F9 sets brush/arrow/rectangle colors without switching tools.
- F10 sets context to 8; F11/F12 explicitly sets zoom to 1×/32×; Ctrl+wheel steps zoom.
- Ctrl+Alt+P/W/A saves PNG/WebP quality 85/AVIF quality 90. Ctrl+Escape discards the session.
- Desktop zoom in this example uses Ctrl+Alt+Z and Ctrl+Alt+wheel, separate from editor bindings.

The full shortcut table is in the [user wiki](../wiki/Configuration:-Screenshot.md#full-feature-demo-configuration).
Run `./target/debug/niri --config resources/knit-desktop.kdl` from a Wayland session.
The config also launches Zed, Obsidian and two Ghostty windows. An isolated smoke run
checked screenshot settings and bindings without that application startup. It
verified opening, drawing with the three colored tools, F5/F6, F11/F12 and PNG export
through Ctrl+Alt+P.

## Integration

Configuration, commands, bindings and rendering share one contract, with no old
hardcoded keys or parallel control path. Schema and validation live in `niri-config`,
wire types in `niri-ipc`, and session state in `ScreenshotUi`/`Editor`.

## Acceptance and verification

- Config: defaults, partial blocks, invalid enums/colors/ranges, includes and reload.
- Decode the complete example through `Config::parse_mem`, not only a KDL AST.
- Bindings: local actions do not consume input outside the UI; remapping works for
  wheel input, another keyboard layout, key repeat and held-key release. Check the
  same trigger for ordinary and screenshot actions and duplicate errors within a category.
- Geometry: all position/alignment combinations, different side margins, narrow
  outputs, fractional scale and rotation; hit-testing matches visible buttons/palettes.
- Magnifier: both threshold boundaries, independent initial zoom, reset on reopening,
  +1/+2/-1 steps, maximum limits, threshold reloads, nearest sampling and center pixel.
- Canvas: two strokes with different widths/colors, Undo/Redo, later settings changes,
  crop intersections and different blur strengths; existing operations stay unchanged.
- UI: bottom brush color stripe, palette opening/closing, color preservation on reload,
  current shortcuts in tooltips, opacity and restoration after cancel.
- Context: focus takes precedence over hover; an unselected tool adjusts its own
  parameter without being selected; UI blocks zoom; pointer movement restores pointer routing.
- Axes: toggle and hold=true, restoring toggle after release, multiple owners,
  unrelated release, no repeat and reanchoring without completing the shape.
- Export: PNG and clipboard match the canvas without UI, dimming or magnifier;
  rendering failure neither closes the editor nor silently exports the unedited frame.
- Runtime: launch a real nested compositor, exercise mouse, keyboard and wheel input,
  inspect the displayed frame and full GL log. Check native Wayland with a hardware
  renderer and Mesa, not only headless tests.

Do not add tests for tooltip wording, copied defaults or mere forwarding.
Permanent regressions must catch user-visible behavior and boundary violations.
