"""Build real niri feature videos from typed, composable Python interactions.

Use PreviewSession as the ownership boundary for a disposable desktop. Geometry
and input primitives can be combined freely; ScreenshotEditor supplies named
controls for the bundled capture profile. Recorder keeps a lossless source and
Video exports MP4, animated WebP and GIF without fabricating application states.
"""

from niri_video.editor import ScreenshotEditor
from niri_video.geometry import Easing, MotionPath, Point, Size
from niri_video.input import Keyboard, Pointer
from niri_video.recording import Recorder, Video
from niri_video.scenario import ScenarioOptions
from niri_video.session import PreviewSession, Window

__all__ = [
    "Easing",
    "Keyboard",
    "MotionPath",
    "Point",
    "Pointer",
    "PreviewSession",
    "Recorder",
    "ScenarioOptions",
    "ScreenshotEditor",
    "Size",
    "Video",
    "Window",
]
