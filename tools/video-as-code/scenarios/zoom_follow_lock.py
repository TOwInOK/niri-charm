"""Compare live pointer following with a locked desktop zoom viewport."""

from pathlib import Path
from tempfile import TemporaryDirectory

from niri_video import Point, PreviewSession, ScenarioOptions
from niri_video.session import Json


def zoom_state(demo: PreviewSession) -> dict[str, Json]:
    """Read the single isolated output's actual zoom state."""
    states = demo.query("zoom")
    assert isinstance(states, list) and len(states) == 1
    state = states[0]
    assert isinstance(state, dict)
    return state


def main() -> None:
    """Pan right, lock while moving left, then unlock and resume following."""
    options = ScenarioOptions.parse(
        "zoom-follow-lock",
        "Show the deadzone and pointer-follow lock in niri",
        output=Path(__file__).resolve().parents[1] / "output",
    )
    with (
        TemporaryDirectory(prefix="niri-zoom-follow-") as directory,
        PreviewSession(output=options.output, size=options.size, niri=options.niri) as demo,
    ):
        config = Path(directory) / "config.kdl"
        _ = config.write_text(
            demo.profile.config() + "\nzoom { debug { deadzone; }; }\n", encoding="utf-8"
        )
        demo.action("load-config-file", "--path", str(config))
        grid = "\n".join(
            "  ".join(f"{row:02d}-{column}-----" for column in "ABCDEF") for row in range(1, 10)
        )
        demo.terminal("\x1b[?25l\n\x1b[1;36mFOLLOW / LOCK\x1b[0m\n\n" + grid)
        demo.wait_pixel(Point(demo.size.width * 0.5, demo.size.height * 0.8), (16, 21, 33))
        start = Point(demo.size.width * 0.5, demo.size.height * 0.3)
        demo.pointer.move_to(start)
        with demo.record(fps=options.fps) as recording:
            demo.frame("native.png")
            demo.pause(1.0)
            demo.action("set-zoom-level", "2.0")
            demo.wait_zoom(2.0)
            demo.pause(1.0)
            before = zoom_state(demo)["focal"]
            demo.pointer.move_to(
                Point(demo.size.width * 0.88, demo.size.height * 0.3), duration=1.2
            )
            demo.wait_until(lambda: zoom_state(demo)["focal"] != before, "viewport to follow")
            demo.pause(1.2)
            demo.frame("following.png")
            demo.action("zoom-lock")
            locked = zoom_state(demo)
            assert locked["locked"] is True
            demo.pointer.move_to(
                Point(demo.size.width * 0.12, demo.size.height * 0.3), duration=1.4
            )
            demo.pause(1.5)
            assert zoom_state(demo)["focal"] == locked["focal"]
            demo.frame("locked.png")
            demo.action("zoom-lock")
            demo.wait_until(
                lambda: zoom_state(demo)["focal"] != locked["focal"], "following after unlock"
            )
            assert zoom_state(demo)["locked"] is False
            demo.pause(1.5)
            demo.frame("unlocked.png")
            demo.action("reset-zoom")
            demo.wait_zoom(1.0)
            demo.pointer.move_to(start)
            demo.pause(1.0)
            demo.frame("restored.png")
        video = recording.stop()
    options.export(video)


if __name__ == "__main__":
    main()
