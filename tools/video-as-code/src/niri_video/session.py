"""Own a private Xwayland, nested niri, clients and their capture lifecycle.

Nothing is sent to the host niri socket or X display. Only Xwayland connects to
the existing Wayland desktop; all demo clients receive the private runtime and
HOME. Software Mesa rendering is deliberate for this capture-only environment,
not a workaround applied to the user's graphics configuration.
"""

from __future__ import annotations

import json
import math
import os
import secrets
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType
from typing import cast

from niri_video.geometry import Point, Size
from niri_video.input import Keyboard, Pointer
from niri_video.profile import CaptureProfile
from niri_video.recording import Recorder, screenshot
from niri_video.x11 import X11

type Json = None | bool | int | float | str | list[Json] | dict[str, Json]


@dataclass(frozen=True)
class Window:
    """Identify a native client by the ID returned by this session's niri IPC."""

    id: int
    title: str
    app_id: str
    focused: bool


@dataclass
class _Child:
    process: subprocess.Popen[bytes]
    log: Path
    essential: bool


class PreviewSession:
    """Run real GUI interactions in a disposable, authenticated nested desktop.

    Use as a context manager. ``output`` retains recordings and process logs;
    temporary authentication, sockets and application profiles are removed on
    exit. The instance is one-shot and must not be shared between threads.
    """

    def __init__(
        self,
        *,
        output: Path,
        size: Size | None = None,
        niri: str = "niri-charm",
        timeout: float = 15.0,
    ) -> None:
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("Session timeout must be positive and finite")
        self.output = output.resolve()
        self.size = size or Size(1280, 720)
        self.profile = CaptureProfile(self.size)
        self.niri = niri
        self.timeout = timeout
        self._children: list[_Child] = []
        self._temporary: tempfile.TemporaryDirectory[str] | None = None
        self._connection: X11 | None = None
        self._pointer: Pointer | None = None
        self._keyboard: Keyboard | None = None
        self._recorders: list[Recorder] = []
        self._started = False
        self._display = ""
        self._window = 0
        self._authority: Path | None = None
        self._runtime: Path | None = None
        self._environment: dict[str, str] = {}
        self._client_environment: dict[str, str] = {}

    def __enter__(self) -> PreviewSession:
        if self._started:
            raise RuntimeError("Create a new PreviewSession for each capture run")
        self._started = True
        try:
            self._start()
        except BaseException:
            self.close()
            raise
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()

    @property
    def pointer(self) -> Pointer:
        """Return the smooth pointer bound to the owned niri window."""
        if self._pointer is None:
            raise RuntimeError("Enter the session before using its pointer")
        return self._pointer

    @property
    def keyboard(self) -> Keyboard:
        """Return the keyboard bound to the private X display."""
        if self._keyboard is None:
            raise RuntimeError("Enter the session before using its keyboard")
        return self._keyboard

    def _spawn(
        self,
        argv: Sequence[str],
        environment: Mapping[str, str],
        name: str,
        *,
        essential: bool = False,
        pass_fds: tuple[int, ...] = (),
    ) -> subprocess.Popen[bytes]:
        log = self.output / f"{name}.log"
        with log.open("xb") as stream:
            process = subprocess.Popen(
                argv,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=stream,
                stderr=subprocess.STDOUT,
                pass_fds=pass_fds,
                start_new_session=True,
            )
        self._children.append(_Child(process, log, essential))
        return process

    def _start(self) -> None:
        for executable in (self.niri, "Xwayland", "xauth", "ffmpeg"):
            if shutil.which(executable) is None:
                raise RuntimeError(f"Required executable is unavailable: {executable}")
        if not os.environ.get("WAYLAND_DISPLAY") or not os.environ.get("XDG_RUNTIME_DIR"):
            raise RuntimeError("Run captures from an existing Wayland desktop")
        vendors = Path("/usr/share/glvnd/egl_vendor.d").glob("*.json")
        mesa = next((p for p in vendors if "mesa" in p.read_text().lower()), None)
        if mesa is None:
            raise RuntimeError("A Mesa EGL vendor is required for isolated software capture")
        self.output.mkdir(parents=True, exist_ok=True)
        self._temporary = tempfile.TemporaryDirectory(prefix="niri-video-")
        work = Path(self._temporary.name)
        self._runtime = work / "runtime"
        self._runtime.mkdir(mode=0o700)
        self._authority = work / "Xauthority"
        self._authority.touch(mode=0o600)
        cookie = secrets.token_bytes(16)
        self._authorize(":99", cookie)

        read_fd, write_fd = os.pipe()
        try:
            self._spawn(
                [
                    "Xwayland",
                    "-displayfd",
                    str(write_fd),
                    "-geometry",
                    f"{self.size.width}x{self.size.height}",
                    "-shm",
                    "-nolisten",
                    "tcp",
                    "-auth",
                    str(self._authority),
                    "-noreset",
                ],
                os.environ.copy(),
                "xwayland",
                essential=True,
                pass_fds=(write_fd,),
            )
        except BaseException:
            os.close(read_fd)
            raise
        finally:
            os.close(write_fd)
        try:
            deadline = time.monotonic() + self.timeout
            ready = b""
            while b"\n" not in ready:
                self._check_children()
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("Xwayland did not publish its private display")
                if select.select([read_fd], [], [], min(remaining, 0.1))[0]:
                    chunk = os.read(read_fd, 64)
                    if not chunk:
                        raise RuntimeError("Xwayland closed its readiness pipe before startup")
                    ready += chunk
            number = ready.strip().decode("ascii")
            if not number.isdecimal():
                raise RuntimeError(f"Invalid Xwayland display number: {number!r}")
            self._display = f":{number}"
        finally:
            os.close(read_fd)
        self._authorize(self._display, cookie)
        self._connection = X11(self._display, cookie)

        environment = os.environ.copy()
        for key in (
            "WAYLAND_DISPLAY",
            "DISPLAY",
            "NIRI_SOCKET",
            "DBUS_SESSION_BUS_ADDRESS",
            "NOTIFY_SOCKET",
            "WAYLAND_SOCKET",
            "DESKTOP_STARTUP_ID",
            "XDG_ACTIVATION_TOKEN",
        ):
            environment.pop(key, None)
        for variable, directory in (
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
        ):
            path = work / directory
            path.mkdir()
            environment[variable] = str(path)
        environment.update(
            DISPLAY=self._display,
            XAUTHORITY=str(self._authority),
            XDG_RUNTIME_DIR=str(self._runtime),
            WINIT_UNIX_BACKEND="x11",
            WINIT_X11_SCALE_FACTOR="1",
            LIBGL_ALWAYS_SOFTWARE="1",
            __EGL_VENDOR_LIBRARY_FILENAMES=str(mesa),
            GSETTINGS_BACKEND="memory",
            GTK_USE_PORTAL="0",
            NO_AT_BRIDGE="1",
        )
        self._environment = environment
        config = work / "capture.kdl"
        config.write_text(self.profile.config())
        validation = subprocess.run(
            [self.niri, "validate", "--config", str(config)],
            env=environment,
            capture_output=True,
            text=True,
            timeout=self.timeout,
        )
        if validation.returncode:
            raise RuntimeError(f"Capture config is invalid:\n{validation.stderr}")
        self._spawn([self.niri, "--config", str(config)], environment, "niri", essential=True)
        self.wait_until(self._sockets_ready, "niri IPC and Wayland sockets")
        self.wait_until(self._find_window, "niri X11 window")
        self._connection.resize(self._window, self.size)
        self._connection.focus(self._window)
        self.wait_until(self._geometry_ready, "requested niri output geometry")
        self._pointer = Pointer(self._connection, self._window, self.size)
        self._keyboard = Keyboard(self._connection)
        self.pointer.move_to(Point(self.size.width / 2, self.size.height / 2), duration=0.2)

    def _authorize(self, display: str, cookie: bytes) -> None:
        subprocess.run(
            [
                "xauth",
                "-f",
                str(self._authority),
                "add",
                display,
                "MIT-MAGIC-COOKIE-1",
                cookie.hex(),
            ],
            check=True,
            capture_output=True,
            timeout=self.timeout,
        )

    def _sockets_ready(self) -> bool:
        if self._runtime is None:
            return False
        ipc = next((p for p in self._runtime.glob("niri*.sock") if p.is_socket()), None)
        wayland = next((p for p in self._runtime.glob("wayland-*") if p.is_socket()), None)
        if ipc is None or wayland is None:
            return False
        self._environment["NIRI_SOCKET"] = str(ipc)
        self._client_environment = self._environment.copy()
        self._client_environment.pop("DISPLAY", None)
        self._client_environment.pop("WINIT_UNIX_BACKEND", None)
        self._client_environment.update(WAYLAND_DISPLAY=wayland.name, GDK_BACKEND="wayland")
        return True

    def _find_window(self) -> bool:
        if self._connection is None:
            return False
        candidates = self._connection.children()
        for window in candidates:
            _, _, width, height = self._connection.geometry(window)
            if width >= 64 and height >= 64:
                self._window = window
                return True
        return False

    def _geometry_ready(self) -> bool:
        if self._connection is None:
            return False
        _, _, width, height = self._connection.geometry(self._window)
        if (width, height) != (self.size.width, self.size.height):
            return False
        outputs = self.query("outputs")
        if not isinstance(outputs, dict):
            return False
        for output in outputs.values():
            if not isinstance(output, dict):
                continue
            logical = output.get("logical")
            if isinstance(logical, dict):
                return (logical.get("width"), logical.get("height")) == (
                    self.size.width,
                    self.size.height,
                )
        return False

    def _check_children(self) -> None:
        for child in self._children:
            status = child.process.poll()
            if child.essential and status is not None:
                detail = child.log.read_text(errors="replace")[-6000:]
                raise RuntimeError(f"{child.log.stem} exited with {status}:\n{detail}")

    def wait_until(self, predicate: Callable[[], bool], description: str) -> None:
        """Wait for observable readiness; fail rather than recording a guessed state."""
        deadline = time.monotonic() + self.timeout
        while not predicate():
            self._check_children()
            if time.monotonic() >= deadline:
                raise TimeoutError(f"Timed out waiting for {description}")
            time.sleep(0.02)

    def pause(self, seconds: float) -> None:
        """Hold a ready scene for the audience, not as a substitute for readiness."""
        if not math.isfinite(seconds) or seconds < 0:
            raise ValueError("Pause duration must be finite and non-negative")
        self._check_children()
        time.sleep(seconds)
        self._check_children()

    def query(self, topic: str) -> Json:
        """Read JSON from the private compositor, never the inherited host socket."""
        if "NIRI_SOCKET" not in self._environment:
            raise RuntimeError("The private IPC socket is not ready")
        result = subprocess.run(
            [self.niri, "msg", "--json", topic],
            env=self._environment,
            capture_output=True,
            text=True,
            check=True,
            timeout=self.timeout,
        )
        return cast(Json, json.loads(result.stdout))

    def action(self, name: str, *arguments: str) -> None:
        """Execute a real niri action using an argv list rather than a shell."""
        if "NIRI_SOCKET" not in self._environment:
            raise RuntimeError("The private IPC socket is not ready")
        subprocess.run(
            [self.niri, "msg", "action", name, *arguments],
            env=self._environment,
            capture_output=True,
            text=True,
            check=True,
            timeout=self.timeout,
        )

    def wait_zoom(self, level: float) -> None:
        """Await the rendered and target zoom levels of the single private output."""
        if not math.isfinite(level) or level < 1:
            raise ValueError("Zoom level must be finite and at least one")

        def settled() -> bool:
            states = self.query("zoom")
            if not isinstance(states, list) or len(states) != 1:
                return False
            state = states[0]
            if not isinstance(state, dict):
                return False
            current, target = state.get("level"), state.get("target_level")
            return (
                isinstance(current, int | float)
                and isinstance(target, int | float)
                and math.isclose(current, level, abs_tol=0.001)
                and math.isclose(target, level, abs_tol=0.001)
            )

        self.wait_until(settled, f"desktop zoom to settle at {level}")

    def windows(self) -> tuple[Window, ...]:
        """Return typed client identities from this compositor's window list."""
        payload = self.query("windows")
        if not isinstance(payload, list):
            raise RuntimeError("niri returned an invalid window list")
        windows: list[Window] = []
        for row in payload:
            if not isinstance(row, dict) or not isinstance(row.get("id"), int):
                raise RuntimeError("niri returned an invalid window record")
            identifier = row["id"]
            assert isinstance(identifier, int)
            windows.append(
                Window(
                    identifier,
                    str(row.get("title") or ""),
                    str(row.get("app_id") or ""),
                    row.get("is_focused") is True,
                )
            )
        return tuple(windows)

    def focus(self, window: Window) -> None:
        """Focus a native window and await the compositor's reported focus state."""
        self.action("focus-window", "--id", str(window.id))
        self.wait_until(
            lambda: any(w.id == window.id and w.focused for w in self.windows()),
            f"focus on {window.title!r}",
        )

    def launch(self, argv: Sequence[str]) -> subprocess.Popen[bytes]:
        """Start and own a Wayland client with an isolated home and runtime."""
        if not self._client_environment:
            raise RuntimeError("Enter the session before launching a client")
        return self._spawn(argv, self._client_environment, f"client-{len(self._children)}")

    def terminal(self, text: str, *, title: str = "Niri feature preview") -> Window:
        """Open Ghostty displaying only supplied demo text, without a personal shell."""
        if shutil.which("ghostty") is None:
            raise RuntimeError("The terminal helper requires Ghostty")
        previous = {window.id for window in self.windows()}
        self.launch(
            [
                "ghostty",
                "--config-default-files=false",
                "--gtk-single-instance=false",
                "--window-decoration=false",
                "--shell-integration=none",
                "--font-size=20",
                "--background=#101521",
                "--foreground=#dce5f3",
                "--window-padding-x=40",
                "--window-padding-y=36",
                "--cursor-style-blink=false",
                f"--title={title}",
                "-e",
                sys.executable,
                "-c",
                "import signal, sys; print(sys.argv[1], flush=True); signal.pause()",
                text,
            ]
        )
        self.wait_until(
            lambda: any(w.id not in previous and w.title == title for w in self.windows()),
            f"terminal {title!r}",
        )
        window = next(w for w in self.windows() if w.id not in previous and w.title == title)
        self.focus(window)
        return window

    def pixel(self, point: Point) -> tuple[int, int, int]:
        """Read a real rendered RGB pixel from the nested compositor window."""
        if self._connection is None:
            raise RuntimeError("Enter the session before inspecting pixels")
        return self._connection.pixel(self._window, point)

    def wait_pixel(self, point: Point, color: tuple[int, int, int], tolerance: int = 3) -> None:
        """Await a rendered color, useful for palette readiness or committed strokes."""
        self.wait_until(
            lambda: all(
                abs(a - b) <= tolerance for a, b in zip(self.pixel(point), color, strict=True)
            ),
            f"pixel {point} to become {color}",
        )

    def record(self, name: str = "capture", *, fps: int = 60) -> Recorder:
        """Create a lossless recorder for the owned window, including niri's cursor."""
        if self._authority is None or self._connection is None:
            raise RuntimeError("Enter the session before recording")
        if Path(name).name != name or name in ("", ".", ".."):
            raise ValueError("Recording name must be a single filename stem")
        recorder = Recorder(
            display=self._display,
            authority=self._authority,
            window=self._window,
            width=self.size.width,
            height=self.size.height,
            output=self.output / f"{name}.mkv",
            fps=fps,
            draw_mouse=False,
        )
        self._recorders.append(recorder)
        return recorder

    def frame(self, name: str) -> Path:
        """Save visible editor chrome and drawings, not niri's overlay-free export."""
        if self._authority is None or self._connection is None:
            raise RuntimeError("Enter the session before capturing a frame")
        if Path(name).name != name or not name.endswith(".png"):
            raise ValueError("Frame name must be a single PNG filename")
        return screenshot(
            display=self._display,
            authority=self._authority,
            window=self._window,
            width=self.size.width,
            height=self.size.height,
            output=self.output / name,
            draw_mouse=False,
        )

    def close(self) -> None:
        """Release input and stop only processes created by this session."""
        try:
            for recorder in reversed(self._recorders):
                if recorder.is_recording:
                    recorder.stop()
        finally:
            self._recorders.clear()
            if self._connection is not None:
                self._connection.close()
                self._connection = None
            self._pointer = None
            self._keyboard = None
            for child in reversed(self._children):
                if child.process.poll() is not None:
                    continue
                try:
                    os.killpg(child.process.pid, signal.SIGTERM)
                    child.process.wait(timeout=3)
                except ProcessLookupError:
                    pass
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(child.process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    child.process.wait(timeout=3)
            self._children.clear()
            if self._temporary is not None:
                self._temporary.cleanup()
                self._temporary = None
            self._environment.clear()
            self._client_environment.clear()
            self._authority = None
            self._runtime = None
