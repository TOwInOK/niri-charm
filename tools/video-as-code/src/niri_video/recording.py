"""Capture a private X11 window and export its lossless recording with FFmpeg.

Capture uses only the supplied display, authority file, and window ID. Final files
are published atomically without replacing existing files; interrupted or failed
encodes never leave a misleading final artifact.
"""

from __future__ import annotations

import math
import os
import signal
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType
from typing import BinaryIO

_THREADS = "2"
_STARTUP_TIMEOUT = 15.0
_STOP_TIMEOUT = 10.0
_SIGNAL_TIMEOUT = 3.0
_LOG_BYTES = 16_384


def _command() -> list[str]:
    return [
        "ffmpeg",
        "-hide_banner",
        "-loglevel",
        "warning",
        "-nostats",
        "-n",
        "-filter_threads",
        "1",
        "-filter_complex_threads",
        "1",
    ]


def _capture_environment(display: str, authority: Path) -> dict[str, str]:
    if not display:
        raise ValueError("An explicit X11 display is required.")
    if not authority.is_file():
        raise FileNotFoundError(f"X11 authority file does not exist: {authority}")
    environment = os.environ.copy()
    environment["DISPLAY"] = display
    environment["XAUTHORITY"] = str(authority.resolve())
    return environment


def _capture_input(
    *, display: str, window: int, width: int, height: int, fps: int, draw_mouse: bool
) -> list[str]:
    if not 0 < window <= 0xFFFFFFFF:
        raise ValueError("window must be a non-root X11 window ID (1..4294967295).")
    if width <= 0 or height <= 0 or fps <= 0:
        raise ValueError("Capture width, height, and fps must be positive.")
    return [
        "-threads",
        _THREADS,
        "-probesize",
        "32",
        "-f",
        "x11grab",
        "-framerate",
        str(fps),
        "-video_size",
        f"{width}x{height}",
        "-window_id",
        str(window),
        "-draw_mouse",
        str(int(draw_mouse)),
        "-i",
        display,
    ]


def _stage_output(output: Path) -> tuple[tempfile.TemporaryDirectory[str], Path]:
    if output.exists() or output.is_symlink():
        raise FileExistsError(f"Refusing to overwrite existing output: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    directory = tempfile.TemporaryDirectory(prefix=".niri-video-", dir=output.parent)
    return directory, Path(directory.name).resolve() / output.name


def _publish(staging: Path, output: Path) -> None:
    if not staging.is_file() or staging.stat().st_size == 0:
        raise RuntimeError("FFmpeg did not produce a nonempty output file.")
    # A same-filesystem hard link publishes exclusively, even if another writer
    # creates the destination after the initial existence check.
    os.link(staging, output)


def _log_tail(handle: BinaryIO) -> str:
    size = os.fstat(handle.fileno()).st_size
    return (
        os.pread(handle.fileno(), _LOG_BYTES, max(0, size - _LOG_BYTES))
        .decode("utf-8", errors="replace")
        .strip()
    )


def _error(reason: str, log: BinaryIO) -> RuntimeError:
    return RuntimeError(f"{reason}\nFFmpeg log:\n{_log_tail(log) or '(no diagnostic output)'}")


def _has_frame(data: bytes) -> bool:
    for line in data.splitlines():
        key, separator, value = line.partition(b"=")
        if separator and key == b"frame":
            try:
                if int(value) > 0:
                    return True
            except ValueError:
                continue
    return False


def _terminate_owned(process: subprocess.Popen[bytes]) -> None:
    # Every caller creates a fresh session, so its PID is its own process-group ID.
    for requested_signal in (signal.SIGTERM, signal.SIGKILL):
        if process.poll() is not None:
            return
        try:
            os.killpg(process.pid, requested_signal)
        except ProcessLookupError:
            process.wait(timeout=_SIGNAL_TIMEOUT)
            return
        try:
            process.wait(timeout=_SIGNAL_TIMEOUT)
            return
        except subprocess.TimeoutExpired:
            continue
    raise RuntimeError("The owned FFmpeg process did not exit after SIGKILL.")


def _spawn(
    command: list[str],
    *,
    log: BinaryIO,
    progress: BinaryIO,
    environment: dict[str, str] | None,
    interactive: bool,
) -> subprocess.Popen[bytes]:
    try:
        return subprocess.Popen(
            command,
            stdin=subprocess.PIPE if interactive else subprocess.DEVNULL,
            stdout=progress,
            stderr=log,
            env=environment,
            start_new_session=True,
        )
    except OSError as error:
        raise RuntimeError(f"Unable to launch FFmpeg: {error}") from error


def _wait_for_frame(process: subprocess.Popen[bytes], progress: BinaryIO, log: BinaryIO) -> None:
    deadline = time.monotonic() + _STARTUP_TIMEOUT
    offset = 0
    pending = b""
    while True:
        returncode = process.poll()
        if returncode is not None:
            raise _error(f"FFmpeg exited during capture startup (status {returncode}).", log)
        data = os.pread(progress.fileno(), 8192, offset)
        offset += len(data)
        pending += data
        complete, separator, pending = pending.rpartition(b"\n")
        if separator and _has_frame(complete):
            return
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise _error("FFmpeg did not capture its first frame within 15 seconds.", log)
        time.sleep(min(0.02, remaining))


def _run_ffmpeg(
    command: list[str],
    *,
    environment: dict[str, str] | None = None,
    timeout: float | None = None,
) -> None:
    with tempfile.TemporaryFile() as log, tempfile.TemporaryFile() as progress:
        process = _spawn(
            command + ["-progress", "pipe:1", "-stats_period", "0.1"],
            log=log,
            progress=progress,
            environment=environment,
            interactive=False,
        )
        try:
            returncode = process.wait(timeout=timeout)
        except BaseException as error:
            try:
                _terminate_owned(process)
            except Exception as shutdown_error:
                error.add_note(str(shutdown_error))
            if isinstance(error, subprocess.TimeoutExpired):
                raise _error("FFmpeg timed out while capturing a screenshot.", log) from error
            raise
        if returncode != 0:
            raise _error(f"FFmpeg failed (status {returncode}).", log)
        if not _has_frame(_log_tail(progress).encode()):
            raise _error(
                "FFmpeg produced no video frames; check the source and trim interval.", log
            )


@dataclass(frozen=True, slots=True)
class Video:
    """A source video that remains available for repeated exports.

    Attributes:
        path: Path to the lossless recording or another FFmpeg-readable video.
    """

    path: Path

    def export(
        self,
        output: Path,
        *,
        fps: int = 30,
        width: int | None = None,
        start: float = 0,
        end: float | None = None,
    ) -> Path:
        """Encode an MP4, infinitely looping GIF, or infinitely looping animated WebP.

        Args:
            output: New destination ending in .mp4, .gif, or .webp.
            fps: Positive output frame rate.
            width: Optional positive width; height follows the source aspect ratio.
                MP4 widths round up to even pixels, with an even proportional height.
            start: Inclusive start time in seconds, relative to the source's first frame.
            end: Optional exclusive end time in seconds, on the same timeline.

        Returns:
            The supplied output path, only after encoding and publication succeed.

        Raises:
            ValueError: The format, dimensions, frame rate, or trim interval is invalid.
            FileNotFoundError: The source does not exist.
            FileExistsError: The destination already exists.
            RuntimeError: FFmpeg fails or the interval contains no video frames.
        """
        suffix = output.suffix.lower()
        if suffix not in {".mp4", ".gif", ".webp"}:
            raise ValueError("Export output must end in .mp4, .gif, or .webp.")
        if fps <= 0 or (width is not None and width <= 0):
            raise ValueError("Export fps and width must be positive.")
        if not math.isfinite(start) or start < 0:
            raise ValueError("start must be a finite, nonnegative time in seconds.")
        if end is not None and (not math.isfinite(end) or end <= start):
            raise ValueError("end must be finite and greater than start.")
        if not self.path.is_file():
            raise FileNotFoundError(f"Video source does not exist: {self.path}")

        trim = f"trim=start={start}"
        if end is not None:
            trim += f":end={end}"
        filters = [trim, "setpts=PTS-STARTPTS", f"fps={fps}"]
        if suffix == ".mp4":
            target_width = str(((width + 1) // 2) * 2) if width is not None else "ceil(iw/2)*2"
            filters.append(f"scale=w={target_width}:h=-2:flags=lanczos")
        elif width is not None:
            filters.append(f"scale=w={width}:h=-1:flags=lanczos")
        video_filter = ",".join(filters)
        command = _command() + [
            "-nostdin",
            "-threads",
            _THREADS,
            "-i",
            str(self.path.resolve()),
            "-an",
            "-sn",
            "-dn",
        ]
        if suffix == ".gif":
            graph = (
                f"[0:v:0]{video_filter},split[frames][colors];"
                "[colors]palettegen=stats_mode=diff[palette];"
                "[frames][palette]paletteuse=dither=sierra2_4a:diff_mode=rectangle[video]"
            )
            command += ["-filter_complex", graph, "-map", "[video]", "-c:v", "gif", "-loop", "0"]
        else:
            command += ["-map", "0:v:0", "-vf", video_filter]
            if suffix == ".mp4":
                command += [
                    "-c:v",
                    "libx264",
                    "-crf",
                    "18",
                    "-preset",
                    "medium",
                    "-pix_fmt",
                    "yuv420p",
                    "-movflags",
                    "+faststart",
                ]
            else:
                command += [
                    "-c:v",
                    "libwebp_anim",
                    "-lossless",
                    "1",
                    "-quality",
                    "100",
                    "-compression_level",
                    "6",
                    "-loop",
                    "0",
                ]
        directory, staging = _stage_output(output)
        try:
            _run_ffmpeg(command + ["-threads", _THREADS, "-f", suffix[1:], str(staging)])
            _publish(staging, output)
        finally:
            directory.cleanup()
        return output


class Recorder:
    """Record one explicit X11 window to a lossless FFV1/bgr0 Matroska video.

    Entering waits for FFmpeg's first encoded frame, rather than merely its process
    startup. Exiting flushes the raw recording even if the scenario raises, but never
    exports it automatically. Each recorder can be entered once.
    """

    def __init__(
        self,
        *,
        display: str,
        authority: Path,
        window: int,
        width: int,
        height: int,
        output: Path,
        fps: int = 60,
        draw_mouse: bool = True,
    ) -> None:
        """Configure capture without launching processes or creating output files."""
        if output.suffix.lower() != ".mkv":
            raise ValueError("The raw recording output must end in .mkv.")
        self._input = _capture_input(
            display=display,
            window=window,
            width=width,
            height=height,
            fps=fps,
            draw_mouse=draw_mouse,
        )
        self._environment = _capture_environment(display, authority)
        self._output = output
        self._directory: tempfile.TemporaryDirectory[str] | None = None
        self._staging: Path | None = None
        self._process: subprocess.Popen[bytes] | None = None
        self._log: BinaryIO | None = None
        self._progress: BinaryIO | None = None
        self._video: Video | None = None
        self._failure: BaseException | None = None
        self._entered = False

    @property
    def is_recording(self) -> bool:
        """Return whether this recorder still owns an active capture process."""
        return (
            self._process is not None
            and self._failure is None
            and self._video is None
            and self._process.poll() is None
        )

    def __enter__(self) -> Recorder:
        """Start recording and wait at most 15 seconds for the first encoded frame."""
        if self._entered:
            raise RuntimeError("A Recorder can only be entered once.")
        self._entered = True
        try:
            self._directory, self._staging = _stage_output(self._output)
            self._log = tempfile.TemporaryFile()
            # A regular file keeps progress flowing for arbitrarily long scenarios;
            # an undrained PIPE would eventually block FFmpeg after startup.
            self._progress = tempfile.TemporaryFile()
            command = (
                _command()
                + self._input
                + [
                    "-an",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "bgr0",
                    "-threads",
                    _THREADS,
                    "-progress",
                    "pipe:1",
                    "-stats_period",
                    "0.1",
                    "-f",
                    "matroska",
                    str(self._staging),
                ]
            )
            self._process = _spawn(
                command,
                log=self._log,
                progress=self._progress,
                environment=self._environment,
                interactive=True,
            )
            _wait_for_frame(self._process, self._progress, self._log)
        except BaseException as error:
            self._failure = error
            if self._process is not None:
                try:
                    _terminate_owned(self._process)
                except Exception as shutdown_error:
                    error.add_note(str(shutdown_error))
            self._cleanup()
            raise
        return self

    def __exit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Flush capture while preserving an exception raised by the scenario."""
        try:
            self.stop()
        except Exception as error:
            if exception is None:
                raise
            exception.add_note(f"Recording shutdown also failed: {error}")

    def stop(self) -> Video:
        """Gracefully flush recording and return the same Video on subsequent calls.

        FFmpeg receives ``q`` first. A stalled shutdown escalates to SIGTERM and
        SIGKILL only in its owned process group. Failed capture never publishes a
        final MKV, and subsequent calls report the original failure.

        Raises:
            RuntimeError: Recording was not started, FFmpeg fails, or shutdown stalls.
            FileExistsError: Another writer has created the destination.
        """
        if self._video is not None:
            return self._video
        if self._failure is not None:
            raise self._failure
        process, log, staging = self._process, self._log, self._staging
        if process is None or log is None or staging is None:
            raise RuntimeError("Recording has not been started; enter the Recorder first.")
        try:
            if process.poll() is None and process.stdin is not None:
                try:
                    process.stdin.write(b"q\n")
                    process.stdin.flush()
                except BrokenPipeError:
                    pass
                try:
                    process.stdin.close()
                except BrokenPipeError:
                    pass
            try:
                returncode = process.wait(timeout=_STOP_TIMEOUT)
            except subprocess.TimeoutExpired as error:
                _terminate_owned(process)
                raise _error("FFmpeg did not flush recording within 10 seconds.", log) from error
            if returncode != 0:
                raise _error(f"FFmpeg recording failed (status {returncode}).", log)
            _publish(staging, self._output)
            self._video = Video(self._output)
            return self._video
        except BaseException as error:
            self._failure = error
            try:
                _terminate_owned(process)
            except Exception as shutdown_error:
                error.add_note(str(shutdown_error))
            raise
        finally:
            self._cleanup()

    def _cleanup(self) -> None:
        if self._process is not None and self._process.stdin is not None:
            try:
                self._process.stdin.close()
            except BrokenPipeError:
                pass
        for handle in (self._log, self._progress):
            if handle is not None:
                handle.close()
        self._log = None
        self._progress = None
        if self._directory is not None:
            self._directory.cleanup()
            self._directory = None
        self._staging = None


def screenshot(
    *,
    display: str,
    authority: Path,
    window: int,
    width: int,
    height: int,
    output: Path,
    draw_mouse: bool = True,
) -> Path:
    """Capture one visible X11-window frame without replacing an existing image.

    Args:
        display: The owned X11 display to capture.
        authority: Its authentication file.
        window: An explicit, non-root X11 window ID.
        width: Positive capture width in pixels.
        height: Positive capture height in pixels.
        output: A new image path with an FFmpeg-supported image extension, such as .png.
        draw_mouse: Whether to include the visible pointer.

    Returns:
        The supplied path after the image has been captured and published.

    Raises:
        ValueError: Capture parameters are invalid.
        FileNotFoundError: The authority file is missing.
        FileExistsError: The destination exists.
        RuntimeError: FFmpeg fails or takes more than 15 seconds.
    """
    environment = _capture_environment(display, authority)
    command = (
        _command()
        + ["-nostdin"]
        + _capture_input(
            display=display,
            window=window,
            width=width,
            height=height,
            fps=60,
            draw_mouse=draw_mouse,
        )
    )
    directory, staging = _stage_output(output)
    try:
        _run_ffmpeg(
            command
            + [
                "-frames:v",
                "1",
                "-threads",
                _THREADS,
                "-f",
                "image2",
                "-update",
                "1",
                str(staging),
            ],
            environment=environment,
            timeout=_STARTUP_TIMEOUT,
        )
        _publish(staging, output)
    finally:
        directory.cleanup()
    return output
