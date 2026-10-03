use std::collections::HashSet;

use smithay::input::keyboard::Keycode;
use smithay::utils::{Physical, Point};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoldButton {
    Pointer(u32),
    Tablet(u32),
}

/// One constraint shared by all drawing tools. Figure origins remain in their edits.
#[derive(Default)]
pub struct Axes {
    toggled: bool,
    held_keys: HashSet<Keycode>,
    held_buttons: HashSet<HoldButton>,
    projection: Option<Projection>,
}

struct Projection {
    raw: Point<i32, Physical>,
    anchor: Point<i32, Physical>,
    point: Point<i32, Physical>,
    ray: Option<Ray>,
    constrained: bool,
    via: Option<Point<i32, Physical>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Ray {
    Right,
    Left,
    Down,
    Up,
}

impl Axes {
    pub fn enabled(&self) -> bool {
        self.toggled || !self.held_buttons.is_empty() || !self.held_keys.is_empty()
    }

    pub fn toggle(&mut self, raw: Option<Point<i32, Physical>>) {
        self.set_enabled(!self.toggled, raw);
    }

    pub fn set_enabled(&mut self, enabled: bool, raw: Option<Point<i32, Physical>>) {
        if self.toggled == enabled {
            return;
        }
        self.toggled = enabled;
        self.reanchor(raw);
    }

    pub fn begin_hold(&mut self, key: Keycode, raw: Option<Point<i32, Physical>>) {
        if self.held_keys.insert(key) {
            self.reanchor(raw);
        }
    }

    pub fn end_hold(&mut self, key: Keycode, raw: Option<Point<i32, Physical>>) {
        if self.held_keys.remove(&key) {
            self.reanchor(raw);
        }
    }

    pub fn pointer_hold(
        &mut self,
        button: HoldButton,
        held: bool,
        raw: Option<Point<i32, Physical>>,
    ) {
        let changed = if held {
            self.held_buttons.insert(button)
        } else {
            self.held_buttons.remove(&button)
        };
        if changed {
            self.reanchor(raw);
        }
    }

    pub fn clear_holds(&mut self, raw: Option<Point<i32, Physical>>) -> bool {
        if self.held_keys.is_empty() && self.held_buttons.is_empty() {
            return false;
        }
        self.held_keys.clear();
        self.held_buttons.clear();
        self.reanchor(raw);
        true
    }

    fn reanchor(&mut self, raw: Option<Point<i32, Physical>>) {
        let enabled = self.enabled();
        if let Some(projection) = &mut self.projection {
            if let Some(raw) = raw {
                projection.raw = raw;
            }
            projection.anchor = projection.point;
            projection.ray = None;
            projection.constrained = enabled;
        }
    }

    pub fn begin(&mut self, point: Point<i32, Physical>, eligible: bool) {
        self.projection = eligible.then_some(Projection {
            raw: point,
            anchor: point,
            point,
            ray: None,
            constrained: self.enabled(),
            via: None,
        });
    }

    pub fn take_corner(&mut self) -> Option<Point<i32, Physical>> {
        self.projection.as_mut().and_then(|p| p.via.take())
    }

    pub fn end(&mut self) {
        self.projection = None;
    }

    pub fn origin(&self) -> Option<Point<i32, Physical>> {
        self.projection
            .as_ref()
            .filter(|p| p.constrained)
            .map(|p| p.anchor)
    }

    pub fn point(&self) -> Option<Point<i32, Physical>> {
        self.projection.as_ref().map(|p| p.point)
    }

    pub fn project(&mut self, raw: Point<i32, Physical>, threshold: f64) -> Point<i32, Physical> {
        let Some(p) = &mut self.projection else {
            return raw;
        };
        let dx = raw.x.saturating_sub(p.raw.x);
        let dy = raw.y.saturating_sub(p.raw.y);
        if !p.constrained {
            p.point = (p.anchor.x.saturating_add(dx), p.anchor.y.saturating_add(dy)).into();
            return p.point;
        }
        p.via = None;
        let old_ray = p.ray;
        let t = threshold.max(1.);
        let along = match p.ray {
            Some(Ray::Right) => f64::from(dx),
            Some(Ray::Left) => -f64::from(dx),
            Some(Ray::Down) => f64::from(dy),
            Some(Ray::Up) => -f64::from(dy),
            None => 0.,
        };
        if along <= t {
            p.ray = None;
        }
        if p.ray.is_none() && f64::from(dx).abs().max(f64::from(dy).abs()) > t {
            p.ray = Some(if dx.unsigned_abs() >= dy.unsigned_abs() {
                if dx >= 0 {
                    Ray::Right
                } else {
                    Ray::Left
                }
            } else if dy >= 0 {
                Ray::Down
            } else {
                Ray::Up
            });
        }
        if old_ray.is_some() && old_ray != p.ray {
            p.via = Some(p.anchor);
        }
        p.point = match p.ray {
            Some(Ray::Right | Ray::Left) => (p.anchor.x.saturating_add(dx), p.anchor.y).into(),
            Some(Ray::Up | Ray::Down) => (p.anchor.x, p.anchor.y.saturating_add(dy)).into(),
            None => p.anchor,
        };
        p.point
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reanchor_preserves_pen_and_figure_origin() {
        let mut axes = Axes::default();
        axes.set_enabled(true, None);
        axes.begin((100, 100).into(), true);
        assert_eq!(axes.project((200, 120).into(), 8.), (200, 100).into());
        axes.set_enabled(false, Some((200, 120).into()));
        assert_eq!(axes.project((200, 120).into(), 8.), (200, 100).into());
        axes.set_enabled(true, Some((200, 120).into()));
        assert_eq!(axes.origin(), Some((200, 100).into()));
        assert_eq!(axes.project((203, 220).into(), 8.), (200, 200).into());
    }

    #[test]
    fn junction_absorbs_small_overshoot_before_turning() {
        let mut axes = Axes::default();
        axes.set_enabled(true, None);
        axes.begin((100, 100).into(), true);
        assert_eq!(axes.project((180, 101).into(), 8.), (180, 100).into());
        assert_eq!(axes.project((97, 98).into(), 8.), (100, 100).into());
        assert_eq!(axes.project((97, 70).into(), 8.), (100, 70).into());
        axes.end();
        assert!(axes.origin().is_none());
    }

    #[test]
    fn hold_release_restores_toggle_and_multiple_keys() {
        let mut axes = Axes::default();
        axes.begin_hold(10u32.into(), None);
        axes.begin_hold(11u32.into(), None);
        axes.end_hold(10u32.into(), None);
        assert!(axes.enabled());
        axes.end_hold(11u32.into(), None);
        assert!(!axes.enabled());
        axes.set_enabled(true, None);
        axes.pointer_hold(HoldButton::Pointer(1), true, None);
        axes.pointer_hold(HoldButton::Pointer(1), false, None);
        assert!(axes.enabled());
    }

    #[test]
    fn button_hold_ignores_unrelated_releases() {
        let mut axes = Axes::default();
        axes.pointer_hold(HoldButton::Pointer(2), true, None);
        axes.begin((100, 100).into(), true);
        assert_eq!(axes.project((160, 103).into(), 8.), (160, 100).into());
        axes.pointer_hold(HoldButton::Pointer(1), false, None);
        axes.pointer_hold(HoldButton::Tablet(2), false, None);
        assert!(axes.enabled());
        assert_eq!(axes.origin(), Some((100, 100).into()));
        axes.pointer_hold(HoldButton::Tablet(2), true, None);
        axes.pointer_hold(HoldButton::Pointer(2), false, None);
        assert!(axes.enabled());
        axes.pointer_hold(HoldButton::Tablet(2), false, None);
        assert!(!axes.enabled());
    }
}
