"""Shared command-line options for ordinary, independently runnable scenarios.

Scenarios remain Python programs rather than a second DSL. This module only
handles repeatable invocation and output naming; interactions live in the script.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Literal

from niri_video.geometry import Size
from niri_video.recording import Video

type Format = Literal["mp4", "webp", "gif"]


@dataclass(frozen=True)
class ScenarioOptions:
    """Hold checked CLI settings and the unique output directory for one run."""

    output: Path
    size: Size
    niri: str
    fps: int
    formats: tuple[Format, ...]

    @classmethod
    def parse(cls, name: str, description: str, *, output: Path) -> ScenarioOptions:
        """Parse common options; ``--help`` never launches a GUI or creates files."""
        parser = argparse.ArgumentParser(description=description)
        parser.add_argument("--output", type=Path, default=output, help="Parent output directory")
        parser.add_argument(
            "--niri", default="niri-charm", help="Fork binary or absolute build path"
        )
        parser.add_argument("--width", type=int, default=1280)
        parser.add_argument("--height", type=int, default=720)
        parser.add_argument("--fps", type=int, default=60, help="Source capture frame rate")
        parser.add_argument(
            "--format",
            action="append",
            choices=("mp4", "webp", "gif"),
            help="Repeat to select exports; defaults to all three formats",
        )
        args = parser.parse_args()
        if not 1 <= args.fps <= 120:
            parser.error("--fps must be between 1 and 120")
        try:
            size = Size(args.width, args.height)
        except ValueError as error:
            parser.error(str(error))
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
        directory: Path = args.output / f"{name}-{stamp}"
        directory.mkdir(parents=True, exist_ok=False)
        formats: tuple[Format, ...] = tuple(dict.fromkeys(args.format or ("mp4", "webp", "gif")))
        return cls(directory, size, str(args.niri), int(args.fps), formats)

    def export(self, video: Video) -> tuple[Path, ...]:
        """Encode selected deliverables, retaining the lossless source and logs."""
        paths = tuple(
            video.export(self.output / f"preview.{format_}", fps=min(self.fps, 30))
            for format_ in self.formats
        )
        for path in paths:
            print(path.resolve())
        return paths
