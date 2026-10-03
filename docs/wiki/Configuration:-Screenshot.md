### Overview

The root `screenshot { ... }` block customizes the built-in interactive screenshot editor: its toolbar, tooltips, selection, brush, arrow, rectangle, blur, axis guides and magnifier. It does not affect the appearance of non-interactive `screenshot-screen` or `screenshot-window` captures.

Place this block at the top level of `config.kdl`, alongside `binds`, not inside it. All sections and settings are optional; specify only what you want to change.

The toolbar, selection frame, dimming, axis guides, brush cursor and magnifier are UI overlays and are not included in the exported image. Drawings, shapes and blur edits are included.

The available tools are Select, Draw, Arrow, Rectangle and Blur. Text annotations are not implemented.

### Default configuration

```kdl
screenshot {
    toolbar {
        position "top"
        align "center"
        margin top=8 right=8 bottom=8 left=8
        button-size 40
        icon-size 24
        background-color "#12121a"
        border-color "#ffffff29"
        border-width 1
        accent-color "#80c7ff"
        icon-color "#ffffffeb"
        disabled-icon-color "#ffffff4d"
        hover-color "#ffffff1f"
        pressed-color "#00000059"
        opacity 1.0
        drag-opacity 0.5
    }

    tooltips {
        enabled true
        font "Sans"
        font-size 11
    }

    selection {
        border-color "#ffffff"
        border-width 2
        dim-color "#000000"
        dim-opacity 0.5
        remember-selection true
    }

    brush {
        width 4
        color "#ff0000"
        palette "#ff0000" "#ff8000" "#ffe000" "#00c040" "#0080ff" "#ffffff" "#000000"
        open-palette-on-click true
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
        strength "medium"
    }

    magnifier {
        diameter 192
        border-width 2
        border-color "#ffffff"
        grid-color "#0000008c"
        pointer-gap 16
        initial-zoom 1
        show-from 2
        max-zoom 32
        grid true
        grid-from 4
        cross-highlight true
        cross-highlight-color "#a6d9f259"
    }
}
```

### Units and colors

Toolbar geometry, axis guide dimensions and magnifier diameter/gap/border width use logical pixels, so they follow output scaling. Selection border width and brush/arrow/rectangle widths use pixels of the captured frame.

UI colors support transparency; eight-digit HEX is `#RRGGBBAA`, not `#AARRGGBB`. Color alpha is multiplied by the element's opacity. For example, a half-transparent `dim-color` with `dim-opacity 0.5` produces quarter-opacity dimming.

Brush and palette colors must be fully opaque. Use `#RRGGBB` or an alpha of `ff`; translucent brush colors are rejected.

### Toolbar

| Setting               | Default       | Meaning                                                                    |
| --------------------- | ------------- | -------------------------------------------------------------------------- |
| `position`            | `"top"`       | Screen edge: `"top"` or `"bottom"`.                                        |
| `align`               | `"center"`    | Horizontal alignment: `"left"`, `"center"` or `"right"`.                   |
| `margin`              | All sides `8` | Independent non-negative `top`, `right`, `bottom`, `left` properties.      |
| `button-size`         | `40`          | Positive button size. Panel height is calculated automatically.            |
| `icon-size`           | `24`          | Positive SVG icon size, no larger than `button-size`.                      |
| `background-color`    | `"#12121a"`   | Panel background.                                                          |
| `border-color`        | `"#ffffff29"` | Panel outline.                                                             |
| `border-width`        | `1`           | Non-negative outline width; `0` removes it.                                |
| `accent-color`        | `"#80c7ff"`   | Active-control accent.                                                     |
| `icon-color`          | `"#ffffffeb"` | Normal icon color.                                                         |
| `disabled-icon-color` | `"#ffffff4d"` | Unavailable-control icon color.                                            |
| `hover-color`         | `"#ffffff1f"` | Hover background.                                                          |
| `pressed-color`       | `"#00000059"` | Pressed-control background.                                                |
| `opacity`             | `1.0`         | Resting opacity, from `0` to `1`.                                          |
| `drag-opacity`        | `0.5`         | Absolute opacity during a canvas/selection gesture, from `0` to `opacity`. |

`position "top"` uses the top margin; `"bottom"` uses the bottom margin. Center alignment centers the toolbar between the left and right margins, so unequal margins shift its center. On small outputs the toolbar scales down instead of wrapping its buttons.

The palette and tooltips open toward the center of the screen: below a top toolbar or above a bottom toolbar. Their positions are constrained to the available area. They share the toolbar theme.

`drag-opacity` is **not a multiplier** of `opacity`. It applies while selecting or moving the crop, drawing, or creating a blur rectangle. Clicking toolbar buttons or palette colors does not trigger it. Release or cancel restores normal opacity. Set both opacities to the same value to disable fading.

### Tooltips

| Setting     | Default  | Meaning                                                             |
| ----------- | -------- | ------------------------------------------------------------------- |
| `enabled`   | `true`   | Show tooltips on hover or keyboard focus. Set `false` to hide them. |
| `font`      | `"Sans"` | Font description.                                                   |
| `font-size` | `11`     | Positive font size.                                                 |

Shortcut labels reflect the configured bindings rather than a separate set of tooltip shortcuts.

### Selection

| Setting              | Default     | Meaning                                                                  |
| -------------------- | ----------- | ------------------------------------------------------------------------ |
| `border-color`       | `"#ffffff"` | Crop frame color.                                                        |
| `border-width`       | `2`         | Non-negative frame width in captured pixels.                             |
| `dim-color`          | `"#000000"` | Tint outside the crop.                                                   |
| `dim-opacity`        | `0.5`       | Dimming opacity multiplier, from `0` to `1`; `0` disables dimming.       |
| `remember-selection` | `true`      | Restore the previous selection on the next editor opening when possible. |

Dimming affects only the frozen background outside the selected region, including inactive outputs. It does not dim your drawing or become part of the saved image. Selection is limited to one output.

### Brush, arrow and rectangle

| Setting                   | Default                  | Meaning                                                               |
| ------------------------- | ------------------------ | --------------------------------------------------------------------- |
| `width`                   | `4`                      | Integer stroke width, `1`–`128` captured pixels.                      |
| `color`                   | `"#ff0000"`              | Initial opaque drawing color.                                         |
| `palette`                 | Seven colors shown above | One to seven opaque colors, in display order.                         |
| `open-palette-on-click`   | `true`                   | Repeated activation of the selected colored tool toggles its palette. |
| `close-palette-on-select` | `true`                   | Close the palette after selecting a color.                            |

Palette numbering starts at 1: `screenshot-palette-color 1` selects the first configured swatch. The initial `color` need not occur in the palette. Colors and widths chosen in the editor affect new strokes, not existing drawings.

`brush`, `arrow` and `rectangle` accept the same settings above, but store independent colors, widths and palettes; they do not inherit custom brush settings. Each button has its own bottom color stripe and a width number in the top-right corner. The brush cursor outlines its actual captured-pixel diameter. Selecting a different tool never opens its palette; activating the selected tool again toggles its palette only when `open-palette-on-click` is enabled. `screenshot-tool` never opens it. The shared palette edits only the selected colored tool.

Drag an arrow from tail to head; its head scales with the captured width. Drag a rectangle between opposite corners; it has no fill. Each gesture previews live and becomes one undo/redo operation on mouse release. Escape cancels it. Changes to tool parameters never recolor or resize completed edits.

### Blur

`strength` accepts `"low"`, `"medium"` (default), or `"high"`. It is captured when a blur gesture starts, so changing it does not alter earlier blur rectangles.

The Blur button has three bottom segments filled from left to right. Its tooltip names the level without repeating the count. Clicking it only selects Blur. `screenshot-context "+1"` and `"-1"` change the keyboard-focused tool, falling back to the tool under the pointer. For Blur this changes strength without wrapping; absolute values `1`, `2`, `3` select low, medium, high. The command does not select the tool, and only new blur regions use the new strength.

Blur affects the frozen background beneath drawings, not the drawings themselves. It is a visual effect, **not a guarantee that sensitive information cannot be recovered**.

### Axis guides

The axes mode is shared by brush, arrow and rectangle and survives tool switches. Its separate toolbar toggle sits before Copy/Save. Both dashed guides always use the same color and opacity; the chosen axis is not highlighted.

The configuration section is `axes`, not `axis`. It configures appearance and the switching threshold, not an initial enabled state: there is no `axes.enabled` setting. Axes start disabled; use the toggle, hold or explicit set action to enable them.

| Setting            | Default     | Meaning                                           |
| ------------------ | ----------- | ------------------------------------------------- |
| `color`            | `"#80c7ff"` | Guide color; supports alpha.                      |
| `opacity`          | `0.4`       | Alpha multiplier, from `0` to `1`.                |
| `width`            | `1`         | Positive line width in logical pixels.            |
| `dash-length`      | `6`         | Positive dash length in logical pixels.           |
| `dash-gap`         | `4`         | Positive gap in logical pixels.                   |
| `switch-threshold` | `8`         | Positive switching-zone radius in logical pixels. |

Before drawing, the intersection follows the pointer. Pressing the mouse anchors it; the initial movement chooses a horizontal or vertical ray. Returning to the intersection enters a small switching zone, preventing accidental tails on the opposite ray. Only drawing coordinates are constrained; the system pointer remains a free crosshair.

`screenshot-toggle-axes` toggles the mode; `screenshot-set-axes true` enables it explicitly. The local `screenshot-toggle-axes hold=true` bind enables it while its physical key is held. Releasing hold restores the toggle state. During a drag, releasing and pressing hold again reanchors at the current drawn point without jumping or completing the edit. For Rectangle, the first corner stays fixed: set the width, release/repress hold, then set the height. Only releasing the mouse completes the figure.

Hold also supports a mouse or stylus button. It ends only when that same physical control is released; releasing another button does not disable it. With multiple hold controls, the mode remains enabled until all are released. Bare modifier triggers such as `Shift_L` are supported for this action without changing ordinary key-binding matching.

`hold` defaults to `false`. It is a local bind property, not an IPC argument: IPC `screenshot-toggle-axes` always toggles. Hold requires a physical release event, so wheel and touchpad scroll triggers are rejected. Held actions do not repeat even when the bind enables repeat.

```kdl
binds {
    A { screenshot-tool "arrow"; }
    D { screenshot-tool "rectangle"; }
    X repeat=false { screenshot-toggle-axes; }
    Shift_L repeat=false { screenshot-toggle-axes hold=true; }
    Minus { screenshot-context "-1"; }
    Equal { screenshot-context "+1"; }
    WheelScrollUp { screenshot-context "+1"; }
    WheelScrollDown { screenshot-context "-1"; }
}
```

### Context-sensitive adjustment

`screenshot-context` first uses the control focused by `screenshot-focus-next` or `screenshot-focus-previous`. Without keyboard focus it uses pointer hit-testing. A focused or hovered Draw, Arrow or Rectangle button changes its own stroke width; Blur changes its strength. Neither focus nor this command selects the tool. Select, palette controls and other UI absorb the command without zooming; with no focus, toolbar padding also blocks it and the canvas changes magnifier zoom. With neither focus nor a pointer, it is a no-op. Mouse movement clears keyboard focus and restores pointer-based routing. Each tool remembers its value independently. A bare integer sets an absolute value from `1` to `128`, clamped to the target: widths `1`–`128`, Blur `1`–`3`, magnifier rounded down to a power of two within `max-zoom`. Signed strings such as `"+1"` or `"-1"` adjust without wrapping. Tool parameters remain frozen during a gesture, while canvas zoom remains available. Explicit `screenshot-magnifier-zoom` ignores both focus and hover.

### Magnifier

| Setting                 | Default       | Meaning                                                               |
| ----------------------- | ------------- | --------------------------------------------------------------------- |
| `diameter`              | `192`         | Positive lens diameter in logical pixels.                             |
| `border-width`          | `2`           | Inward border width in logical pixels; non-negative, `0` disables it. |
| `border-color`          | `"#ffffff"`   | Border color, with alpha support.                                     |
| `grid-color`            | `"#0000008c"` | Grid line color, with alpha support.                                  |
| `pointer-gap`           | `16`          | Non-negative gap between pointer and lens, in logical pixels.         |
| `initial-zoom`          | `1`           | Starting zoom: `1`, `2`, `4`, `8`, `16`, or `32`.                     |
| `show-from`             | `2`           | Visibility threshold: `2`, `4`, `8`, `16`, or `32`.                   |
| `max-zoom`              | `32`          | Maximum zoom: `2`, `4`, `8`, `16`, or `32`.                           |
| `grid`                  | `true`        | Enable the pixel grid.                                                |
| `grid-from`             | `4`           | Grid threshold: `2`, `4`, `8`, `16`, or `32`.                         |
| `cross-highlight`       | `true`        | Highlight the central pixel's row and column.                         |
| `cross-highlight-color` | `"#a6d9f259"` | Color of that highlight.                                              |

`initial-zoom` and `show-from` must not exceed `max-zoom`. `grid-from` may exceed it; in that case the grid remains hidden until the maximum is raised. The grid also requires the lens to be visible.

The border is drawn inside the lens without changing its outer diameter. Its default is a single white outline, replacing the former black/white contour. A transparent border reveals the image beneath it. `grid-color` blends over the image; its default approximately preserves the previous grid darkening. It does not change `cross-highlight-color` or the central pixel marker. These colors and the border width update on configuration reload.

At the default `initial-zoom 1`, the lens starts hidden. To show it immediately, use `initial-zoom 2` with `show-from 2`. Zoom changes in discrete steps, not continuously. The lens shows the frozen image plus edits, without the toolbar or selection overlay.

### Customization example

A compact bottom-right toolbar, blue brush, persistent palette and immediately visible magnifier:

```kdl
screenshot {
    toolbar {
        position "bottom"
        align "right"
        margin bottom=20 right=20
        button-size 36
        icon-size 20
        background-color "#20242eee"
        accent-color "#89b4fa"
        opacity 0.95
        drag-opacity 0.3
    }
    selection {
        border-color "#89b4fa"
        dim-opacity 0.35
    }
    brush {
        width 6
        color "#0080ff"
        palette "#0080ff" "#ff8000" "#ffffff" "#000000"
        close-palette-on-select false
    }
    magnifier {
        diameter 160
        initial-zoom 2
        show-from 2
        max-zoom 16
        grid-from 8
    }
}
```

### Opening, bindings and export

The initial tool belongs to the opening action, not the appearance block. Bindings, file paths, formats and export quality are also configured **outside** `screenshot {}`:

```kdl
screenshot-path "~/Pictures/Screenshots/%Y-%m-%d_%H-%M-%S.{ext}"
screenshot-format "png"

binds {
    Print { screenshot show-pointer=false; }
    Mod+Print { screenshot tool="draw"; }
    Ctrl+Alt+W { confirm-screenshot format="webp" quality=85; }
    Ctrl+Alt+A { confirm-screenshot format="avif" quality=90; }
}
```

Opening tools are `"select"` (default), `"draw"`, `"arrow"`, `"rectangle"` and `"blur"`. See [Key Bindings](./Configuration:-Key-Bindings.md#screenshot-screenshot-screen-screenshot-window) for editor actions.

`quality` applies only to that export: WebP accepts `0`–`100` (lossy, including at `100`), AVIF accepts `1`–`100`. Without it, WebP is lossless and AVIF uses quality `90`. PNG and clipboard-only exports ignore quality; clipboard images always remain PNG. There are no quality settings inside the appearance block. See [screenshot-format](./Configuration:-Miscellaneous.md#screenshot-format) and [screenshot-path](./Configuration:-Miscellaneous.md#screenshot-path).

`screenshot-confirm` activates the keyboard-focused control; without focus it saves and copies. `confirm-screenshot` exports regardless of focus. Neither exports an unfinished gesture. `screenshot-cancel` closes the palette, cancels the current gesture, then closes the editor on subsequent calls; `cancel-screenshot` immediately discards the session.

### Full-feature demo configuration

The repository example `resources/knit-desktop.kdl` exposes all editor actions and independent tool settings. It is a demo, not the default configuration above: it starts Zed, Obsidian and two Ghostty windows. Run it from an existing Wayland session with `./target/debug/niri --config resources/knit-desktop.kdl`.

| Demo binding | Action |
| --- | --- |
| `Alt+U` / `Print`; `Alt+Shift+U` | Open Select; open Draw without the captured pointer |
| `V`, `B`, `A`, `D`, `R` | Select, Draw, Arrow, Rectangle, Blur |
| `C`; `1`–`7` | Toggle palette; select a swatch |
| `Tab` / `Shift+Tab`; `-` / `=` or wheel | Move focus; adjust the focused control, otherwise pointer context |
| `Ctrl+wheel`; `F11` / `F12` | Explicit lens steps; absolute 1× / 32× |
| `X`; hold left `Shift` or right mouse | Toggle axes; temporarily enable axes |
| Arrows; `Ctrl+arrows`; `Shift+arrows`; hold `Space` and drag | Move crop 1 px; move 20 px; resize 1 px; move with pointer |
| `Ctrl+Z`; `Ctrl+Shift+Z` / `Ctrl+Y` | Undo; redo |
| `P`; `F3` / `F4` | Toggle captured pointer; explicitly show / hide |
| `F1` / `F2`; `F5` / `F6` | Open / close palette; enable / disable axes |
| `F7`, `F8`, `F9` | Set brush orange, arrow green, rectangle blue without selecting the tool |
| `F10` | Set context to 8: width 8, Blur clamped to 3, or canvas zoom 8× |
| `Enter`; `Escape`; `Ctrl+Escape` | Confirm focus or save; cancel one step; discard immediately |
| `Ctrl+C` / `Ctrl+S` | Copy only / save and copy |
| `Ctrl+Alt+P` / `Ctrl+Alt+W` / `Ctrl+Alt+A` | Export PNG / WebP quality 85 / AVIF quality 90 |
| `Ctrl+Print` / `Alt+Print` | Capture screen / window without the editor |

The demo starts with a red 4 px brush, green 8 px arrow and blue 6 px rectangle. The arrow palette stays open after color selection; repeated activation of Rectangle does not open its palette (use `C` or `F1`). These differences are intentional checks of independent settings. Existing annotations retain their original color and width.

### Reloading and validation

Configuration reload updates the open editor's theme, geometry and palette. During a gesture, application is deferred until release or cancel, so controls do not move between press and release.

- Reload preserves the current tool, each tool's color and width, crop and undo/redo history.
- `brush`, `arrow` and `rectangle` colors/widths and `magnifier.initial-zoom` are defaults for the next opening; `remember-selection` also affects the next opening.
- A selected color remains selected even if removed from the palette; it simply has no highlighted swatch.
- A lower `max-zoom` clamps the current zoom. Magnifier thresholds and appearance update with the applied configuration.
- Changing configured `blur.strength` updates the current strength for new blur gestures only. Unrelated reloads preserve the strength chosen in the editor.

Sizes must be finite and positive; margins and border widths must be finite and non-negative. Opacities must be within `0`–`1`. Contradictory values specified together, such as `icon-size > button-size`, `drag-opacity > opacity`, or `show-from > max-zoom`, are rejected.

When partial settings are merged, including through [include files](./Configuration:-Include.md), unspecified values are retained. To keep merged settings valid, the effective button size, resting opacity and maximum zoom are raised when needed to accommodate the icon size, drag opacity and zoom thresholds. Set both sides of a constraint explicitly when lowering them.

Validate your configuration before trying it:

```sh
niri validate --config ~/.config/niri/config.kdl
```
