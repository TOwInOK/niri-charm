"""Show Overview ending a locked zoom session rather than temporarily hiding it."""

from pathlib import Path

from niri_video import Point, PreviewSession, ScenarioOptions


def main() -> None:
    """Start at 1x, zoom to 2x, enter Overview, and return to the initial 1x view."""
    options = ScenarioOptions.parse(
        "zoom-overview",
        "Demonstrate zoom-to-Overview camera handoff and unzoomed return",
        output=Path(__file__).resolve().parents[1] / "output",
    )
    with PreviewSession(output=options.output, size=options.size, niri=options.niri) as demo:
        demo.terminal(
            "\x1b[?25l\n\x1b[1;36mZOOM > OVERVIEW\x1b[0m\n\n"
            "1x > 2x > Overview > 1x\n"
            "Zoom, then open Overview\n"
            "Close: back to 1x"
        )
        demo.wait_pixel(Point(demo.size.width * 0.5, demo.size.height * 0.7), (16, 21, 33))
        demo.pointer.move_to(Point(demo.size.width * 0.02, demo.size.height * 0.03))
        with demo.record(fps=options.fps) as recording:
            demo.frame("native.png")
            demo.pause(1.5)
            demo.action("zoom", "2.0", "--lock")
            demo.wait_zoom(2.0)
            demo.frame("zoomed.png")
            demo.pause(1.5)
            demo.action("open-overview")
            demo.wait_zoom(1.0)
            states = demo.query("zoom")
            assert isinstance(states, list) and isinstance(states[0], dict)
            assert states[0]["locked"] is False
            demo.pause(1.5)
            demo.frame("overview.png")
            demo.action("close-overview")
            demo.wait_pixel(Point(demo.size.width * 0.5, demo.size.height * 0.7), (16, 21, 33))
            demo.wait_zoom(1.0)
            demo.pause(1.5)
            demo.frame("unzoomed.png")
        video = recording.stop()
    options.export(video)


if __name__ == "__main__":
    main()
