"""Real-time, window-local pointer and keyboard gestures on an isolated display."""

from __future__ import annotations

import math
import time
from collections.abc import Generator
from contextlib import contextmanager

from .geometry import Easing, MotionPath, Point, Size
from .x11 import X11


class Pointer:
    """Schedule individual XTest events in the owned window's coordinate space."""

    def __init__(self, x11: X11, window: int, size: Size, rate: int = 120) -> None:
        if type(rate) is not int or rate <= 0:
            raise ValueError("Pointer sampling rate must be a positive integer")
        self._x11 = x11
        self._window = window
        self._size = size
        self._rate = rate

    def move_to(
        self,
        point: Point,
        duration: float = 0.6,
        easing: Easing = Easing.EASE_IN_OUT,
    ) -> None:
        """Approach a local point with selected timing and an exact endpoint."""
        _validate_seconds(duration, "Movement duration", allow_zero=True)
        self._validate_point(point)
        start = self._x11.pointer_position(self._window)
        if duration == 0.0:
            self._x11.move(self._window, point)
        elif start == point:
            deadline = time.monotonic() + duration
            self._x11.move(self._window, point)
            _wait_until(deadline)
        else:
            self._animate(MotionPath.line(start, point), duration, easing=easing)

    def click(self, point: Point | None = None, button: int = 1, dwell: float = 0.18) -> None:
        """Optionally approach a point, press a button, dwell, and always release it."""
        _validate_seconds(dwell, "Click dwell", allow_zero=True)
        _validate_button(button)
        if point is not None:
            self.move_to(point)
        with self.held(button):
            _wait_until(time.monotonic() + dwell)

    def drag(
        self,
        path: MotionPath,
        duration: float = 2.0,
        approach: float = 0.6,
        easing: Easing = Easing.LINEAR,
    ) -> None:
        """Approach smoothly before pressing, then trace every vertex with selected timing.

        Linear traversal has approximately constant arc-length speed. Frame-rate
        samples are supplemented with exact vertex deadlines, inverted through
        the selected easing, so sharp corners remain intact with either mode.
        """
        _validate_seconds(duration, "Drag duration")
        _validate_seconds(approach, "Drag approach", allow_zero=True)
        for point in path.points:
            self._validate_point(point)
        self.move_to(path.points[0], duration=approach)
        with self.held():
            self._animate(path, duration, easing=easing)

    def scroll(self, steps: int, horizontal: bool = False, interval: float = 0.1) -> None:
        """Send wheel ticks; positive steps scroll up, or right when horizontal.

        Horizontal ticks use standard X11 buttons 6/7. The nested compositor
        forwards both axes; individual clients decide whether to consume them.
        """
        if type(steps) is not int:
            raise ValueError("Scroll steps must be an integer")
        _validate_seconds(interval, "Scroll interval", allow_zero=True)
        if horizontal:
            button = 7 if steps > 0 else 6
        else:
            button = 4 if steps > 0 else 5
        start = time.monotonic()
        for index in range(abs(steps)):
            _wait_until(start + index * interval)
            self.click(button=button, dwell=0.0)

    @contextmanager
    def held(self, button: int = 1) -> Generator[None, None, None]:
        """Hold a button through a block and release it even when the block raises."""
        _validate_button(button)
        # Preserve a surrounding hold rather than releasing a state this block
        # did not acquire. X11 also retains these states for close-time cleanup.
        already_pressed = self._x11.button_pressed(button)
        try:
            if not already_pressed:
                self._x11.button(button, True)
            yield
        finally:
            if not already_pressed:
                self._x11.button(button, False)

    def _validate_point(self, point: Point) -> None:
        """Require a pixel-center position inside the declared owned-window size."""
        if (
            not 0.0 <= point.x <= self._size.width - 1
            or not 0.0 <= point.y <= self._size.height - 1
        ):
            raise ValueError(f"Pointer point {point!r} is outside the owned window {self._size!r}")

    def _animate(self, path: MotionPath, duration: float, *, easing: Easing) -> None:
        """Merge frame samples with exact corners and flush each motion at its deadline."""
        count = max(1, math.ceil(duration * self._rate))
        events: dict[float, list[Point] | None] = {
            index / count: None for index in range(count + 1)
        }
        for progress, point in zip(path.vertex_progresses, path.points, strict=True):
            fraction = _time_at_distance(progress, easing)
            vertices = events.get(fraction)
            if vertices is None:
                events[fraction] = [point]
            else:
                # Preserve every vertex even when floating-point resolution gives
                # adjacent tiny segments the same deadline.
                vertices.append(point)
        start = time.monotonic()
        for progress, vertices in sorted(events.items()):
            _wait_until(start + progress * duration)
            if vertices is None:
                self._x11.move(self._window, path.at(easing.apply(progress)))
            else:
                for point in vertices:
                    self._x11.move(self._window, point)


class Keyboard:
    """Type and hold existing-keymap keys without remapping the private X server."""

    def __init__(self, x11: X11) -> None:
        self._x11 = x11

    def press(self, key: str) -> None:
        """Press and release a named X keysym, such as ``Return`` or ``Escape``."""
        with self.held(key):
            pass

    def chord(self, *keys: str) -> None:
        """Press named keysyms in order, then release them in reverse order."""
        if not keys:
            raise ValueError("A keyboard chord needs at least one keysym")
        with self.held(*keys):
            pass

    @contextmanager
    def held(self, *keys: str) -> Generator[None, None, None]:
        """Hold named keysyms through a block, releasing acquired states in finally."""
        # Resolve the entire chord before sending any event. Aliases may resolve
        # to the same code, so hold each physical key only once.
        keycodes = tuple(dict.fromkeys(self._x11.keycode_for_keysym(key) for key in keys))
        with self._held_keycodes(keycodes):
            yield

    def type_text(self, text: str, interval: float = 0.05) -> None:
        """Type printable ASCII, tabs, and newlines with the existing map plus Shift.

        Unsupported characters or unavailable symbols are rejected before any
        partial entry. Punctuation such as a literal ``+`` is mapped as text,
        never interpreted as a chord separator or a named keysym.
        """
        _validate_seconds(interval, "Typing interval", allow_zero=True)
        strokes = self._x11.text_keycodes(text)
        shift_code = (
            self._x11.keycode_for_keysym("Shift_L") if any(shifted for _, shifted in strokes) else 0
        )
        start = time.monotonic()
        for index, (keycode, shifted) in enumerate(strokes):
            _wait_until(start + index * interval)
            modifiers = (shift_code,) if shifted else ()
            with self._held_keycodes(modifiers):
                try:
                    self._x11.keycode(keycode, True)
                finally:
                    self._x11.keycode(keycode, False)

    @contextmanager
    def _held_keycodes(self, keycodes: tuple[int, ...]) -> Generator[None, None, None]:
        """Acquire only new physical key holds and unwind them in reverse order."""
        acquired: list[int] = []
        try:
            for keycode in keycodes:
                if not self._x11.keycode_pressed(keycode):
                    acquired.append(keycode)
                    self._x11.keycode(keycode, True)
            yield
        finally:
            for keycode in reversed(acquired):
                self._x11.keycode(keycode, False)


def _validate_seconds(value: float, name: str, *, allow_zero: bool = False) -> None:
    """Reject timing values that cannot define finite monotonic deadlines."""
    if not math.isfinite(value) or value < 0.0 or (not allow_zero and value == 0.0):
        constraint = "nonnegative" if allow_zero else "positive"
        raise ValueError(f"{name} must be finite and {constraint}")


def _validate_button(button: int) -> None:
    """Prevalidate a button before an optional click approach moves the pointer."""
    if type(button) is not int or not 1 <= button <= 255:
        raise ValueError("X11 button numbers must be integers between 1 and 255")


def _time_at_distance(progress: float, easing: Easing) -> float:
    """Invert a monotone easing curve to schedule an original path vertex exactly."""
    if easing is Easing.LINEAR or progress == 0.0 or progress == 1.0:
        return progress
    lower = 0.0
    upper = 1.0
    # Bisection resolves the quintic inverse to floating-point precision without
    # introducing a dependency or assuming a polynomial has an analytic inverse.
    for _ in range(56):
        middle = (lower + upper) / 2.0
        if easing.apply(middle) < progress:
            lower = middle
        else:
            upper = middle
    return (lower + upper) / 2.0


def _wait_until(deadline: float) -> None:
    """Sleep toward one absolute monotonic deadline without accumulating drift."""
    while (remaining := deadline - time.monotonic()) > 0.0:
        time.sleep(remaining)
