"""Record Draw: open a terminal, crop, draw a curve, change color, draw a star.

Run from the uv project with ``uv run scenarios/draw.py``. The lossless source,
visible editor frame, native screenshot export and selected video formats are
kept in a fresh output subdirectory. No personal windows or config are captured.
"""

from pathlib import Path

from niri_video import Easing, MotionPath, Point, PreviewSession, ScenarioOptions, ScreenshotEditor
from niri_video.profile import rgb


def main() -> None:
    """Compose the demonstration from reusable, real interaction primitives."""
    options = ScenarioOptions.parse(
        "draw",
        "Demonstrate freehand drawing and palette changes in niri",
        output=Path(__file__).resolve().parents[1] / "output",
    )
    with PreviewSession(output=options.output, size=options.size, niri=options.niri) as demo:
        width, height = demo.size.width, demo.size.height
        curve = MotionPath.bezier(
            Point(width * 0.16, height * 0.56),
            Point(width * 0.26, height * 0.25),
            Point(width * 0.32, height * 0.82),
            Point(width * 0.46, height * 0.50),
        )
        star = MotionPath.star(Point(width * 0.73, height * 0.56), height * 0.16)
        with demo.record(fps=options.fps) as recording:
            print("Opening demo terminal", flush=True)
            demo.terminal(
                "\n\x1b[1;36mNIRI / SCREENSHOT EDITOR\x1b[0m\n\n"
                "Draw a curve. Pick a color. Make a star.\n\n"
                "Real gestures, scripted in Python."
            )
            demo.wait_pixel(Point(width * 0.5, height * 0.7), (16, 21, 33))
            demo.pause(0.7)
            print("Opening editor and selecting the canvas", flush=True)
            editor = ScreenshotEditor(demo).open()
            editor.select(Point(width * 0.04, height * 0.14), Point(width * 0.90, height * 0.90))
            editor.choose_tool("draw")
            print("Drawing the blue curve", flush=True)
            editor.draw(curve, duration=1.8, easing=Easing.LINEAR)
            demo.wait_pixel(curve.at(0.5), rgb("blue"))
            demo.pause(0.3)
            print("Choosing orange from the visible palette", flush=True)
            editor.choose_color("orange")
            print("Drawing the orange star", flush=True)
            editor.draw(star, duration=2.3, easing=Easing.EASE_IN_OUT)
            demo.wait_pixel(star.at(0.3), rgb("orange"))
            demo.pointer.move_to(
                Point(width * 0.9, height * 0.83), duration=0.5, easing=Easing.EASE_IN_OUT
            )
            demo.pause(1.2)
        demo.frame("editor.png")
        editor.save()
        video = recording.stop()
    options.export(video)


if __name__ == "__main__":
    main()
