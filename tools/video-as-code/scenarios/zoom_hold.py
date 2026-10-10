"""Exercise a real hold binding and restore an already magnified view."""

from pathlib import Path
from tempfile import TemporaryDirectory

from niri_video import Point, PreviewSession, ScenarioOptions


def main() -> None:
    """Loop through 1x, 2x, held 4x, restored 2x, and the initial 1x view."""
    options = ScenarioOptions.parse(
        "zoom-hold",
        "Demonstrate 1x to 2x to held 4x to restored 2x to 1x",
        output=Path(__file__).resolve().parents[1] / "output",
    )
    with (
        TemporaryDirectory(prefix="niri-zoom-hold-") as directory,
        PreviewSession(output=options.output, size=options.size, niri=options.niri) as demo,
    ):
        base = Path(directory) / "base.kdl"
        _ = base.write_text(demo.profile.config(), encoding="utf-8")
        config = Path(directory) / "config.kdl"
        _ = config.write_text(
            'include "base.kdl"\nbinds { F8 repeat=false { zoom 4.0 hold=true; }; }\n',
            encoding="utf-8",
        )
        demo.action("load-config-file", "--path", str(config))
        demo.terminal("\x1b[?25l\x1b[1;36mHOLD TO ZOOM\x1b[0m\nHold F8: 4x\nRelease: 2x")
        demo.wait_pixel(Point(demo.size.width * 0.5, demo.size.height * 0.7), (16, 21, 33))
        start = Point(demo.size.width * 0.02, demo.size.height * 0.03)
        demo.pointer.move_to(start)
        demo.wait_zoom(1.0)
        # Clear Super inherited on host focus entry, only on the private X server.
        demo.keyboard.press("Super_L")
        with demo.record(fps=options.fps) as recording:
            demo.frame("native.png")
            demo.pause(1.5)
            demo.action("set-zoom-level", "2.0")
            demo.wait_zoom(2.0)
            demo.pause(2.0)
            demo.frame("before-hold.png")
            with demo.keyboard.held("F8"):
                demo.wait_zoom(4.0)
                states = demo.query("zoom")
                assert isinstance(states, list) and isinstance(states[0], dict)
                assert states[0]["locked"] is False
                demo.frame("held.png")
                demo.pause(2.5)
            demo.wait_zoom(2.0)
            states = demo.query("zoom")
            assert isinstance(states, list) and isinstance(states[0], dict)
            assert states[0]["locked"] is False
            demo.frame("restored.png")
            demo.pause(2.0)
            demo.action("reset-zoom")
            demo.wait_zoom(1.0)
            demo.pointer.move_to(start)
            demo.pause(1.5)
            demo.frame("unzoomed.png")
        video = recording.stop()
    options.export(video)


if __name__ == "__main__":
    main()
