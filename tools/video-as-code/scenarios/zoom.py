"""Record real niri desktop zoom, not enlargement of a bitmap in postproduction.

Run ``uv run scenarios/zoom.py``. The same terminal and window border remain in
view as the compositor zooms in and resets, making the scale change observable.
"""

from pathlib import Path

from niri_video import Point, PreviewSession, ScenarioOptions


def main() -> None:
    """Record a focused terminal at native scale, zoomed in, and restored."""
    options = ScenarioOptions.parse(
        "zoom",
        "Demonstrate compositor-level zoom in niri",
        output=Path(__file__).resolve().parents[1] / "output",
    )
    with PreviewSession(output=options.output, size=options.size, niri=options.niri) as demo:
        demo.terminal(
            "\x1b[?25l\n\x1b[1;36mDESKTOP ZOOM\x1b[0m\n\n"
            "Text + borders grow.\n"
            "The app stays unchanged.\n\n"
            "1x  >  2x  >  1x"
        )
        demo.wait_pixel(Point(demo.size.width * 0.5, demo.size.height * 0.7), (16, 21, 33))
        # Keep the anchor identical at the start and end of the loop.
        demo.pointer.move_to(Point(demo.size.width * 0.02, demo.size.height * 0.03))
        with demo.record(fps=options.fps) as recording:
            demo.pause(0.8)
            demo.frame("native.png")
            demo.action("zoom", "2.0", "--lock")
            demo.wait_zoom(2.0)
            demo.frame("zoomed.png")
            demo.pause(2.0)
            demo.action("zoom", "2.0", "--lock")
            demo.wait_zoom(1.0)
            demo.pause(1.5)
        demo.frame("restored.png")
        video = recording.stop()
    options.export(video)


if __name__ == "__main__":
    main()
