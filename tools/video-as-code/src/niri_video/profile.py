"""Controlled niri configuration and named screenshot-editor coordinates.

The profile fixes output scale to one and owns its toolbar settings. Its geometry
mirrors ``bar_layout``/``pop_layout`` in ``src/ui/screenshot_editor.rs`` rather than
pretending the Wayland UI exposes DOM-style selectors. Keep changes to those
layouts and this adapter together; scenarios themselves never hard-code buttons.
"""

from dataclasses import dataclass
from typing import Literal

from niri_video.geometry import Point, Size

Color = Literal["blue", "orange", "green", "pink"]
Tool = Literal["select", "draw", "arrow", "rectangle", "blur"]
COLORS: dict[Color, str] = {
    "blue": "#73c2ff",
    "orange": "#ffba75",
    "green": "#9cdb83",
    "pink": "#eaa0d5",
}
TOOLS: tuple[Tool, ...] = ("select", "draw", "arrow", "rectangle", "blur")


def rgb(color: Color) -> tuple[int, int, int]:
    """Return the exact RGB bytes of a named demo brush color."""
    value = COLORS[color]
    return int(value[1:3], 16), int(value[3:5], 16), int(value[5:7], 16)


@dataclass(frozen=True)
class CaptureProfile:
    """Own the config and geometry used by an isolated capture session.

    Coordinates are physical pixels in the nested niri window. The controlled
    profile intentionally requires a canvas large enough not to shrink controls.
    """

    size: Size
    margin: int = 24
    button_size: int = 48

    def __post_init__(self) -> None:
        if self.size.width < self.bar_width + 2 * self.margin or self.size.height < 360:
            raise ValueError("Capture canvas is too small for the screenshot toolbar")

    @property
    def bar_width(self) -> int:
        """Return the toolbar width including its fixed group gaps and padding."""
        return 11 * self.button_size + 4 * 12 + 2 * 4

    @property
    def bar_left(self) -> float:
        """Return the centered toolbar's left edge."""
        return (self.size.width - self.bar_width) / 2

    @property
    def ready_pixel(self) -> Point:
        """Return an unobscured interior pixel used to await the rendered toolbar."""
        return Point(self.bar_left + 2, self.margin + self.button_size / 2 + 4)

    def tool_point(self, tool: Tool) -> Point:
        """Return a tool's clickable center, without moving the real pointer."""
        index = TOOLS.index(tool)
        return Point(
            self.bar_left + 4 + (index + 0.5) * self.button_size,
            self.margin + 4 + self.button_size / 2,
        )

    def color_point(self, tool: Tool, color: Color) -> Point:
        """Return a swatch center in the active tool's four-color palette."""
        index = tuple(COLORS).index(color)
        palette_width = 4 * 28 + 2 * 6
        left = max(
            self.margin,
            min(
                self.tool_point(tool).x - palette_width / 2,
                self.size.width - self.margin - palette_width,
            ),
        )
        top = self.margin + self.button_size + 2 * 4 + 4
        return Point(left + 6 + (index + 0.5) * 28, top + 6 + 14)

    def config(self) -> str:
        """Render a complete private niri config, never touching the host config."""
        palette = " ".join(f'"{value}"' for value in COLORS.values())
        colored_tools = "\n".join(
            f'''    {tool} {{
        width 7
        color "{COLORS["blue"]}"
        palette {palette}
        open-palette-on-click true
        close-palette-on-select true
    }}'''
            for tool in ("brush", "arrow", "rectangle")
        )
        return f"""// Owned by niri-video; no personal configuration is loaded.
output "winit" {{
    scale 1
}}
hotkey-overlay {{
    skip-at-startup
}}
prefer-no-csd
layout {{
    gaps 24
    default-column-width {{ proportion 0.92; }}
    focus-ring {{
        width 3
        active-color "#73c2ff"
    }}
}}
screenshot {{
    toolbar {{
        position "top"
        align "center"
        margin top={self.margin} right={self.margin} bottom={self.margin} left={self.margin}
        button-size {self.button_size}
        icon-size 28
        background-color "#181e2e"
        opacity 1.0
        drag-opacity 1.0
    }}
    selection {{
        remember-selection false
        dim-opacity 0.35
    }}
{colored_tools}
}}
binds {{
    Print {{ screenshot; }}
    B {{ screenshot-tool "draw"; }}
    V {{ screenshot-tool "select"; }}
    C {{ screenshot-toggle-palette; }}
    Ctrl+Z {{ screenshot-undo; }}
    Ctrl+Shift+Z {{ screenshot-redo; }}
    Escape {{ screenshot-cancel; }}
    Return {{ screenshot-confirm; }}
}}
"""
