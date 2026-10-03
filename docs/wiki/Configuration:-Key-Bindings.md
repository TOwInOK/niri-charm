### Overview

Key bindings are declared in the `binds {}` section of the config.

> [!NOTE]
> This is one of the few sections that *does not* get automatically filled with defaults if you omit it, so make sure to copy it from the default config.

Each bind is a hotkey followed by one action enclosed in curly brackets.
For example:

```kdl
binds {
    Mod+Left { focus-column-left; }
    Super+Alt+L { spawn "swaylock"; }
}
```

The hotkey consists of modifiers separated by `+` signs, followed by an XKB key name in the end.

Valid modifiers are:

- `Ctrl` or `Control`;
- `Shift`;
- `Alt`;
- `Super` or `Win`;
- `ISO_Level3_Shift` or `Mod5`—this is the AltGr key on certain layouts;
- `ISO_Level5_Shift`: can be used with an xkb lv5 option like `lv5:caps_switch`;
- `Mod`.

`Mod` is a special modifier that is equal to `Super` when running niri on a TTY, and to `Alt` when running niri as a nested winit window.
This way, you can test niri in a window without causing too many conflicts with the host compositor's key bindings.
For this reason, most of the default keys use the `Mod` modifier.

<sup>Since: 25.05</sup> You can customize the `Mod` key [in the `input` section of the config](./Configuration:-Input.md#mod-key-mod-key-nested).

> [!TIP]
> To find an XKB name for a particular key, you may use a program like [`wev`](https://git.sr.ht/~sircmpwn/wev).
>
> Open it from a terminal and press the key that you want to detect.
> In the terminal, you will see output like this:
>
> ```
> [14:     wl_keyboard] key: serial: 757775; time: 44940343; key: 113; state: 1 (pressed)
>                       sym: Left         (65361), utf8: ''
> [14:     wl_keyboard] key: serial: 757776; time: 44940432; key: 113; state: 0 (released)
>                       sym: Left         (65361), utf8: ''
> [14:     wl_keyboard] key: serial: 757777; time: 44940753; key: 114; state: 1 (pressed)
>                       sym: Right        (65363), utf8: ''
> [14:     wl_keyboard] key: serial: 757778; time: 44940846; key: 114; state: 0 (released)
>                       sym: Right        (65363), utf8: ''
> ```
>
> Here, look at `sym: Left` and `sym: Right`: these are the key names.
> I was pressing the left and the right arrow in this example.
>
> Keep in mind that binding shifted keys requires spelling out Shift and the unshifted version of the key, according to your XKB layout.
> For example, on the US QWERTY layout, <kbd>&lt;</kbd> is on <kbd>Shift</kbd> + <kbd>,</kbd>, so to bind it, you spell out something like `Mod+Shift+Comma`.
>
> As another example, if you've configured the French [BÉPO](https://en.wikipedia.org/wiki/B%C3%89PO) XKB layout, your <kbd>&lt;</kbd> is on <kbd>AltGr</kbd> + <kbd>«</kbd>.
> <kbd>AltGr</kbd> is `ISO_Level3_Shift`, or equivalently `Mod5`, so to bind it, you spell out something like `Mod+Mod5+guillemotleft`.
>
> When resolving latin keys, niri will search for the *first* configured XKB layout that has the latin key.
> So for example with US QWERTY and RU layouts configured, US QWERTY will be used for latin binds.

<sup>Since: 0.1.8</sup> Binds will repeat by default (i.e. holding down a bind will make it trigger repeatedly).
You can disable that for specific binds with `repeat=false`:

```kdl
binds {
    Mod+T repeat=false { spawn "alacritty"; }
}
```

Binds can also have a cooldown, which will rate-limit the bind and prevent it from repeatedly triggering too quickly.

```kdl
binds {
    Mod+T cooldown-ms=500 { spawn "alacritty"; }
}
```

This is mostly useful for the scroll bindings.

### Scroll Bindings

You can bind mouse wheel scroll ticks using the following syntax.
These binds will change direction based on the `natural-scroll` setting.

```kdl
binds {
    Mod+WheelScrollDown cooldown-ms=150 { focus-workspace-down; }
    Mod+WheelScrollUp   cooldown-ms=150 { focus-workspace-up; }
    Mod+WheelScrollRight                { focus-column-right; }
    Mod+WheelScrollLeft                 { focus-column-left; }
}
```

Similarly, you can bind touchpad scroll "ticks".
Touchpad scrolling is continuous, so for these binds it is split into discrete intervals based on distance travelled.

These binds are also affected by touchpad's `natural-scroll`, so these example binds are "inverted", since niri has `natural-scroll` enabled for touchpads by default.

```kdl
binds {
    Mod+TouchpadScrollDown { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.02+"; }
    Mod+TouchpadScrollUp   { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.02-"; }
}
```

Both mouse wheel and touchpad scroll binds will prevent applications from receiving any scroll events when their modifiers are held down.
For example, if you have a `Mod+WheelScrollDown` bind, then while holding `Mod`, all mouse wheel scrolling will be consumed by niri.

### Mouse Click Bindings

<sup>Since: 25.01</sup>

You can bind mouse clicks using the following syntax.

```kdl
binds {
    Mod+MouseLeft    { close-window; }
    Mod+MouseRight   { close-window; }
    Mod+MouseMiddle  { close-window; }
    Mod+MouseForward { close-window; }
    Mod+MouseBack    { close-window; }
}
```

Mouse clicks operate on the window that was focused at the time of the click, not the window you're clicking.

Note that binding `Mod+MouseLeft` or `Mod+MouseRight` will override the corresponding gesture (moving or resizing the window).

### Custom Hotkey Overlay Titles

<sup>Since: 25.02</sup>

The hotkey overlay (the Important Hotkeys dialog) shows a hardcoded list of binds.
You can customize this list using the `hotkey-overlay-title` property.

To add a bind to the hotkey overlay, set the property to the title that you want to show:
```kdl
binds {
    Mod+Shift+S hotkey-overlay-title="Toggle Dark/Light Style" { spawn "some-script.sh"; }
}
```

Binds with custom titles are listed after the hardcoded binds and before non-customized Spawn binds.

To remove a hardcoded bind from the hotkey overlay, set the property to null:
```kdl
binds {
    Mod+Q hotkey-overlay-title=null { close-window; }
}
```

> [!TIP]
> When multiple key combinations are bound to the same action:
> - If any of the binds has a custom hotkey overlay title, niri will show that bind.
> - Otherwise, if any of the binds has a null title, niri will hide the bind.
> - Otherwise, niri will show the first key combination.

Custom titles support [Pango markup](https://docs.gtk.org/Pango/pango_markup.html):

```kdl
binds {
    Mod+Shift+S hotkey-overlay-title="<b>Toggle</b> <span foreground='red'>Dark</span>/Light Style" { spawn "some-script.sh"; }
}
```

![Custom markup example.](https://github.com/user-attachments/assets/2a2ba914-bfa7-4dfa-bb5e-49839034765d)

### Actions

Almost every action that you can bind is also available for programmatic invocation via `niri msg action`.
The exception is action options whose semantics depend on the physical bind lifecycle, such as [`zoom hold=true`](#zoom).
Run `niri msg action` to get a full list of actions along with their short descriptions.

Here are a few actions that benefit from more explanation.

#### `spawn`

Run a program.

`spawn` accepts a path to the program binary as the first argument, followed by arguments to the program.
For example:

```kdl
binds {
    // Run alacritty.
    Mod+T { spawn "alacritty"; }

    // Run `wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.1+`.
    XF86AudioRaiseVolume { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.1+"; }
}
```

> [!TIP]
>
> <sup>Since: 0.1.5</sup>
>
> Spawn bindings have a special `allow-when-locked=true` property that makes them work even while the session is locked:
>
> ```kdl
> binds {
>     // This mute bind will work even when the session is locked.
>     XF86AudioMute allow-when-locked=true { spawn "wpctl" "set-mute" "@DEFAULT_AUDIO_SINK@" "toggle"; }
> }
> ```

For `spawn`, niri *does not* use a shell to run commands, which means that you need to manually separate arguments.
See [`spawn-sh`](#spawn-sh) below for an action that uses a shell.

```kdl
binds {
    // Correct: every argument is in its own quotes.
    Mod+T { spawn "alacritty" "-e" "/usr/bin/fish"; }

    // Wrong: will interpret the whole `alacritty -e /usr/bin/fish` string as the binary path.
    Mod+D { spawn "alacritty -e /usr/bin/fish"; }

    // Wrong: will pass `-e /usr/bin/fish` as one argument, which alacritty won't understand.
    Mod+Q { spawn "alacritty" "-e /usr/bin/fish"; }
}
```

This also means that you cannot expand environment variables or `~`.
If you need this, you can run the command through a shell manually.

```kdl
binds {
    // Wrong: no shell expansion here. These strings will be passed literally to the program.
    Mod+T { spawn "grim" "-o" "$MAIN_OUTPUT" "~/screenshot.png"; }

    // Correct: run this through a shell manually so that it can expand the arguments.
    // Note that the entire command is passed as a SINGLE argument,
    // because shell will do its own argument splitting by whitespace.
    Mod+D { spawn "sh" "-c" "grim -o $MAIN_OUTPUT ~/screenshot.png"; }

    // You can also use a shell to run multiple commands,
    // use pipes, process substitution, and so on.
    Mod+Q { spawn "sh" "-c" "notify-send clipboard \"$(wl-paste)\""; }
}
```

As a special case, niri will expand `~` to the home directory *only* at the beginning of the program name.

```kdl
binds {
    // This will work: one ~ at the very beginning.
    Mod+T { spawn "~/scripts/do-something.sh"; }
}
```

#### `spawn-sh`

<sup>Since: 25.08</sup>

Run a command through the shell.

The argument is a single string that is passed verbatim to `sh`.
You can use shell variables, pipelines, `~` expansion, and everything else as expected.

```kdl
binds {
    // Works with spawn-sh: all arguments in the same string.
    Mod+D { spawn-sh "alacritty -e /usr/bin/fish"; }

    // Works with spawn-sh: shell variable ($MAIN_OUTPUT), ~ expansion.
    Mod+T { spawn-sh "grim -o $MAIN_OUTPUT ~/screenshot.png"; }

    // Works with spawn-sh: process substitution.
    Mod+Q { spawn-sh "notify-send clipboard \"$(wl-paste)\""; }

    // Works with spawn-sh: multiple commands.
    Super+Alt+S { spawn-sh "pkill orca || exec orca"; }
}
```

`spawn-sh "some command"` is equivalent to `spawn "sh" "-c" "some command"`—it's just a less confusing shorthand.
Keep in mind that going through the shell incurs a tiny performance penalty compared to directly `spawn`ing some binary.

Using `sh` is hardcoded, consistent with other compositors.
If you want a different shell, write it out using `spawn`, e.g. `spawn "fish" "-c" "some fish command"`.

#### `quit`

Exit niri after showing a confirmation dialog to avoid accidentally triggering it.

```kdl
binds {
    Mod+Shift+E { quit; }
}
```

If you want to skip the confirmation dialog, set the flag like so:

```kdl
binds {
    Mod+Shift+E { quit skip-confirmation=true; }
}
```

#### `do-screen-transition`

<sup>Since: 0.1.6</sup>

Freeze the screen for a brief moment then crossfade to the new contents.

```kdl
binds {
    Mod+Return { do-screen-transition; }
}
```

This action is mainly useful to trigger from scripts changing the system theme or style (between light and dark for example).
It makes transitions like this, where windows change their style one by one, look smooth and synchronized.

For example, using the GNOME color scheme setting:

```shell
niri msg action do-screen-transition
dconf write /org/gnome/desktop/interface/color-scheme "\"prefer-dark\""
```

By default, the screen is frozen for 250 ms to give windows time to redraw, before the crossfade.
You can set this delay like this:

```kdl
binds {
    Mod+Return { do-screen-transition delay-ms=100; }
}
```

Or, in scripts:

```shell
niri msg action do-screen-transition --delay-ms 100
```

#### `toggle-window-rule-opacity`

<sup>Since: 25.02</sup>

Toggle the opacity window rule of the focused window.
This only has an effect if the window's opacity window rule is already set to semitransparent.

```kdl
binds {
    Mod+O { toggle-window-rule-opacity; }
}
```

#### `screenshot`, `screenshot-screen`, `screenshot-window`

Actions for taking screenshots.

- `screenshot`: opens the built-in interactive screenshot UI.
- `screenshot-screen`, `screenshot-window`: takes a screenshot of the focused screen or window respectively.

The screenshot is both stored to the clipboard and saved to disk, according to the [`screenshot-path` option](./Configuration:-Miscellaneous.md#screenshot-path).

<sup>Since: 25.02</sup> You can disable saving to disk for a specific bind with the `write-to-disk=false` property:

```kdl
binds {
    Ctrl+Print { screenshot-screen write-to-disk=false; }
    Alt+Print { screenshot-window write-to-disk=false; }
}
```

The root `screenshot-format` setting chooses the default file format. Override it
for one editor export with `confirm-screenshot format=...`; this does not change
the config or subsequent saves. Clipboard contents always remain PNG.

```kdl
screenshot-format "png"
screenshot-path "~/Pictures/Screenshots/%Y-%m-%d_%H-%M-%S.{ext}"
binds {
    Ctrl+S { confirm-screenshot; }
    Ctrl+Alt+P { confirm-screenshot format="png"; }
    Ctrl+Alt+W { confirm-screenshot format="webp"; }
    Ctrl+Alt+A { confirm-screenshot format="avif"; }
}
```

PNG is lossless; WebP defaults to lossless and AVIF to lossy quality 90 with 4:4:4 chroma.
Pass `quality=85` to `confirm-screenshot` for one export (WebP 0–100, AVIF 1–100).
Omitting quality preserves lossless WebP / AVIF quality 90; PNG ignores it.
The same override is available as `niri msg action confirm-screenshot --format avif --quality 85`.

In the interactive screenshot UI, pressing <kbd>Ctrl</kbd><kbd>C</kbd> will copy the screenshot to the clipboard without writing it to disk.

The screenshot editor freezes each output when opened. Its rectangular SVG toolbar, selection appearance, brush, blur strength and magnifier are configured in the root `screenshot` block. Position, alignment, independent edge margins, colors, sizes and resting/drag opacity are configurable. See [Screenshot configuration](./Configuration:-Screenshot.md) for appearance settings and examples.

All keyboard and wheel actions use the existing root `binds` block; there are no hidden fallback shortcuts. **Existing user configs must add the editor bindings** from the example or `resources/default-config.kdl`. The shortcuts below describe those supplied bindings, not hardcoded keys. Local editor bindings are ignored while closed and take priority over ordinary bindings on the same key while open. When merging blocks or include files, a binding only replaces another binding on the same key in the same category; ordinary and editor-local bindings coexist.

- **Select · V:** drag to select the crop. Existing selection movement and resize binds still work; holding Space or the compositor modifier while dragging moves the crop in any tool. Space no longer saves the screenshot.
- **Draw, B:** drag a freehand stroke or click for a dot. Width and color are captured per stroke. Each colored tool shows its own bottom color stripe and width number. Repeated activation of its selected button opens its palette (`open-palette-on-click`); selecting a different tool does not.
- **Arrow, A / Rectangle, D:** drag tail-to-head or between opposite corners. Rectangles are unfilled; arrow heads scale with width. Colors, widths and palettes are independent of Draw. Each completed gesture is one undo/redo operation.
- **Blur, R:** drag a rectangle to blur the frozen background beneath drawings. Three bottom segments show low/medium/high; the shared `Minus` / `Equal` bindings adjust the parameter under the pointer — strength over the Blur button — without wrapping, for new regions only. Clicking Blur only selects it. Blur is not a guarantee of secure redaction.
- **Axes, X / hold Shift_L:** shared constraint mode for Draw, Arrow and Rectangle. Both dashed guides use the same color/opacity. During drag, release and press hold again to move the anchor to the current drawn point without completing the figure. For Rectangle this lets you set width, reanchor and set height in one drag. Release returns the toggle state. The separate toolbar toggle is before Copy/Save.
- **Palette, C:** opens for the active Draw, Arrow or Rectangle tool. Configure 1–7 opaque colors in its `brush.palette`, `arrow.palette` or `rectangle.palette`; click or use the supplied digit bindings. Clicking outside dismisses it without painting.
- **Undo / Redo:** Ctrl+Z undoes; Ctrl+Shift+Z or Ctrl+Y redoes the shared history across outputs. Undo during drawing/blur cancels the unfinished gesture. Unavailable buttons are dimmed.
- **Cursor · P:** toggle the captured pointer. The same solid, neutral cursor icon is used in both states; an accent stripe beneath it indicates that the pointer is included.
- **Copy · Ctrl+C:** copy the crop without writing a file.
- **Save · Enter / Ctrl+S:** save and copy the crop.

Tab / Shift+Tab move focus among enabled controls; Enter activates the focused control, or saves when there is no focus. Escape dismisses the palette, cancels a gesture (restoring crop), then closes the editor. Explicit `confirm-screenshot write-to-disk=false/true` always means copy/save, regardless of focus. Export during an unfinished gesture is ignored.

The magnifier previews the frozen image with its edits. Toolbar, magnifier, selection, axis guides and brush-size cursor never enter the exported image. Edits stay attached to output pixels when the crop moves. Selection is limited to one output at a time.
Only the frozen background outside the crop is dimmed. Draw strokes and blur regions keep their full brightness inside and outside the crop; selection borders and controls remain above the edits.

Keyboard focus from `screenshot-focus-next` / `screenshot-focus-previous` takes priority for `screenshot-context`: a focused tool changes its own parameter without becoming selected; focused Select or other UI blocks the command. Mouse movement clears focus. The pointer-routing rules below apply when no control has keyboard focus; explicit `screenshot-magnifier-zoom` ignores both focus and hover.

The circular magnifier uses nearest-neighbor sampling. `initial-zoom` sets the opening scale, `show-from` its visibility threshold, `max-zoom` the limit, and `grid-from` the independent pixel-grid threshold. The supplied wheel bindings bind `screenshot-context`: over the canvas they step power-of-two zoom (`+1` / `-1`), over a tool button they adjust that tool's parameter, elsewhere they do nothing. Explicit `screenshot-magnifier-zoom` zooms regardless of the pointer position. Scrolling never changes desktop zoom while the editor is open. The center pixel retains its exact color. Cross-highlight and its color are configurable; the lens stays out of exports.

Reload updates theme, geometry, palette and magnifier limits without resetting tool, color, width, history or crop; geometry changes wait until a gesture ends. Brush defaults and initial zoom apply on the next opening. Commands are also available through `niri msg action`: for example `screenshot --tool draw`, `screenshot-brush-color "#0080ff"`, `screenshot-context +1`, and `screenshot-magnifier-zoom 4`. `screenshot-context` changes the value under the pointer: stroke width for Draw, Arrow and Rectangle (`1`–`128`), strength for Blur (`1`–`3`), magnifier zoom over the canvas; a bare integer is absolute — each target clamps it to its range — a signed string adjusts, and over Select or other UI it is a no-op that does not zoom. `screenshot-magnifier-zoom` acts directly, ignoring the pointer.
The center pixel's row and column are highlighted across the circular lens. Each band is one source pixel wide and follows the magnification; the center pixel itself retains its original color inside its outline. This overlay is excluded from export.

History lasts until the editor closes. Changing the tool, color, or crop does not add a history entry. A new completed edit clears redo history; undo during a drawing or blur gesture cancels that unfinished edit. If an edit cannot be rendered, the toolbar displays an error message and saving leaves the editor open rather than silently exporting without the effect.

<sup>Since: 25.05</sup> You can hide the mouse pointer in screenshots with the `show-pointer=false` property:

```kdl
binds {
    // The pointer will be hidden by default
    // (you can still show it by pressing P).
    Print { screenshot show-pointer=false; }

    // The pointer will be hidden on the screenshot.
    Ctrl+Print { screenshot-screen show-pointer=false; }
}
```

<sup>Since: 26.04</sup> You can show the mouse pointer on window screenshots with the `show-pointer=true` property.
The pointer will be included only if the window is currently receiving pointer input (usually this means the pointer is on top of the window).

```kdl
binds {
    // The pointer will be visible on the screenshot
    // if it's on top of the window.
    Alt+Print { screenshot-window show-pointer=true; }
}
```

#### `screenshot-undo`, `screenshot-redo`

Undo or redo a stroke or blur rectangle in the screenshot editor. The default editor shortcuts are <kbd>Ctrl</kbd><kbd>Z</kbd> for undo, and <kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>Z</kbd> or <kbd>Ctrl</kbd><kbd>Y</kbd> for redo.

These actions can also be assigned through the usual binding system:

```kdl
binds {
    Ctrl+Z { screenshot-undo; }
    Ctrl+Shift+Z { screenshot-redo; }
}
```

Screenshot-history bindings are active only while the editor is open; outside it, their keys pass through to applications. `screenshot-screen` and `screenshot-window` remain immediate captures without an editing step.

#### `toggle-keyboard-shortcuts-inhibit`

<sup>Since: 25.02</sup>

Applications such as remote-desktop clients and software KVM switches may request that niri stops processing its keyboard shortcuts so that they may, for example, forward the key presses as-is to a remote machine.
`toggle-keyboard-shortcuts-inhibit` is an escape hatch that toggles the inhibitor.
It's a good idea to bind it, so a buggy application can't hold your session hostage.

```kdl
binds {
    Mod+Escape { toggle-keyboard-shortcuts-inhibit; }
}
```

You can also make certain binds ignore inhibiting with the `allow-inhibiting=false` property.
They will always be handled by niri and never passed to the window.

```kdl
binds {
    // This bind will always work, even when using a virtual machine.
    Super+Alt+L allow-inhibiting=false { spawn "swaylock"; }
}
```

#### `zoom`

<sup>Since: next release</sup>

Actions for desktop zoom, which magnifies the rendered desktop without changing the output scale.
See the [Accessibility](./Accessibility.md#desktop-zoom) page for an overview and the [`zoom` config section](./Configuration:-Miscellaneous.md#zoom_1) for the settings.

```kdl
binds {
    Mod+Z { zoom 2.0; }
    Mod+X { zoom 2.0 hold=true; }
    Mod+0 { reset-zoom; }

    // Mouse wheel zooming works too.
    // Mod+WheelScrollUp { zoom-in; }
    // Mod+WheelScrollDown { zoom-out; }
}
```

Zoom actions apply to the output under the pointer, or to the focused output when the pointer is not on any output.
They are ignored while the [Overview](./Overview.md) is open, since opening it ends the zoom session.

- `zoom-in`: multiplies the target zoom level by [`increment-factor`](./Configuration:-Miscellaneous.md#increment-factor), clamped between 1× and `max-zoom`.
- `zoom-out`: divides the target zoom level by `increment-factor`, ending at exactly 1×.
- `set-zoom-level <LEVEL>`: sets the target zoom level to `LEVEL`, which must be at least `1`. Values above `max-zoom` are clamped to it.
- `reset-zoom`: returns the target zoom level to 1×.
- `zoom-lock`: toggles the zoom lock. While locked, the zoomed viewport no longer follows the pointer. `zoom-in` and `zoom-out` capture the pointer as their fixed anchor on each command; absolute level changes keep the content at the viewport center centered. `zoom <LEVEL> lock=true` activation also anchors on the pointer. `zoom-lock hold=true` inverts the lock only while the trigger is held: pressing locks an unlocked viewport or unlocks a locked one, and releasing restores the previous state.
- `zoom <LEVEL>`: if the current target zoom level is above 1×, returns it to 1×; otherwise zooms to `LEVEL`. `LEVEL` must be greater than `1` and is clamped to `max-zoom`. It is the preset used when entering zoom, not a permanent fixed target. `zoom <LEVEL> lock=true` zooms in around the pointer and locks the viewport immediately, keeping the point under the pointer visually anchored during the zoom-in; toggling back to 1× unlocks it. `zoom <LEVEL> hold=true` makes it a momentary zoom: pressing the trigger remembers the current zoom view and temporarily zooms to `LEVEL`; releasing the same trigger restores the previous view, not simply 1×. `zoom <LEVEL> hold=true lock=true` also locks the viewport during the hold—pointer-anchored like the toggle—and restores the previous lock state on release.

`zoom` with `hold=true` and `zoom-lock hold=true` are bind-only: they are not available through `niri msg action`, since their semantics depend on the physical press and release.
The IPC `zoom` action accepts only `level` and `lock`.
The trigger must have a release event, so scroll binds cannot be used for `zoom hold=true` or `zoom-lock hold=true`.
