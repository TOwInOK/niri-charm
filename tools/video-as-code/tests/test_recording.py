"""Exercise consumer-visible recording failures and real FFmpeg export edge cases."""

from __future__ import annotations

import hashlib
import json
import shutil
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import ClassVar, NotRequired, TypedDict, cast

from niri_video.recording import Recorder, Video


class _Stream(TypedDict):
    codec_name: str
    width: int
    height: int
    pix_fmt: str
    nb_read_frames: str
    duration: NotRequired[str]
    display_aspect_ratio: NotRequired[str]


class _Probe(TypedDict):
    streams: list[_Stream]


def _probe(path: Path) -> _Stream:
    result = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt,nb_read_frames,duration,display_aspect_ratio",
            "-of",
            "json",
            str(path),
        ],
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
    )
    return cast(_Probe, json.loads(result.stdout))["streams"][0]


def _webp_chunks(path: Path) -> list[tuple[bytes, bytes]]:
    data = path.read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        raise AssertionError("The encoder did not produce a WebP RIFF container.")
    if struct.unpack_from("<I", data, 4)[0] + 8 != len(data):
        raise AssertionError("The WebP container is truncated.")
    chunks: list[tuple[bytes, bytes]] = []
    offset = 12
    while offset < len(data):
        tag = data[offset : offset + 4]
        length = struct.unpack_from("<I", data, offset + 4)[0]
        start = offset + 8
        end = start + length
        if end > len(data):
            raise AssertionError("The WebP chunk is truncated.")
        chunks.append((tag, data[start:end]))
        offset = end + (length % 2)
    return chunks


@unittest.skipUnless(shutil.which("ffmpeg") and shutil.which("ffprobe"), "FFmpeg is required")
class ExportTests(unittest.TestCase):
    """Check real exported frames, aspect ratios, loops, and failure publication."""

    _source_directory: ClassVar[tempfile.TemporaryDirectory[str]]
    _source: ClassVar[Path]
    _video: ClassVar[Video]
    _source_digest: ClassVar[str]

    @classmethod
    def setUpClass(cls) -> None:
        """Generate a short moving RGB source with deliberately odd dimensions."""
        cls._source_directory = tempfile.TemporaryDirectory(prefix="niri-video-tests-")
        cls.addClassCleanup(cls._source_directory.cleanup)
        cls._source = Path(cls._source_directory.name) / "source.mkv"
        subprocess.run(
            [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-n",
                "-filter_threads",
                "1",
                "-threads",
                "2",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=161x91:rate=12:duration=1.5",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "bgr0",
                "-threads",
                "2",
                str(cls._source),
            ],
            check=True,
            capture_output=True,
            timeout=15,
        )
        cls._video = Video(cls._source)
        cls._source_digest = hashlib.sha256(cls._source.read_bytes()).hexdigest()

    def setUp(self) -> None:
        """Give every test a fresh destination directory."""
        directory = tempfile.TemporaryDirectory(prefix="niri-video-export-tests-")
        self.addCleanup(directory.cleanup)
        self.output = Path(directory.name)

    def test_mp4_preserves_aspect_and_trims_to_real_frames(self) -> None:
        """An odd width becomes even, without stretching or losing the requested interval."""
        output = self.output / "clip.mp4"
        self._video.export(output, fps=12, width=101, start=0.25, end=1.0)
        stream = _probe(output)
        self.assertEqual(stream["codec_name"], "h264")
        self.assertEqual(stream["pix_fmt"], "yuv420p")
        self.assertEqual((stream["width"], stream["height"]), (102, 58))
        self.assertEqual(stream.get("display_aspect_ratio"), "23:13")
        self.assertEqual(stream["nb_read_frames"], "9")
        assert "duration" in stream
        self.assertAlmostEqual(float(stream["duration"]), 0.75, places=2)
        encoded = output.read_bytes()
        self.assertLess(encoded.index(b"moov"), encoded.index(b"mdat"))

    def test_animated_formats_have_frames_and_infinite_loops(self) -> None:
        """GIF has decoded frames and a palette; WebP has multiple actual animation chunks."""
        gif = self.output / "clip.gif"
        webp = self.output / "clip.webp"
        for output in (gif, webp):
            self._video.export(output, fps=12, width=101, start=0.25, end=1.0)
        gif_stream = _probe(gif)
        self.assertEqual(gif_stream["nb_read_frames"], "9")
        self.assertEqual((gif_stream["width"], gif_stream["height"]), (101, 57))
        self.assertIn(b"NETSCAPE2.0\x03\x01\x00\x00\x00", gif.read_bytes())
        chunks = _webp_chunks(webp)
        animation = next(payload for tag, payload in chunks if tag == b"ANIM")
        self.assertEqual(struct.unpack_from("<H", animation, 4)[0], 0)
        self.assertEqual(sum(tag == b"ANMF" for tag, _ in chunks), 9)
        self.assertEqual(hashlib.sha256(self._source.read_bytes()).hexdigest(), self._source_digest)

    def test_no_overwrite_including_dangling_symlink(self) -> None:
        """Neither a real file nor a broken symlink can be silently replaced."""
        existing = self.output / "existing.mp4"
        existing.write_bytes(b"keep this content")
        with self.assertRaises(FileExistsError):
            self._video.export(existing)
        self.assertEqual(existing.read_bytes(), b"keep this content")
        dangling = self.output / "dangling.gif"
        dangling.symlink_to(self.output / "missing.gif")
        with self.assertRaises(FileExistsError):
            self._video.export(dangling)
        self.assertTrue(dangling.is_symlink())

    def test_empty_intervals_never_publish_empty_artifacts(self) -> None:
        """Successful FFmpeg exit with zero frames is still an export failure."""
        for suffix in ("mp4", "gif", "webp"):
            with self.subTest(format=suffix):
                output = self.output / f"empty.{suffix}"
                with self.assertRaises(RuntimeError):
                    self._video.export(output, start=10, end=11)
                self.assertFalse(output.exists())
        self.assertEqual(list(self.output.iterdir()), [])
        self.assertEqual(hashlib.sha256(self._source.read_bytes()).hexdigest(), self._source_digest)

    def test_invalid_source_removes_partial_output(self) -> None:
        """A decode error leaves no partial export and never modifies the source."""
        source = self.output / "broken.mkv"
        source.write_bytes(b"not a video")
        output = self.output / "broken.mp4"
        with self.assertRaises(RuntimeError):
            Video(source).export(output)
        self.assertEqual(source.read_bytes(), b"not a video")
        self.assertEqual(list(self.output.iterdir()), [source])

    def test_invalid_intervals_and_formats_are_rejected(self) -> None:
        """Invalid timestamps and unsupported formats cannot create a destination."""
        output = self.output / "invalid.mp4"
        for start, end in ((-1.0, None), (float("nan"), None), (1.0, 1.0)):
            with self.subTest(start=start, end=end), self.assertRaises(ValueError):
                self._video.export(output, start=start, end=end)
        with self.assertRaises(ValueError):
            self._video.export(output, end=float("inf"))
        with self.assertRaises(ValueError):
            self._video.export(self.output / "unsupported.png")
        self.assertEqual(list(self.output.iterdir()), [])


@unittest.skipUnless(shutil.which("ffmpeg"), "FFmpeg is required")
class RecorderFailureTests(unittest.TestCase):
    """Check state and diagnostics when an explicitly nonexistent display fails."""

    def test_failed_start_leaves_no_capture_or_active_recorder(self) -> None:
        """No frame, output, or active recorder is claimed after actual X11 connection failure."""
        with tempfile.TemporaryDirectory(prefix="niri-video-capture-tests-") as temporary:
            root = Path(temporary)
            authority = root / "authority"
            authority.touch()
            output = root / "capture.mkv"
            recorder = Recorder(
                display=":2147483647",
                authority=authority,
                window=1,
                width=64,
                height=48,
                output=output,
            )
            self.assertFalse(recorder.is_recording)
            with self.assertRaises(RuntimeError):
                recorder.stop()
            with self.assertRaises(RuntimeError):
                with recorder:
                    self.fail("Capture started on a nonexistent display.")
            self.assertFalse(recorder.is_recording)
            self.assertFalse(output.exists())
            with self.assertRaises(RuntimeError):
                recorder.stop()
            self.assertEqual(list(root.iterdir()), [authority])
