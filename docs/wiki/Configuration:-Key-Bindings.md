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

In the interactive screenshot UI, pressing <kbd>Ctrl</kbd><kbd>C</kbd> will copy the screenshot to the clipboard without writing it to disk.

The screenshot editor freezes each output when opened. Its rectangular, bordered SVG toolbar stays centered at the top in one row, shrinking to fit narrow outputs. Hovering anywhere in a button segment lightens that segment’s background; the active tool uses the accent icon color. The panel is fully opaque at rest and fades to 50% opacity during selection, drawing, blur, and crop-move gestures, returning on release or cancellation. Clicking toolbar controls does not fade the panel. Hover and keyboard focus show shortcut tooltips.

- **Select · V:** drag to select the crop. Existing selection movement and resize binds still work; holding Space or the compositor modifier while dragging moves the crop in any tool. Space no longer saves the screenshot.
- **Draw · B:** drag a freehand stroke, or click to draw a dot. Strokes are four captured pixels wide. Clicking the brush button selects Draw and toggles its palette; pressing B selects Draw without opening the palette. A horizontal stripe beneath the brush icon shows the current drawing color, including when another tool is selected. It has the same placement as the cursor button’s status stripe.
- **Blur · R:** drag a rectangle to blur the frozen background. Drawing stays sharp above blur, regardless of operation order. Blur is not a guarantee of secure redaction of sensitive information.
- **Palette · C:** integrated into the brush button, with no separate toolbar button. Press C while Draw is selected, or click the brush, to open it. Choose one of seven colors with a click or keys 1–7. The selected swatch has a contrasting border and check mark. Choosing a color closes the popover; clicking outside dismisses it without starting a stroke. Existing strokes retain their colors. Enter on the focused brush opens the palette and focuses the current swatch; choosing a color or pressing Escape returns focus to the brush.
- **Undo / Redo:** navigate the shared history of strokes and blur rectangles across outputs. Ctrl+Z or Alt+Z undo; Ctrl+Shift+Z, Ctrl+Y, or Alt+Shift+Z redo. Unavailable history buttons are dimmed.
- **Cursor · P:** toggle the captured pointer. The same solid, neutral cursor icon is used in both states; an accent stripe beneath it indicates that the pointer is included.
- **Copy · Ctrl+C:** copy the crop without writing a file.
- **Save · Enter / Ctrl+S:** save and copy the crop.

Tab / Shift+Tab move focus among enabled controls; Enter activates a focused control rather than saving unexpectedly. Escape first dismisses an open palette, then cancels an unfinished gesture (restoring the previous crop for selection/movement), and otherwise closes the editor without saving. These editor shortcuts are hardcoded for now and do not change desktop bindings outside the editor. Configured screenshot undo/redo bindings remain supported.

The magnifier previews the frozen image with its edits. Neither the toolbar, magnifier, nor selection decoration is included in the exported image. Edits stay attached to their output pixels when the crop moves. Selection is limited to one output at a time.
Only the frozen background outside the crop is dimmed. Draw strokes and blur regions keep their full brightness inside and outside the crop; selection borders and controls remain above the edits.

The circular magnifier starts hidden at **1×** each time the editor opens. Scroll up/down to switch between **1×, 2×, 4×, 8×, 16×, and 32×**; scrolling is owned by the editor and does not change desktop zoom. At 4× and above, a one-source-pixel grid helps precise selection. The center pixel is outlined, and the existing crosshair cursor marks the selection hotspot. Magnification uses nearest-neighbor sampling, with no RGB/Hex/coordinate panel. Near output edges the source stays centered on the cursor; the lens moves to the opposite side when space permits.
At **1×** the magnifier is hidden. Scroll up once to show it again at **2×**; the selection crosshair remains available while the lens is hidden.
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
