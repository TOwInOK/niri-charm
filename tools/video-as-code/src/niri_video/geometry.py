"""Immutable geometry and arc-length paths for reproducible pointer movement."""

from __future__ import annotations

import math
from bisect import bisect_left
from dataclasses import dataclass, field
from enum import StrEnum
from typing import Self


class Easing(StrEnum):
    """Select linear traversal or a smooth quintic ease-in/ease-out timing curve."""

    LINEAR = "linear"
    EASE_IN_OUT = "ease-in-out"

    def apply(self, progress: float) -> float:
        """Map a finite time fraction in [0, 1] to a monotone distance fraction."""
        if not math.isfinite(progress) or not 0.0 <= progress <= 1.0:
            raise ValueError("Easing progress must be finite and between zero and one")
        if self is Easing.LINEAR:
            return progress
        # Evaluate the second half by symmetry to keep rounding inside [0, 1]
        # and preserve the flat endpoint without cancellation near completion.
        fraction = progress if progress <= 0.5 else 1.0 - progress
        eased = fraction * fraction * fraction * (10.0 + fraction * (-15.0 + 6.0 * fraction))
        return eased if progress <= 0.5 else 1.0 - eased


@dataclass(frozen=True, slots=True)
class Point:
    """A finite position in window-local pixel coordinates."""

    x: float
    y: float

    def __post_init__(self) -> None:
        """Reject coordinates that cannot describe a real position."""
        if not math.isfinite(self.x) or not math.isfinite(self.y):
            raise ValueError("Point coordinates must be finite")


@dataclass(frozen=True, slots=True)
class Size:
    """Positive integer dimensions of a window or recording surface."""

    width: int
    height: int

    def __post_init__(self) -> None:
        """Reject fractional, boolean, or nonpositive dimensions."""
        for dimension in (self.width, self.height):
            if type(dimension) is not int or dimension <= 0:
                raise ValueError("Size dimensions must be positive integers")


@dataclass(frozen=True, slots=True)
class MotionPath:
    """A polyline traversed according to cumulative distance, not vertex count."""

    points: tuple[Point, ...]
    _progresses: tuple[float, ...] = field(init=False, repr=False, compare=False)

    def __post_init__(self) -> None:
        """Compute normalized vertex distances and reject degenerate paths."""
        if type(self.points) is not tuple or len(self.points) < 2:
            raise ValueError("A motion path needs a tuple containing at least two points")
        if any(type(point) is not Point for point in self.points):
            raise ValueError("A motion path may contain only Point values")

        distances = [0.0]
        for start, end in zip(self.points, self.points[1:], strict=False):
            segment = math.hypot(end.x - start.x, end.y - start.y)
            distance = distances[-1] + segment
            if not math.isfinite(distance):
                raise ValueError("Path lengths must be finite")
            distances.append(distance)
        length = distances[-1]
        if length <= 0.0:
            raise ValueError("A motion path must have positive length")
        object.__setattr__(self, "_progresses", tuple(distance / length for distance in distances))

    @property
    def vertex_progresses(self) -> tuple[float, ...]:
        """Return each original vertex's fraction of the total path distance."""
        return self._progresses

    def at(self, progress: float) -> Point:
        """Interpolate at an arc-length fraction between zero and one, inclusive.

        Endpoints and exact vertex fractions return the original points. Repeated
        consecutive vertices are allowed, but a wholly stationary path is not.
        """
        if not math.isfinite(progress) or not 0.0 <= progress <= 1.0:
            raise ValueError("Path progress must be finite and between zero and one")
        if progress == 0.0:
            return self.points[0]
        if progress == 1.0:
            return self.points[-1]

        index = bisect_left(self._progresses, progress)
        if progress == self._progresses[index]:
            return self.points[index]
        start_progress = self._progresses[index - 1]
        end_progress = self._progresses[index]
        fraction = (progress - start_progress) / (end_progress - start_progress)
        return _interpolate(self.points[index - 1], self.points[index], fraction)

    @classmethod
    def line(cls, start: Point, end: Point) -> Self:
        """Build a straight path with exact start and end positions."""
        return cls((start, end))

    @classmethod
    def bezier(
        cls,
        start: Point,
        c1: Point,
        c2: Point,
        end: Point,
        samples: int = 120,
    ) -> Self:
        """Sample a cubic Bezier into a polyline, including both endpoints.

        ``samples`` is the total number of sampled points and must be at least
        two. Traversal follows the sampled curve's arc length.
        """
        if type(samples) is not int or samples < 2:
            raise ValueError("A Bezier path needs at least two integer samples")
        points = [start]
        for index in range(1, samples - 1):
            progress = index / (samples - 1)
            # De Casteljau interpolation avoids the large intermediate differences
            # that a polynomial expansion can create for distant control points.
            a = _interpolate(start, c1, progress)
            b = _interpolate(c1, c2, progress)
            c = _interpolate(c2, end, progress)
            points.append(
                _interpolate(_interpolate(a, b, progress), _interpolate(b, c, progress), progress)
            )
        points.append(end)
        return cls(tuple(points))

    @classmethod
    def star(cls, center: Point, radius: float) -> Self:
        """Build a closed, continuous five-point pentagram starting at its top."""
        if not math.isfinite(radius) or radius <= 0.0:
            raise ValueError("A star radius must be finite and positive")
        vertices = tuple(
            Point(
                center.x + radius * math.cos(-math.pi / 2.0 + index * 4.0 * math.pi / 5.0),
                center.y + radius * math.sin(-math.pi / 2.0 + index * 4.0 * math.pi / 5.0),
            )
            for index in range(5)
        )
        return cls((*vertices, vertices[0]))


def _interpolate(start: Point, end: Point, progress: float) -> Point:
    """Compute a weighted position without subtracting huge coordinates."""
    remaining = 1.0 - progress
    return Point(remaining * start.x + progress * end.x, remaining * start.y + progress * end.y)
