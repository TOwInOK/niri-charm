"""Authenticated, explicitly addressed Xlib and XTest access to a private display."""

from __future__ import annotations

import ctypes
import ctypes.util
import math
import threading
from collections.abc import Callable
from functools import cache
from typing import Protocol, cast

from .geometry import Point, Size


class _Display(ctypes.Structure):
    """Opaque native Display; never reinterpret an Xlib connection as an integer."""


class _ImageFunctions(ctypes.Structure):
    """Native XImage function table, retained to describe the complete layout."""

    _fields_ = [
        ("create_image", ctypes.c_void_p),
        ("destroy_image", ctypes.c_void_p),
        ("get_pixel", ctypes.c_void_p),
        ("put_pixel", ctypes.c_void_p),
        ("sub_image", ctypes.c_void_p),
        ("add_pixel", ctypes.c_void_p),
    ]


class _XImage(ctypes.Structure):
    """Native XImage layout, including its visual's RGB component masks."""

    _fields_ = [
        ("width", ctypes.c_int),
        ("height", ctypes.c_int),
        ("xoffset", ctypes.c_int),
        ("format", ctypes.c_int),
        ("data", ctypes.c_void_p),
        ("byte_order", ctypes.c_int),
        ("bitmap_unit", ctypes.c_int),
        ("bitmap_bit_order", ctypes.c_int),
        ("bitmap_pad", ctypes.c_int),
        ("depth", ctypes.c_int),
        ("bytes_per_line", ctypes.c_int),
        ("bits_per_pixel", ctypes.c_int),
        ("red_mask", ctypes.c_ulong),
        ("green_mask", ctypes.c_ulong),
        ("blue_mask", ctypes.c_ulong),
        ("obdata", ctypes.c_void_p),
        ("functions", _ImageFunctions),
    ]
    red_mask: int
    green_mask: int
    blue_mask: int


# Native pointers expose precise contents to Python, while C signatures below
# use the actual generated POINTER types rather than untyped void pointers.
class _DisplayPointer(Protocol):
    """A structural view of an opaque Display pointer."""

    @property
    def contents(self) -> _Display:
        """Return the opaque native display structure."""
        ...


class _ImagePointer(Protocol):
    """A structural view of an XImage pointer."""

    @property
    def contents(self) -> _XImage:
        """Return the correctly laid out native image header."""
        ...


class _IntPointer(Protocol):
    """A structural view of a native signed-integer pointer."""

    @property
    def contents(self) -> ctypes.c_int:
        """Return a signed native integer."""
        ...


class _UIntPointer(Protocol):
    """A structural view of a native unsigned-integer pointer."""

    @property
    def contents(self) -> ctypes.c_uint:
        """Return an unsigned native integer."""
        ...


class _ULongPointer(Protocol):
    """A structural view of a scalar unsigned-long output pointer."""

    @property
    def contents(self) -> ctypes.c_ulong:
        """Return the native XID, keysym, or mask value."""
        ...


class _WindowPointer(_ULongPointer, Protocol):
    """An unsigned-long array whose ctypes elements are unboxed Python integers."""

    def __getitem__(self, index: int) -> int: ...


class _WindowListPointer(Protocol):
    """A structural view of an output pointer to a window array."""

    @property
    def contents(self) -> _ULongPointer:
        """Return the array pointer allocated by XQueryTree."""
        ...


_CDisplayPointer = ctypes.POINTER(_Display)
_CImagePointer = ctypes.POINTER(_XImage)
_CIntPointer = ctypes.POINTER(ctypes.c_int)
_CUIntPointer = ctypes.POINTER(ctypes.c_uint)
_CWindowPointer = ctypes.POINTER(ctypes.c_ulong)
_CWindowListPointer = ctypes.POINTER(_CWindowPointer)


_AUTHORIZATION_LOCK = threading.Lock()


class _Native:
    """Precisely signed native functions, with matching Python callable types."""

    def __init__(self) -> None:
        xlib_name = ctypes.util.find_library("X11")
        xtst_name = ctypes.util.find_library("Xtst")
        if xlib_name is None or xtst_name is None:
            raise RuntimeError("X11 input requires the libX11 and libXtst shared libraries")
        xlib = ctypes.CDLL(xlib_name)
        xtst = ctypes.CDLL(xtst_name)
        self._libraries = (xlib, xtst)

        xlib.XSetAuthorization.argtypes = [
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
        ]
        xlib.XSetAuthorization.restype = None
        self.set_authorization: Callable[[bytes, int, bytes, int], None] = cast(
            Callable[[bytes, int, bytes, int], None], xlib.XSetAuthorization
        )

        xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
        xlib.XOpenDisplay.restype = _CDisplayPointer
        self.open_display: Callable[[bytes], _DisplayPointer] = cast(
            Callable[[bytes], _DisplayPointer], xlib.XOpenDisplay
        )

        xlib.XDefaultRootWindow.argtypes = [_CDisplayPointer]
        xlib.XDefaultRootWindow.restype = ctypes.c_ulong
        self.default_root: Callable[[_DisplayPointer], int] = cast(
            Callable[[_DisplayPointer], int], xlib.XDefaultRootWindow
        )

        xlib.XDefaultScreen.argtypes = [_CDisplayPointer]
        xlib.XDefaultScreen.restype = ctypes.c_int
        self.default_screen: Callable[[_DisplayPointer], int] = cast(
            Callable[[_DisplayPointer], int], xlib.XDefaultScreen
        )

        xlib.XQueryTree.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            _CWindowPointer,
            _CWindowPointer,
            _CWindowListPointer,
            _CUIntPointer,
        ]
        xlib.XQueryTree.restype = ctypes.c_int
        self.query_tree: Callable[
            [_DisplayPointer, int, _ULongPointer, _ULongPointer, _WindowListPointer, _UIntPointer],
            int,
        ] = cast(
            Callable[
                [
                    _DisplayPointer,
                    int,
                    _ULongPointer,
                    _ULongPointer,
                    _WindowListPointer,
                    _UIntPointer,
                ],
                int,
            ],
            xlib.XQueryTree,
        )

        xlib.XGetGeometry.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            _CWindowPointer,
            _CIntPointer,
            _CIntPointer,
            _CUIntPointer,
            _CUIntPointer,
            _CUIntPointer,
            _CUIntPointer,
        ]
        xlib.XGetGeometry.restype = ctypes.c_int
        self.get_geometry: Callable[
            [
                _DisplayPointer,
                int,
                _ULongPointer,
                _IntPointer,
                _IntPointer,
                _UIntPointer,
                _UIntPointer,
                _UIntPointer,
                _UIntPointer,
            ],
            int,
        ] = cast(
            Callable[
                [
                    _DisplayPointer,
                    int,
                    _ULongPointer,
                    _IntPointer,
                    _IntPointer,
                    _UIntPointer,
                    _UIntPointer,
                    _UIntPointer,
                    _UIntPointer,
                ],
                int,
            ],
            xlib.XGetGeometry,
        )

        xlib.XTranslateCoordinates.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_int,
            _CIntPointer,
            _CIntPointer,
            _CWindowPointer,
        ]
        xlib.XTranslateCoordinates.restype = ctypes.c_int
        self.translate_coordinates: Callable[
            [_DisplayPointer, int, int, int, int, _IntPointer, _IntPointer, _ULongPointer], int
        ] = cast(
            Callable[
                [_DisplayPointer, int, int, int, int, _IntPointer, _IntPointer, _ULongPointer], int
            ],
            xlib.XTranslateCoordinates,
        )

        xlib.XMoveResizeWindow.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint,
            ctypes.c_uint,
        ]
        xlib.XMoveResizeWindow.restype = ctypes.c_int
        self.move_resize: Callable[[_DisplayPointer, int, int, int, int, int], int] = cast(
            Callable[[_DisplayPointer, int, int, int, int, int], int], xlib.XMoveResizeWindow
        )

        xlib.XRaiseWindow.argtypes = [_CDisplayPointer, ctypes.c_ulong]
        xlib.XRaiseWindow.restype = ctypes.c_int
        self.raise_window: Callable[[_DisplayPointer, int], int] = cast(
            Callable[[_DisplayPointer, int], int], xlib.XRaiseWindow
        )

        xlib.XSetInputFocus.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        xlib.XSetInputFocus.restype = ctypes.c_int
        self.set_input_focus: Callable[[_DisplayPointer, int, int, int], int] = cast(
            Callable[[_DisplayPointer, int, int, int], int], xlib.XSetInputFocus
        )

        xlib.XQueryPointer.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            _CWindowPointer,
            _CWindowPointer,
            _CIntPointer,
            _CIntPointer,
            _CIntPointer,
            _CIntPointer,
            _CUIntPointer,
        ]
        xlib.XQueryPointer.restype = ctypes.c_int
        self.query_pointer: Callable[
            [
                _DisplayPointer,
                int,
                _ULongPointer,
                _ULongPointer,
                _IntPointer,
                _IntPointer,
                _IntPointer,
                _IntPointer,
                _UIntPointer,
            ],
            int,
        ] = cast(
            Callable[
                [
                    _DisplayPointer,
                    int,
                    _ULongPointer,
                    _ULongPointer,
                    _IntPointer,
                    _IntPointer,
                    _IntPointer,
                    _IntPointer,
                    _UIntPointer,
                ],
                int,
            ],
            xlib.XQueryPointer,
        )

        xlib.XStringToKeysym.argtypes = [ctypes.c_char_p]
        xlib.XStringToKeysym.restype = ctypes.c_ulong
        self.string_to_keysym: Callable[[bytes], int] = cast(
            Callable[[bytes], int], xlib.XStringToKeysym
        )

        xlib.XKeysymToKeycode.argtypes = [_CDisplayPointer, ctypes.c_ulong]
        xlib.XKeysymToKeycode.restype = ctypes.c_ubyte
        self.keysym_to_keycode: Callable[[_DisplayPointer, int], int] = cast(
            Callable[[_DisplayPointer, int], int], xlib.XKeysymToKeycode
        )

        xlib.XDisplayKeycodes.argtypes = [_CDisplayPointer, _CIntPointer, _CIntPointer]
        xlib.XDisplayKeycodes.restype = ctypes.c_int
        self.display_keycodes: Callable[[_DisplayPointer, _IntPointer, _IntPointer], int] = cast(
            Callable[[_DisplayPointer, _IntPointer, _IntPointer], int], xlib.XDisplayKeycodes
        )

        xlib.XGetKeyboardMapping.argtypes = [
            _CDisplayPointer,
            ctypes.c_ubyte,
            ctypes.c_int,
            _CIntPointer,
        ]
        xlib.XGetKeyboardMapping.restype = _CWindowPointer
        self.keyboard_mapping: Callable[
            [_DisplayPointer, int, int, _IntPointer], _WindowPointer
        ] = cast(
            Callable[[_DisplayPointer, int, int, _IntPointer], _WindowPointer],
            xlib.XGetKeyboardMapping,
        )

        xlib.XConvertCase.argtypes = [ctypes.c_ulong, _CWindowPointer, _CWindowPointer]
        xlib.XConvertCase.restype = None
        self.convert_case: Callable[[int, _ULongPointer, _ULongPointer], None] = cast(
            Callable[[int, _ULongPointer, _ULongPointer], None], xlib.XConvertCase
        )

        xlib.XGetImage.argtypes = [
            _CDisplayPointer,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint,
            ctypes.c_uint,
            ctypes.c_ulong,
            ctypes.c_int,
        ]
        xlib.XGetImage.restype = _CImagePointer
        self.get_image: Callable[
            [_DisplayPointer, int, int, int, int, int, int, int],
            _ImagePointer,
        ] = cast(
            Callable[
                [_DisplayPointer, int, int, int, int, int, int, int],
                _ImagePointer,
            ],
            xlib.XGetImage,
        )

        xlib.XGetPixel.argtypes = [_CImagePointer, ctypes.c_int, ctypes.c_int]
        xlib.XGetPixel.restype = ctypes.c_ulong
        self.get_pixel: Callable[[_ImagePointer, int, int], int] = cast(
            Callable[[_ImagePointer, int, int], int], xlib.XGetPixel
        )

        xlib.XDestroyImage.argtypes = [_CImagePointer]
        xlib.XDestroyImage.restype = ctypes.c_int
        self.destroy_image: Callable[[_ImagePointer], int] = cast(
            Callable[[_ImagePointer], int], xlib.XDestroyImage
        )

        xlib.XFree.argtypes = [ctypes.c_void_p]
        xlib.XFree.restype = ctypes.c_int
        self.free: Callable[[_ULongPointer], int] = cast(Callable[[_ULongPointer], int], xlib.XFree)

        xlib.XSync.argtypes = [_CDisplayPointer, ctypes.c_int]
        xlib.XSync.restype = ctypes.c_int
        self.sync: Callable[[_DisplayPointer, int], int] = cast(
            Callable[[_DisplayPointer, int], int], xlib.XSync
        )

        xlib.XCloseDisplay.argtypes = [_CDisplayPointer]
        xlib.XCloseDisplay.restype = ctypes.c_int
        self.close_display: Callable[[_DisplayPointer], int] = cast(
            Callable[[_DisplayPointer], int], xlib.XCloseDisplay
        )

        xtst.XTestQueryExtension.argtypes = [
            _CDisplayPointer,
            _CIntPointer,
            _CIntPointer,
            _CIntPointer,
            _CIntPointer,
        ]
        xtst.XTestQueryExtension.restype = ctypes.c_int
        self.query_xtest: Callable[
            [_DisplayPointer, _IntPointer, _IntPointer, _IntPointer, _IntPointer],
            int,
        ] = cast(
            Callable[
                [_DisplayPointer, _IntPointer, _IntPointer, _IntPointer, _IntPointer],
                int,
            ],
            xtst.XTestQueryExtension,
        )

        xtst.XTestFakeMotionEvent.argtypes = [
            _CDisplayPointer,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        xtst.XTestFakeMotionEvent.restype = ctypes.c_int
        self.motion: Callable[[_DisplayPointer, int, int, int, int], int] = cast(
            Callable[[_DisplayPointer, int, int, int, int], int], xtst.XTestFakeMotionEvent
        )

        xtst.XTestFakeButtonEvent.argtypes = [
            _CDisplayPointer,
            ctypes.c_uint,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        xtst.XTestFakeButtonEvent.restype = ctypes.c_int
        self.button: Callable[[_DisplayPointer, int, int, int], int] = cast(
            Callable[[_DisplayPointer, int, int, int], int], xtst.XTestFakeButtonEvent
        )

        xtst.XTestFakeKeyEvent.argtypes = [
            _CDisplayPointer,
            ctypes.c_uint,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        xtst.XTestFakeKeyEvent.restype = ctypes.c_int
        self.key: Callable[[_DisplayPointer, int, int, int], int] = cast(
            Callable[[_DisplayPointer, int, int, int], int], xtst.XTestFakeKeyEvent
        )


@cache
def _native() -> _Native:
    """Load the libraries once without opening or selecting any display."""
    return _Native()


class X11:
    """Own an authenticated connection to an explicitly named private X display.

    No default display, environment variable, or host authentication file is
    consulted. Call ``close`` before stopping the private server. Operations on
    one connection must be serialized by its caller.
    """

    def __init__(self, display: str, cookie: bytes) -> None:
        if not display or "\x00" in display:
            raise ValueError("An explicit, nonempty X display name is required")
        if len(cookie) != 16:
            raise ValueError("MIT-MAGIC-COOKIE-1 authorization requires a 16-byte cookie")
        self._native = _native()
        self._display: _DisplayPointer | None = None
        self._pressed_keys: set[int] = set()
        self._pressed_buttons: set[int] = set()
        authorization = b"MIT-MAGIC-COOKIE-1"
        # XSetAuthorization affects process-global Xlib state. Keep selection and
        # opening indivisible so a second private connection cannot swap cookies.
        with _AUTHORIZATION_LOCK:
            self._native.set_authorization(authorization, len(authorization), cookie, len(cookie))
            connection = self._native.open_display(display.encode("utf-8"))
        if not connection:
            raise RuntimeError(f"Could not authenticate to the private X display {display!r}")
        self._display = connection
        self._root = self._native.default_root(connection)
        self._screen = self._native.default_screen(connection)
        event_base = ctypes.c_int()
        error_base = ctypes.c_int()
        major = ctypes.c_int()
        minor = ctypes.c_int()
        if not self._native.query_xtest(
            connection,
            ctypes.pointer(event_base),
            ctypes.pointer(error_base),
            ctypes.pointer(major),
            ctypes.pointer(minor),
        ):
            self.close()
            raise RuntimeError(f"The private X display {display!r} does not provide XTest")

    @property
    def root(self) -> int:
        """Return the root window of this connection's explicitly selected screen."""
        self._connection()
        return self._root

    def children(self, window: int | None = None) -> tuple[int, ...]:
        """Return direct child windows in bottom-to-top stacking order."""
        connection = self._connection()
        target = self.root if window is None else window
        _validate_window(target)
        root = ctypes.c_ulong()
        parent = ctypes.c_ulong()
        children = _CWindowPointer()
        count = ctypes.c_uint()
        if not self._native.query_tree(
            connection,
            target,
            ctypes.pointer(root),
            ctypes.pointer(parent),
            ctypes.pointer(children),
            ctypes.pointer(count),
        ):
            raise RuntimeError(f"Could not query children of X window {target}")
        try:
            values = cast(_WindowPointer, children)
            return tuple(values[index] for index in range(count.value))
        finally:
            if children:
                self._native.free(children)

    def geometry(self, window: int) -> tuple[int, int, int, int]:
        """Return client-origin root-relative x, y, width, and height."""
        connection = self._connection()
        _validate_window(window)
        root = ctypes.c_ulong()
        parent_x = ctypes.c_int()
        parent_y = ctypes.c_int()
        width = ctypes.c_uint()
        height = ctypes.c_uint()
        border = ctypes.c_uint()
        depth = ctypes.c_uint()
        if not self._native.get_geometry(
            connection,
            window,
            ctypes.pointer(root),
            ctypes.pointer(parent_x),
            ctypes.pointer(parent_y),
            ctypes.pointer(width),
            ctypes.pointer(height),
            ctypes.pointer(border),
            ctypes.pointer(depth),
        ):
            raise RuntimeError(f"Could not read geometry of X window {window}")
        x = ctypes.c_int()
        y = ctypes.c_int()
        child = ctypes.c_ulong()
        if not self._native.translate_coordinates(
            connection,
            window,
            self.root,
            0,
            0,
            ctypes.pointer(x),
            ctypes.pointer(y),
            ctypes.pointer(child),
        ):
            raise RuntimeError(f"X window {window} is not on the selected root screen")
        return x.value, y.value, width.value, height.value

    def resize(self, window: int, size: Size) -> None:
        """Move the owned window to root origin and set its client dimensions."""
        _validate_window(window)
        if size.width > 65535 or size.height > 65535:
            raise ValueError("X11 window dimensions must fit the protocol's 16-bit size fields")
        connection = self._connection()
        self._request(
            self._native.move_resize(connection, window, 0, 0, size.width, size.height),
            "move and resize the owned window",
        )

    def focus(self, window: int) -> None:
        """Raise the owned window and deliver native focus events to its client."""
        _validate_window(window)
        connection = self._connection()
        self._request(self._native.raise_window(connection, window), "raise the owned window")
        # RevertToParent=2 and CurrentTime=0 generate real FocusIn/FocusOut events,
        # including the Winit events needed by the nested compositor.
        self._request(
            self._native.set_input_focus(connection, window, 2, 0), "focus the owned window"
        )

    def pointer_position(self, window: int) -> Point:
        """Read the current pointer position relative to the owned window."""
        connection = self._connection()
        _validate_window(window)
        root = ctypes.c_ulong()
        child = ctypes.c_ulong()
        root_x = ctypes.c_int()
        root_y = ctypes.c_int()
        x = ctypes.c_int()
        y = ctypes.c_int()
        mask = ctypes.c_uint()
        if not self._native.query_pointer(
            connection,
            window,
            ctypes.pointer(root),
            ctypes.pointer(child),
            ctypes.pointer(root_x),
            ctypes.pointer(root_y),
            ctypes.pointer(x),
            ctypes.pointer(y),
            ctypes.pointer(mask),
        ):
            raise RuntimeError(f"Could not query the pointer on X window {window}")
        return Point(float(x.value), float(y.value))

    def move(self, window: int, point: Point) -> None:
        """Send one real, immediately flushed motion in window-local coordinates."""
        x, y, _, _ = self.geometry(window)
        absolute_x = x + round(point.x)
        absolute_y = y + round(point.y)
        if not all(-(2**31) <= coordinate < 2**31 for coordinate in (absolute_x, absolute_y)):
            raise ValueError("Pointer coordinates must fit signed 32-bit XTest coordinates")
        self._request(
            self._native.motion(self._connection(), self._screen, absolute_x, absolute_y, 0),
            "move the pointer",
        )

    def button(self, button: int, pressed: bool) -> None:
        """Set an XTest button state and track it for guaranteed close-time release."""
        if type(button) is not int or not 1 <= button <= 255:
            raise ValueError("X11 button numbers must be integers between 1 and 255")
        connection = self._connection()
        status = self._native.button(connection, button, int(pressed), 0)
        if status:
            if pressed:
                self._pressed_buttons.add(button)
            else:
                self._pressed_buttons.discard(button)
        self._request(status, f"{'press' if pressed else 'release'} button {button}")

    def button_pressed(self, button: int) -> bool:
        """Return whether this connection currently owns a hold on a mouse button."""
        self._connection()
        return button in self._pressed_buttons

    def key(self, keysym: str, pressed: bool) -> None:
        """Set a named X keysym's state using the existing server keymap.

        Names follow Xlib conventions, for example ``Return``, ``Shift_L``,
        ``Super_L``, or ``plus``. Literal text belongs in ``Keyboard.type_text``.
        """
        self.keycode(self.keycode_for_keysym(keysym), pressed)

    def pixel(self, window: int, point: Point) -> tuple[int, int, int]:
        """Read one owned-window pixel, flooring coordinates and decoding RGB masks."""
        connection = self._connection()
        _, _, width, height = self.geometry(window)
        if not 0.0 <= point.x < width or not 0.0 <= point.y < height:
            raise ValueError("Pixel coordinates must be inside the owned window")
        all_planes = ctypes.c_ulong(-1).value
        # ZPixmap=2 preserves the visual masks; XGetPixel handles image byte order
        # and pixel packing instead of assuming an RGB byte layout.
        image = self._native.get_image(
            connection,
            window,
            math.floor(point.x),
            math.floor(point.y),
            1,
            1,
            all_planes,
            2,
        )
        if not image:
            raise RuntimeError(f"Could not capture a pixel from X window {window}")
        try:
            value = self._native.get_pixel(image, 0, 0)
            masks = image.contents
            return (
                _rgb_component(value, masks.red_mask),
                _rgb_component(value, masks.green_mask),
                _rgb_component(value, masks.blue_mask),
            )
        finally:
            self._native.destroy_image(image)

    def close(self) -> None:
        """Release every tracked key and button before closing; repeated calls are safe."""
        connection = self._display
        if connection is None:
            return
        failed = False
        try:
            for keycode in sorted(self._pressed_keys, reverse=True):
                if not self._native.key(connection, keycode, 0, 0):
                    failed = True
            for button in sorted(self._pressed_buttons, reverse=True):
                if not self._native.button(connection, button, 0, 0):
                    failed = True
            self._native.sync(connection, 0)
        finally:
            self._native.close_display(connection)
            self._display = None
            self._pressed_keys.clear()
            self._pressed_buttons.clear()
        if failed:
            raise RuntimeError(
                "XTest failed to release an input state while closing the private display"
            )

    def _connection(self) -> _DisplayPointer:
        """Require a live native Display pointer without falling back to host defaults."""
        if self._display is None:
            raise RuntimeError("The private X11 connection is closed")
        return self._display

    def _request(self, status: int, operation: str) -> None:
        """Check and synchronize one request so motion is never deferred in a batch."""
        if not status:
            raise RuntimeError(f"XTest/Xlib could not {operation}")
        self._native.sync(self._connection(), 0)

    def keycode_for_keysym(self, keysym: str) -> int:
        """Resolve a named symbol without changing keyboard state."""
        if not keysym or "\x00" in keysym:
            raise ValueError("A nonempty X keysym name is required")
        try:
            encoded = keysym.encode("ascii")
        except UnicodeEncodeError as error:
            raise ValueError(f"X keysym names must be ASCII: {keysym!r}") from error
        symbol = self._native.string_to_keysym(encoded)
        if not symbol:
            raise ValueError(f"Unknown X keysym: {keysym!r}")
        keycode = self._native.keysym_to_keycode(self._connection(), symbol)
        if not keycode:
            raise ValueError(f"X keysym {keysym!r} is absent from the private display's keymap")
        return keycode

    def keycode(self, keycode: int, pressed: bool) -> None:
        """Send an already resolved keycode, retaining exactly the pressed code."""
        connection = self._connection()
        if not 1 <= keycode <= 255:
            raise ValueError("X11 keycodes must be between 1 and 255")
        status = self._native.key(connection, keycode, int(pressed), 0)
        if status:
            if pressed:
                self._pressed_keys.add(keycode)
            else:
                self._pressed_keys.discard(keycode)
        self._request(status, f"{'press' if pressed else 'release'} keycode {keycode}")

    def keycode_pressed(self, keycode: int) -> bool:
        """Return whether this connection currently owns a hold on a physical key."""
        self._connection()
        return keycode in self._pressed_keys

    def text_keycodes(self, text: str) -> tuple[tuple[int, bool], ...]:
        """Prevalidate ASCII text against the existing map's unshifted/Shift levels."""
        symbols: list[int] = []
        controls = {"\n": 0xFF0D, "\r": 0xFF0D, "\t": 0xFF09}
        for character in text:
            if character in controls:
                symbols.append(controls[character])
            elif 0x20 <= ord(character) <= 0x7E:
                symbols.append(ord(character))
            else:
                raise ValueError(
                    f"Unsupported text character: {character!r}; "
                    "use printable ASCII, tab, or newline"
                )
        if not symbols:
            return ()

        connection = self._connection()
        minimum = ctypes.c_int()
        maximum = ctypes.c_int()
        self._native.display_keycodes(connection, ctypes.pointer(minimum), ctypes.pointer(maximum))
        count = maximum.value - minimum.value + 1
        if count <= 0:
            raise RuntimeError("The private display returned an empty keyboard map")
        columns = ctypes.c_int()
        mapping = self._native.keyboard_mapping(
            connection, minimum.value, count, ctypes.pointer(columns)
        )
        if not mapping:
            raise RuntimeError("Could not read the private display's existing keyboard map")
        unshifted: dict[int, tuple[int, bool]] = {}
        shifted: dict[int, tuple[int, bool]] = {}
        try:
            if columns.value <= 0:
                raise RuntimeError(
                    "The private display returned a keyboard map without symbol levels"
                )
            for offset in range(count):
                index = offset * columns.value
                base = mapping[index]
                upper = mapping[index + 1] if columns.value > 1 else 0
                if upper == 0:
                    lower_case = ctypes.c_ulong()
                    upper_case = ctypes.c_ulong()
                    self._native.convert_case(
                        base, ctypes.pointer(lower_case), ctypes.pointer(upper_case)
                    )
                    base = lower_case.value
                    upper = upper_case.value
                keycode = minimum.value + offset
                if base:
                    unshifted.setdefault(base, (keycode, False))
                if upper:
                    shifted.setdefault(upper, (keycode, True))
        finally:
            self._native.free(mapping)

        strokes: list[tuple[int, bool]] = []
        for character, symbol in zip(text, symbols, strict=True):
            stroke = unshifted.get(symbol) or shifted.get(symbol)
            if stroke is None:
                raise ValueError(
                    f"Text character {character!r} requires more than "
                    "the existing keymap plus Shift"
                )
            strokes.append(stroke)
        return tuple(strokes)


def _validate_window(window: int) -> None:
    """Reject values that cannot be X protocol window identifiers."""
    if type(window) is not int or not 1 <= window <= 0xFFFFFFFF:
        raise ValueError("An X11 window identifier must be an integer between 1 and 2**32-1")


def _rgb_component(pixel: int, mask: int) -> int:
    """Normalize one contiguous TrueColor component to an eight-bit channel."""
    if mask == 0:
        raise RuntimeError("Pixel capture requires an RGB TrueColor visual")
    shift = (mask & -mask).bit_length() - 1
    maximum = mask >> shift
    component = (pixel & mask) >> shift
    return (component * 255 + maximum // 2) // maximum
