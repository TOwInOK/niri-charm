"""Behavioral tests for finite geometry and distance-based motion paths."""

from __future__ import annotations

import math
import unittest
from dataclasses import FrozenInstanceError

from niri_video.geometry import Easing, MotionPath, Point, Size


class GeometryTests(unittest.TestCase):
    """Cover geometry edges without replacing real input wiring with mocks."""

    def test_easing_modes_have_exact_endpoints_and_monotone_traversal(self) -> None:
        """Keep both timing modes bounded, monotone, and exact at either endpoint."""
        self.assertEqual(Easing.LINEAR.value, "linear")
        self.assertEqual(Easing.EASE_IN_OUT.value, "ease-in-out")
        for easing in Easing:
            with self.subTest(easing=easing):
                self.assertEqual(easing.apply(0.0), 0.0)
                self.assertEqual(easing.apply(1.0), 1.0)
                self.assertEqual(easing.apply(0.5), 0.5)
                values = [easing.apply(index / 100.0) for index in range(101)]
                self.assertTrue(all(0.0 <= value <= 1.0 for value in values))
                self.assertTrue(
                    all(left < right for left, right in zip(values, values[1:], strict=False))
                )

    def test_easing_is_symmetric_and_changes_distance_timing(self) -> None:
        """Move less distance early and more distance later without changing symmetry."""
        for progress in (0.1, 0.2, 0.3, 0.4):
            self.assertEqual(Easing.LINEAR.apply(progress), progress)
            eased = Easing.EASE_IN_OUT.apply(progress)
            self.assertLess(eased, progress)
            self.assertGreater(Easing.EASE_IN_OUT.apply(1.0 - progress), 1.0 - progress)
            self.assertAlmostEqual(eased + Easing.EASE_IN_OUT.apply(1.0 - progress), 1.0)

    def test_easing_rejects_invalid_progress(self) -> None:
        """Reject extrapolation and nonfinite timing rather than leaking invalid positions."""
        for easing in Easing:
            for progress in (-0.1, 1.1, math.nan, math.inf, -math.inf):
                with self.subTest(easing=easing, progress=progress), self.assertRaises(ValueError):
                    easing.apply(progress)

    def test_arc_length_traversal_preserves_unequal_segment_corner(self) -> None:
        """Spend time on segments in proportion to their physical lengths."""
        corner = Point(3.0, 0.0)
        path = MotionPath((Point(0.0, 0.0), corner, Point(3.0, 9.0)))
        self.assertEqual(path.at(0.125), Point(1.5, 0.0))
        self.assertIs(path.at(0.25), corner)
        self.assertEqual(path.at(0.5), Point(3.0, 3.0))
        self.assertEqual(path.at(0.875), Point(3.0, 7.5))

    def test_endpoints_are_exact_and_duplicate_vertices_are_safe(self) -> None:
        """Return original endpoints and skip stationary intermediate segments."""
        start = Point(0.0, 0.0)
        end = Point(8.0, 0.0)
        path = MotionPath((start, start, Point(4.0, 0.0), Point(4.0, 0.0), end, end))
        self.assertIs(path.at(0.0), start)
        self.assertIs(path.at(1.0), end)
        self.assertEqual(path.at(0.5), Point(4.0, 0.0))
        self.assertEqual(path.at(0.75), Point(6.0, 0.0))
        self.assertEqual(MotionPath.line(start, end).at(0.25), Point(2.0, 0.0))

    def test_star_is_closed_and_connects_alternating_outer_vertices(self) -> None:
        """Draw five equal pentagram edges rather than a perimeter pentagon."""
        center = Point(20.0, 30.0)
        radius = 10.0
        path = MotionPath.star(center, radius)
        self.assertEqual(len(path.points), 6)
        self.assertIs(path.points[0], path.points[-1])
        self.assertAlmostEqual(path.points[0].x, center.x)
        self.assertAlmostEqual(path.points[0].y, center.y - radius)
        expected_edge = 2.0 * radius * math.sin(2.0 * math.pi / 5.0)
        for index, (start, end) in enumerate(zip(path.points, path.points[1:], strict=False)):
            self.assertAlmostEqual(math.hypot(start.x - center.x, start.y - center.y), radius)
            self.assertAlmostEqual(math.hypot(end.x - start.x, end.y - start.y), expected_edge)
            vertex = path.at(index / 5.0)
            self.assertAlmostEqual(vertex.x, start.x)
            self.assertAlmostEqual(vertex.y, start.y)
        self.assertIs(path.at(1.0), path.points[0])

    def test_bezier_samples_curve_and_has_exact_endpoints(self) -> None:
        """Use the cubic curve's geometry and allow a nondegenerate closed curve."""
        start = Point(0.0, 0.0)
        end = Point(12.0, 0.0)
        path = MotionPath.bezier(start, Point(0.0, 12.0), Point(12.0, 12.0), end, samples=5)
        self.assertEqual(len(path.points), 5)
        self.assertEqual(path.points[2], Point(6.0, 9.0))
        self.assertIs(path.at(0.0), start)
        self.assertIs(path.at(1.0), end)
        self.assertAlmostEqual(path.at(0.5).x, 6.0)
        self.assertAlmostEqual(path.at(0.5).y, 9.0)
        closed = MotionPath.bezier(start, Point(0.0, 12.0), Point(12.0, 0.0), start)
        self.assertIs(closed.points[0], closed.points[-1])
        self.assertGreater(closed.at(0.5).x, 0.0)
        self.assertGreater(closed.at(0.5).y, 0.0)

    def test_invalid_geometry_is_rejected(self) -> None:
        """Reject nonfinite points, nonpositive dimensions, and stationary paths."""
        for value in (math.nan, math.inf, -math.inf):
            with self.subTest(coordinate=value):
                with self.assertRaises(ValueError):
                    Point(value, 0.0)
                with self.assertRaises(ValueError):
                    Point(0.0, value)
        for width, height in ((0, 10), (10, 0), (-1, 10), (10, -1), (True, 10)):
            with self.subTest(size=(width, height)), self.assertRaises(ValueError):
                Size(width, height)
        for points in ((), (Point(1.0, 1.0),), (Point(1.0, 1.0), Point(1.0, 1.0))):
            with self.subTest(points=points), self.assertRaises(ValueError):
                MotionPath(points)
        with self.assertRaises(ValueError):
            MotionPath.line(Point(-1e308, 0.0), Point(1e308, 0.0))

    def test_invalid_progress_and_builder_arguments_are_rejected(self) -> None:
        """Reject extrapolation, invalid sampling counts, and invalid star radii."""
        start = Point(0.0, 0.0)
        end = Point(10.0, 0.0)
        path = MotionPath.line(start, end)
        for progress in (-0.1, 1.1, math.nan, math.inf, -math.inf):
            with self.subTest(progress=progress), self.assertRaises(ValueError):
                path.at(progress)
        for samples in (0, 1, -1, True):
            with self.subTest(samples=samples), self.assertRaises(ValueError):
                MotionPath.bezier(start, start, end, end, samples=samples)
        for radius in (0.0, -1.0, math.nan, math.inf, -math.inf):
            with self.subTest(radius=radius), self.assertRaises(ValueError):
                MotionPath.star(start, radius)

    def test_geometry_values_are_frozen(self) -> None:
        """Prevent mutation from invalidating coordinates or cached path lengths."""
        for value, attribute in (
            (Point(1.0, 2.0), "x"),
            (Size(10, 20), "width"),
            (MotionPath.line(Point(0.0, 0.0), Point(1.0, 0.0)), "points"),
        ):
            with self.subTest(value=value), self.assertRaises(FrozenInstanceError):
                setattr(value, attribute, None)


if __name__ == "__main__":
    unittest.main()
