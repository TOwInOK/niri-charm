"""Show automatic sampling switching from linear at 2x to nearest at 4x."""

from pathlib import Path
from tempfile import TemporaryDirectory

from niri_video import Point, PreviewSession, ScenarioOptions


def samples(demo: PreviewSession) -> tuple[tuple[int, int, int], ...]:
    """Observe a fixed grid of framebuffer pixels, not just IPC state."""
    return tuple(
        demo.pixel(Point(demo.size.width * x / 32, demo.size.height * y / 24))
        for y in range(1, 24)
        for x in range(1, 32)
    )


def main() -> None:
    """Record 1x to 2x to 4x to 1x, then verify both filters against explicit policies."""
    options = ScenarioOptions.parse(
        "zoom-sampling",
        "Demonstrate automatic linear-to-nearest filtering at the 4x threshold",
        output=Path(__file__).resolve().parents[1] / "output",
    )
    with (
        TemporaryDirectory(prefix="niri-zoom-sampling-") as directory,
        PreviewSession(output=options.output, size=options.size, niri=options.niri) as demo,
    ):
        configs: dict[str, Path] = {}
        for policy in ("auto", "linear", "nearest"):
            config = Path(directory) / f"{policy}.kdl"
            threshold = " threshold=4.0" if policy == "auto" else ""
            _ = config.write_text(
                demo.profile.config()
                + f'\nzoom {{ increment-factor 2.0; sampling "{policy}"{threshold}; }}\n',
                encoding="utf-8",
            )
            configs[policy] = config
        demo.action("load-config-file", "--path", str(configs["auto"]))
        demo.terminal("\x1b[?25l\n\x1b[1;36mAa 08\x1b[0m")
        demo.wait_pixel(Point(demo.size.width * 0.5, demo.size.height * 0.7), (16, 21, 33))
        demo.pointer.move_to(Point(demo.size.width * 0.065, demo.size.height * 0.16))
        with demo.record(fps=options.fps) as recording:
            demo.frame("native.png")
            demo.pause(1.5)
            demo.action("zoom", "2.0", "--lock")
            demo.wait_zoom(2.0)
            smooth = samples(demo)
            demo.frame("linear-2x.png")
            demo.pause(2.0)
            demo.action("zoom-in")
            demo.wait_zoom(4.0)
            sharp = samples(demo)
            demo.frame("nearest-4x.png")
            demo.pause(2.0)
            demo.action("zoom", "2.0", "--lock")
            demo.wait_zoom(1.0)
            demo.pause(1.5)
            demo.frame("restored.png")
        video = recording.stop()

        # Reference checks are outside the recording: the clip never changes policy.
        demo.action("zoom", "2.0", "--lock")
        demo.wait_zoom(2.0)
        demo.action("load-config-file", "--path", str(configs["linear"]))
        demo.wait_until(lambda: samples(demo) == smooth, "auto at 2x to match linear")
        demo.action("load-config-file", "--path", str(configs["nearest"]))
        demo.wait_until(lambda: samples(demo) != smooth, "nearest at 2x to differ from auto")
        demo.action("zoom-in")
        demo.wait_zoom(4.0)
        demo.wait_until(lambda: samples(demo) == sharp, "auto at 4x to match nearest")
        demo.action("load-config-file", "--path", str(configs["linear"]))
        demo.wait_until(lambda: samples(demo) != sharp, "linear at 4x to differ from auto")
    options.export(video)


if __name__ == "__main__":
    main()
