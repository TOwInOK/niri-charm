"""Compose visible screenshot-editor interactions from ordinary pointer gestures.

This adapter targets CaptureProfile, not arbitrary user configurations. Toolbar
selection and palette changes are actual clicks; IPC only opens, saves or closes
the editor. A recorded scenario therefore shows how to use the feature.
"""

from __future__ import annotations

from pathlib import Path

from niri_video.geometry import Easing, MotionPath, Point
from niri_video.profile import Color, Tool, rgb
from niri_video.session import PreviewSession


class ScreenshotEditor:
    """Operate the real toolbar using named controls in the controlled profile."""

    def __init__(self, session: PreviewSession, export: Path | None = None) -> None:
        self.session = session
        self.export = (export or session.output / "screenshot.png").resolve()
        self._tool: Tool = "select"
        self._open = False

    def open(self) -> ScreenshotEditor:
        """Open the editor and wait for its visible toolbar to render."""
        if self._open:
            raise RuntimeError("This editor is already open")
        if self.export.exists():
            raise FileExistsError(self.export)
        self.session.action(
            "screenshot", "--show-pointer", "false", "--path", str(self.export), "--tool", "select"
        )
        self.session.wait_pixel(self.session.profile.ready_pixel, (24, 30, 46))
        self._open = True
        return self

    def _require_open(self) -> None:
        if not self._open:
            raise RuntimeError("Open the screenshot editor before interacting with it")

    def select(self, start: Point, end: Point, *, duration: float = 0.8) -> None:
        """Select a crop by visibly dragging between two canvas points."""
        self.choose_tool("select")
        self.session.pointer.drag(MotionPath.line(start, end), duration=duration)

    def choose_tool(self, tool: Tool) -> None:
        """Move to and click a toolbar tool, without reopening its palette."""
        self._require_open()
        if tool != self._tool:
            self.session.pointer.click(self.session.profile.tool_point(tool))
            self._tool = tool

    def choose_color(self, color: Color) -> None:
        """Show the palette, pause for readability, then click the requested swatch."""
        self._require_open()
        if self._tool not in ("draw", "arrow", "rectangle"):
            raise ValueError("Select a colored drawing tool before choosing a color")
        profile = self.session.profile
        self.session.pointer.click(profile.tool_point(self._tool))
        swatch = profile.color_point(self._tool, color)
        # Sample clear fill, not the active-color checkmark in the swatch center.
        self.session.wait_pixel(Point(swatch.x + 6, swatch.y + 6), rgb(color))
        self.session.pause(0.45)
        self.session.pointer.click(swatch)

    def draw(
        self, path: MotionPath, *, duration: float = 2.0, easing: Easing = Easing.LINEAR
    ) -> None:
        """Draw a timed stroke with explicit easing and a smooth approach to its start."""
        self._require_open()
        if self._tool == "select":
            raise ValueError("Choose a drawing tool before drawing a path")
        self.session.pointer.drag(path, duration=duration, easing=easing)

    def save(self) -> Path:
        """Commit the native screenshot export, which deliberately excludes the UI."""
        self._require_open()
        self.session.action("confirm-screenshot", "--write-to-disk", "true")
        self.session.wait_until(self.export.is_file, "native screenshot export")
        self._open = False
        return self.export

    def cancel(self) -> None:
        """Close the editor without saving the demo image."""
        self._require_open()
        self.session.action("screenshot-cancel")
        self._open = False
