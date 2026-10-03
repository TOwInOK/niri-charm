use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use anyhow::{Context as _, Result};
use niri_config::screenshot::{
    BlurStrength, ScreenshotBrush, ScreenshotConfig, ScreenshotToolbarAlign,
    ScreenshotToolbarPosition,
};
use niri_ipc::{ScreenshotColor, ScreenshotTool, ScreenshotValueChange};
use pangocairo::cairo::{self, ImageSurface};
use smithay::backend::allocator::Fourcc;
use smithay::backend::input::TouchSlot;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{ImportMem, Offscreen};
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point, Rectangle, Size, Transform};

use crate::render_helpers::blur::{Blur, BlurOptions};
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};

/// Binary exponent of a power-of-two zoom level; non-powers floor, 0 lands at 1x.
fn exponent_of(zoom: u32) -> i16 {
    zoom.max(1).ilog2().min(31) as i16
}

/// Configuration color converted to the opaque editor color.
fn rgb8(color: niri_config::Color) -> ScreenshotColor {
    ScreenshotColor {
        r: (color.r * 255.).round() as u8,
        g: (color.g * 255.).round() as u8,
        b: (color.b * 255.).round() as u8,
    }
}

fn rgba(color: niri_config::Color) -> (f64, f64, f64, f64) {
    (
        f64::from(color.r),
        f64::from(color.g),
        f64::from(color.b),
        f64::from(color.a),
    )
}

/// A completed or in-progress canvas edit.
pub enum Edit {
    Stroke(Stroke),
    Arrow(ArrowEdit),
    Rectangle(RectangleEdit),
    Blur(BlurEdit),
}

/// A freehand stroke. Color and width are captured when the gesture starts and
/// never change afterwards.
pub struct Stroke {
    points: Vec<Point<i32, Physical>>,
    color: ScreenshotColor,
    width: u32,
    bounds: Rectangle<i32, Physical>,
}
/// A straight arrow from the gesture origin to the current end point. Color
/// and width are captured when the gesture starts and never change afterwards.
pub struct ArrowEdit {
    origin: Point<i32, Physical>,
    end: Point<i32, Physical>,
    color: ScreenshotColor,
    width: u32,
}

/// An unfilled rectangle between opposite corners of the gesture. Color and
/// width are captured when the gesture starts and never change afterwards.
pub struct RectangleEdit {
    rect: Rectangle<i32, Physical>,
    color: ScreenshotColor,
    width: u32,
}

/// A blur rectangle. `strength` is captured when the gesture starts so a config
/// reload mid-gesture does not change the in-progress edit.
pub struct BlurEdit {
    rect: Rectangle<i32, Physical>,
    strength: BlurStrength,
}

pub struct Command {
    pub output: Output,
    pub edit: Edit,
}

use super::screenshot_ui::Interaction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Tool(ScreenshotTool),
    /// Palette swatch identified by its RGB value.
    Color(ScreenshotColor),
    Copy,
    Undo,
    Redo,
    Pointer,
    /// Axis-guides indicator; `Editor::axes_enabled` mirrors the live state.
    Axes,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOutcome {
    Changed,
    Save,
    Copy,
}
pub struct Editor {
    pub tool: ScreenshotTool,
    /// Session brush color; initialized from `brush.color` at open.
    pub color: ScreenshotColor,
    /// Session brush width in frozen-frame pixels; initialized from
    /// `brush.width` at open and clamped to the IPC brush-width bounds.
    pub width: u32,
    /// Session arrow color; initialized from `arrow.color` at open.
    pub arrow_color: ScreenshotColor,
    /// Session arrow width in frozen-frame pixels; initialized from
    /// `arrow.width` at open and clamped to the IPC brush-width bounds.
    pub arrow_width: u32,
    /// Session rectangle color; initialized from `rectangle.color` at open.
    pub rectangle_color: ScreenshotColor,
    /// Session rectangle width in frozen-frame pixels; initialized from
    /// `rectangle.width` at open and clamped to the IPC brush-width bounds.
    pub rectangle_width: u32,
    /// Session blur strength; initialized from `blur.strength` at open.
    pub blur_strength: BlurStrength,
    /// UI indicator for the axis guides; the owner holds the real state and
    /// sets this only so the toolbar can mirror it.
    pub axes_enabled: bool,
    pub pointer: Option<(Output, Point<i32, Physical>)>,
    pub history: Vec<Command>,
    pub redo: Vec<Command>,
    /// Bumped on every canvas-relevant change; invalidates the magnifier cache.
    pub revision: u64,
    /// Bumped when the applied settings or shortcut labels change; invalidates
    /// the toolbar raster cache.
    pub settings_revision: u64,
    pub magnifier_zoom: MagnifierZoom,
    pub palette_open: bool,
    pub focus: Option<Control>,
    pub space_down: bool,
    /// Config snapshot applied to this session. Reload replaces it via
    /// `apply_settings`; rendering and geometry only ever read this copy.
    pub settings: ScreenshotConfig,
    /// Shortcut labels for tooltips, built by the owner from the real binds.
    shortcuts: Vec<(Control, String)>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(ScreenshotConfig::default(), ScreenshotTool::Select)
    }
}

pub struct MagnifierZoom {
    exponent: i16,
    max_exponent: i16,
}

impl MagnifierZoom {
    fn new(max_zoom: u32) -> Self {
        Self {
            exponent: 0,
            max_exponent: exponent_of(max_zoom),
        }
    }

    /// Current magnification factor (a power of two).
    pub fn level(&self) -> u32 {
        1u32 << self.exponent
    }

    /// Absolute zoom sets the factor, a relative `+N`/`-N` changes the number of
    /// power-of-two steps. Returns whether the zoom changed.
    pub fn change(&mut self, zoom: ScreenshotValueChange) -> bool {
        let next = match zoom {
            ScreenshotValueChange::Set(level) => {
                // Floor to a power of two; level 0 lands at the 1x floor.
                exponent_of(level).clamp(0, self.max_exponent)
            }
            ScreenshotValueChange::Adjust(steps) => (i64::from(self.exponent) + i64::from(steps))
                .clamp(0, i64::from(self.max_exponent))
                as i16,
        };
        let changed = next != self.exponent;
        self.exponent = next;
        changed
    }

    /// Applies a new configured maximum, clamping the current zoom.
    pub fn set_max_zoom(&mut self, max_zoom: u32) {
        self.max_exponent = exponent_of(max_zoom);
        self.exponent = self.exponent.min(self.max_exponent);
    }

    /// Session-initial zoom from `initial-zoom`, bounded by the current max.
    fn reset(&mut self, initial_zoom: u32) {
        self.exponent = exponent_of(initial_zoom).clamp(0, self.max_exponent);
    }
}

impl Editor {
    pub fn new(settings: ScreenshotConfig, tool: ScreenshotTool) -> Self {
        let mut magnifier_zoom = MagnifierZoom::new(settings.magnifier.max_zoom);
        magnifier_zoom.reset(settings.magnifier.initial_zoom);
        Self {
            tool,
            color: rgb8(settings.brush.color),
            width: clamp_width(settings.brush.width),
            arrow_color: rgb8(settings.arrow.color),
            arrow_width: clamp_width(settings.arrow.width),
            rectangle_color: rgb8(settings.rectangle.color),
            rectangle_width: clamp_width(settings.rectangle.width),
            blur_strength: settings.blur.strength,
            axes_enabled: false,
            pointer: None,
            history: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            settings_revision: 0,
            magnifier_zoom,
            palette_open: false,
            focus: None,
            space_down: false,
            settings,
            shortcuts: Vec::new(),
        }
    }

    /// Applies an updated config to the open session. Theme, geometry, palette,
    /// thresholds and magnifier limits take effect immediately; session state
    /// (tool, colors, widths, history, crop) is preserved. The blur strength
    /// follows the config only when the configured strength itself changed, so
    /// an unrelated reload keeps the live selection. Callers defer this while
    /// a gesture is in progress so button geometry cannot move between press
    /// and release.
    pub fn apply_settings(&mut self, settings: ScreenshotConfig) {
        self.magnifier_zoom
            .set_max_zoom(settings.magnifier.max_zoom);
        if self.settings.blur.strength != settings.blur.strength {
            self.blur_strength = settings.blur.strength;
        }
        self.settings = settings;
        // Focus on a swatch that no longer exists returns to the active
        // colored tool's button, or the brush button for the other tools.
        if let Some(Control::Color(color)) = self.focus {
            if !self
                .palette()
                .iter()
                .any(|swatch| rgb8(*swatch) == color)
            {
                let tool = self.color_tool().unwrap_or(ScreenshotTool::Draw);
                self.focus = Some(Control::Tool(tool));
            }
        }
        self.revision = self.revision.wrapping_add(1);
        self.settings_revision = self.settings_revision.wrapping_add(1);
    }

    /// Replaces the shortcut labels shown in tooltips. Rebuilt by the owner on
    /// open and on every config reload from the actually bound keys; an action
    /// without a bind simply has no entry.
    pub fn set_shortcuts(&mut self, shortcuts: Vec<(Control, String)>) {
        self.shortcuts = shortcuts;
        self.settings_revision = self.settings_revision.wrapping_add(1);
    }

    /// Palette of the active tool's brush config; empty for tools without a
    /// color (Select, Blur).
    pub fn palette(&self) -> &[niri_config::Color] {
        tool_palette(&self.settings, self.tool)
    }

    /// Brush-style settings of a tool's own config section. Meaningful only
    /// for colored tools (`color_tool`); other tools return the brush section.
    pub fn tool_settings(&self, tool: ScreenshotTool) -> &ScreenshotBrush {
        match tool {
            ScreenshotTool::Arrow => &self.settings.arrow,
            ScreenshotTool::Rectangle => &self.settings.rectangle,
            _ => &self.settings.brush,
        }
    }

    /// The active tool if it has its own color, i.e. a palette applies to it.
    /// Used to decide whether repeat-clicking the tool opens the palette and
    /// which tool a swatch or palette action targets; never selects a tool.
    pub fn color_tool(&self) -> Option<ScreenshotTool> {
        colored_tool(self.tool)
    }

    /// The color new edits of `tool` take; the brush color for tools without
    /// a color of their own.
    pub fn tool_color(&self, tool: ScreenshotTool) -> ScreenshotColor {
        match tool {
            ScreenshotTool::Draw => self.color,
            ScreenshotTool::Arrow => self.arrow_color,
            ScreenshotTool::Rectangle => self.rectangle_color,
            _ => self.color,
        }
    }

    /// The width new edits of `tool` take in frozen-frame pixels; the brush
    /// width for tools without a width of their own.
    pub fn tool_width(&self, tool: ScreenshotTool) -> u32 {
        match tool {
            ScreenshotTool::Draw => self.width,
            ScreenshotTool::Arrow => self.arrow_width,
            ScreenshotTool::Rectangle => self.rectangle_width,
            _ => self.width,
        }
    }

    /// Sets the session color of a colored tool. No-op for the others.
    pub fn set_tool_color(&mut self, tool: ScreenshotTool, color: ScreenshotColor) {
        match tool {
            ScreenshotTool::Draw => self.color = color,
            ScreenshotTool::Arrow => self.arrow_color = color,
            ScreenshotTool::Rectangle => self.rectangle_color = color,
            _ => {}
        }
    }

    /// Sets or adjusts the session width of a colored tool, clamped to the IPC
    /// brush-width bounds. No-op for the others; returns whether it changed.
    pub fn change_tool_width(
        &mut self,
        tool: ScreenshotTool,
        change: ScreenshotValueChange,
    ) -> bool {
        let width = self.tool_width(tool);
        let next = match change {
            ScreenshotValueChange::Set(value) => value,
            ScreenshotValueChange::Adjust(delta) => {
                width.saturating_add_signed(delta)
            }
        };
        let next = clamp_width(next);
        let changed = next != width;
        match tool {
            ScreenshotTool::Draw => self.width = next,
            ScreenshotTool::Arrow => self.arrow_width = next,
            ScreenshotTool::Rectangle => self.rectangle_width = next,
            _ => {}
        }
        changed
    }

    /// Sets or adjusts the session blur strength: an absolute value is the
    /// 1-based preset index clamped to 1–3, an adjustment steps through the
    /// presets. Returns whether the strength changed.
    pub fn change_blur_strength(&mut self, change: ScreenshotValueChange) -> bool {
        let index = strength_index(self.blur_strength) as i64;
        let next = match change {
            ScreenshotValueChange::Set(level) => i64::from(level).clamp(1, 3) - 1,
            ScreenshotValueChange::Adjust(delta) => (index + i64::from(delta)).clamp(0, 2),
        } as usize;
        let next = [BlurStrength::Low, BlurStrength::Medium, BlurStrength::High][next];
        let changed = next != self.blur_strength;
        self.blur_strength = next;
        changed
    }
    /// Shortcut label for a control, e.g. `Ctrl+Z`, if one is bound.
    pub fn shortcut(&self, control: Control) -> Option<&str> {
        self.shortcuts
            .iter()
            .find(|(c, _)| *c == control)
            .map(|(_, label)| label.as_str())
    }

    pub fn edits<'a>(
        &'a self,
        output: &'a Output,
        interaction: &'a Interaction,
    ) -> impl Iterator<Item = &'a Edit> + Clone {
        self.history
            .iter()
            .filter(move |c| &c.output == output)
            .map(|c| &c.edit)
            .chain(
                std::iter::once(interaction).filter_map(move |gesture| match gesture {
                    Interaction::Canvas {
                        output: owner,
                        edit,
                        ..
                    } if owner == output => Some(edit),
                    _ => None,
                }),
            )
    }

    /// Starts a canvas gesture, capturing the current tool color, width and
    /// blur strength into the new edit.
    pub fn begin(
        &self,
        output: Output,
        point: Point<i32, Physical>,
        slot: Option<TouchSlot>,
    ) -> Interaction {
        let edit = match self.tool {
            ScreenshotTool::Select => return Interaction::Idle,
            ScreenshotTool::Draw => Edit::Stroke(Stroke {
                points: vec![point],
                color: self.color,
                width: self.width,
                bounds: segment_bounds(point, point, self.width),
            }),
            ScreenshotTool::Arrow => Edit::Arrow(ArrowEdit {
                origin: point,
                end: point,
                color: self.arrow_color,
                width: self.arrow_width,
            }),
            ScreenshotTool::Rectangle => Edit::Rectangle(RectangleEdit {
                rect: rectangle(point, point),
                color: self.rectangle_color,
                width: self.rectangle_width,
            }),
            ScreenshotTool::Blur => Edit::Blur(BlurEdit {
                rect: rectangle(point, point),
                strength: self.blur_strength,
            }),
        };
        Interaction::Canvas {
            output,
            slot,
            origin: point,
            edit,
        }
    }

    pub fn motion(
        interaction: &mut Interaction,
        point: Point<i32, Physical>,
        slot: Option<TouchSlot>,
    ) -> Option<Rectangle<i32, Physical>> {
        let Interaction::Canvas {
            slot: owner,
            origin,
            edit,
            ..
        } = interaction
        else {
            return None;
        };
        if *owner != slot {
            return None;
        }
        match edit {
            Edit::Stroke(stroke) => {
                let previous = *stroke.points.last().unwrap();
                if previous == point {
                    return None;
                }
                stroke.points.push(point);
                let damage = segment_bounds(previous, point, stroke.width);
                stroke.bounds = stroke.bounds.merge(damage);
                Some(damage)
            }
            Edit::Arrow(arrow) => {
                if arrow.end == point {
                    return None;
                }
                let old = arrow_bounds(arrow.origin, arrow.end, arrow.width);
                arrow.end = point;
                Some(old.merge(arrow_bounds(arrow.origin, arrow.end, arrow.width)))
            }
            Edit::Rectangle(rect_edit) => {
                let old = rect_bounds(rect_edit.rect, rect_edit.width);
                rect_edit.rect = rectangle(*origin, point);
                Some(old.merge(rect_bounds(rect_edit.rect, rect_edit.width)))
            }
            Edit::Blur(blur) => {
                blur.rect = rectangle(*origin, point);
                None
            }
        }
    }
}
impl Edit {
    /// Damage for rasterized edits; blur rects are drawn directly and do not
    /// touch the raster.
    pub fn stroke_bounds(&self) -> Option<Rectangle<i32, Physical>> {
        match self {
            Self::Stroke(stroke) => Some(stroke.bounds),
            Self::Arrow(arrow) => Some(arrow_bounds(arrow.origin, arrow.end, arrow.width)),
            Self::Rectangle(rect_edit) => Some(rect_bounds(rect_edit.rect, rect_edit.width)),
            Self::Blur(_) => None,
        }
    }
}


/// Clamps a configured or requested stroke width to the IPC bounds.
fn clamp_width(width: u32) -> u32 {
    width.clamp(
        niri_ipc::MIN_SCREENSHOT_BRUSH_WIDTH,
        niri_ipc::MAX_SCREENSHOT_BRUSH_WIDTH,
    )
}

/// Whether the tool has its own color and width, i.e. a palette applies.
pub fn colored_tool(tool: ScreenshotTool) -> Option<ScreenshotTool> {
    match tool {
        ScreenshotTool::Draw | ScreenshotTool::Arrow | ScreenshotTool::Rectangle => Some(tool),
        _ => None,
    }
}

/// Palette of `tool`'s brush-style config; empty for tools without a color.
fn tool_palette(settings: &ScreenshotConfig, tool: ScreenshotTool) -> &[niri_config::Color] {
    match tool {
        ScreenshotTool::Draw => &settings.brush.palette,
        ScreenshotTool::Arrow => &settings.arrow.palette,
        ScreenshotTool::Rectangle => &settings.rectangle.palette,
        _ => &[],
    }
}

/// Slot of a colored tool in `ToolbarState`'s `colors`/`widths` arrays.
fn color_index(tool: ScreenshotTool) -> usize {
    match tool {
        ScreenshotTool::Arrow => 1,
        ScreenshotTool::Rectangle => 2,
        _ => 0,
    }
}

/// Arrowhead arm length in pixels, proportional to the stroke width.
fn arrow_head_len(width: u32) -> f64 {
    (f64::from(width) * 2.8).clamp(9., 72.)
}

/// Conservative bounds of a rasterized rect; `pad` covers the stroke radius.
fn padded(rect: Rectangle<i32, Physical>, pad: i32) -> Rectangle<i32, Physical> {
    Rectangle::new(
        rect.loc - Point::from((pad, pad)),
        rect.size + Size::from((pad * 2, pad * 2)),
    )
}

fn rect_bounds(rect: Rectangle<i32, Physical>, width: u32) -> Rectangle<i32, Physical> {
    padded(rect, width as i32 / 2 + 1)
}

fn arrow_bounds(
    a: Point<i32, Physical>,
    b: Point<i32, Physical>,
    width: u32,
) -> Rectangle<i32, Physical> {
    padded(rectangle(a, b), width as i32 / 2 + arrow_head_len(width) as i32 + 1)
}

/// Draws the arrow shaft and head; caller sets width, color, caps and joins.
/// A zero-length gesture is a dot, matching a one-point stroke.
fn draw_arrow(cr: &cairo::Context, arrow: &ArrowEdit) -> Result<()> {
    let (ax, ay) = (f64::from(arrow.origin.x) + 0.5, f64::from(arrow.origin.y) + 0.5);
    let (bx, by) = (f64::from(arrow.end.x) + 0.5, f64::from(arrow.end.y) + 0.5);
    if arrow.origin == arrow.end {
        cr.arc(ax, ay, cr.line_width() / 2., 0., std::f64::consts::TAU);
        cr.fill()?;
        return Ok(());
    }
    let (dx, dy) = (bx - ax, by - ay);
    let len = dx.hypot(dy);
    let (ux, uy) = (dx / len, dy / len);
    let head = arrow_head_len(arrow.width).min(len / 2.);
    let fin = head * 0.55;
    cr.move_to(ax, ay);
    cr.line_to(bx, by);
    for sign in [-1., 1.] {
        cr.move_to(bx, by);
        cr.line_to(bx - head * ux - sign * fin * uy, by - head * uy + sign * fin * ux);
    }
    cr.stroke()?;
    Ok(())
}

/// Draws the unfilled rectangle; caller sets width, color and joins. A
/// degenerate rect too thin to outline is filled instead.
fn draw_rect(cr: &cairo::Context, rect_edit: &RectangleEdit) -> Result<()> {
    let rect = rect_edit.rect;
    let w = f64::from(rect.size.w);
    let h = f64::from(rect.size.h);
    let width = f64::from(rect_edit.width);
    let (x, y) = (f64::from(rect.loc.x), f64::from(rect.loc.y));
    if w <= width || h <= width {
        cr.rectangle(x, y, w, h);
        cr.fill()?;
    } else {
        cr.rectangle(x + width / 2., y + width / 2., w - width, h - width);
        cr.stroke()?;
    }
    Ok(())
}
pub fn rectangle(a: Point<i32, Physical>, b: Point<i32, Physical>) -> Rectangle<i32, Physical> {
    Rectangle::from_extremities(
        (a.x.min(b.x), a.y.min(b.y)),
        (a.x.max(b.x) + 1, a.y.max(b.y) + 1),
    )
}

fn segment_bounds(
    a: Point<i32, Physical>,
    b: Point<i32, Physical>,
    width: u32,
) -> Rectangle<i32, Physical> {
    let rect = rectangle(a, b);
    let pad = (width as i32) / 2 + 1;
    Rectangle::new(
        rect.loc - Point::from((pad, pad)),
        rect.size + Size::from((pad * 2, pad * 2)),
    )
}

#[derive(Default)]
pub struct Canvas {
    raster: RefCell<Option<Raster>>,
    damage: RefCell<Option<Rectangle<i32, Physical>>>,
    /// Lazily blurred copies of the captured source, keyed by
    /// `[source variant][strength]`; at most three presets per source.
    blurred: [[OnceCell<TextureBuffer<GlesTexture>>; 3]; 3],
}

struct Raster {
    surface: ImageSurface,
    buffer: Option<TextureBuffer<GlesTexture>>,
}

/// Maps a configured blur strength to the shader parameters. This is the single
/// place the preset table lives.
fn blur_options(strength: BlurStrength) -> BlurOptions {
    match strength {
        BlurStrength::Low => BlurOptions {
            passes: 2,
            offset: 2.,
        },
        BlurStrength::Medium => BlurOptions {
            passes: 3,
            offset: 3.,
        },
        BlurStrength::High => BlurOptions {
            passes: 4,
            offset: 4.,
        },
    }
}

fn strength_index(strength: BlurStrength) -> usize {
    match strength {
        BlurStrength::Low => 0,
        BlurStrength::Medium => 1,
        BlurStrength::High => 2,
    }
}

impl Canvas {
    pub fn damage(&self, rect: Option<Rectangle<i32, Physical>>) {
        if let Some(rect) = rect {
            let mut damage = self.damage.borrow_mut();
            *damage = Some(damage.map_or(rect, |old| old.merge(rect)));
        }
    }

    pub fn prepare(
        &self,
        renderer: &mut GlesRenderer,
        editor: &Editor,
        interaction: &Interaction,
        output: &Output,
        size: Size<i32, Physical>,
        scale: f64,
    ) -> Result<()> {
        let Some(damage) = *self.damage.borrow() else {
            return Ok(());
        };
        let Some(damage) = damage.intersection(Rectangle::from_size(size)) else {
            self.damage.borrow_mut().take();
            return Ok(());
        };
        let mut raster = self.raster.borrow_mut();
        if raster.is_none() {
            *raster = Some(Raster {
                surface: ImageSurface::create(cairo::Format::ARgb32, size.w, size.h)?,
                buffer: None,
            });
        }
        let raster = raster.as_mut().unwrap();
        let cr = cairo::Context::new(&raster.surface)?;
        cr.rectangle(
            damage.loc.x.into(),
            damage.loc.y.into(),
            damage.size.w.into(),
            damage.size.h.into(),
        );
        cr.clip();
        cr.set_operator(cairo::Operator::Clear);
        cr.paint()?;
        cr.set_operator(cairo::Operator::Over);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        for edit in editor.edits(output, interaction) {
            let (bounds, color, width) = match edit {
                Edit::Stroke(stroke) => (stroke.bounds, stroke.color, stroke.width),
                Edit::Arrow(arrow) => (
                    arrow_bounds(arrow.origin, arrow.end, arrow.width),
                    arrow.color,
                    arrow.width,
                ),
                Edit::Rectangle(rect_edit) => (
                    rect_bounds(rect_edit.rect, rect_edit.width),
                    rect_edit.color,
                    rect_edit.width,
                ),
                Edit::Blur(_) => continue,
            };
            if !bounds.overlaps(damage) {
                continue;
            }
            cr.set_line_width(f64::from(width));
            let (r, g, b) = color.rgb();
            cr.set_source_rgb(r, g, b);
            match edit {
                Edit::Stroke(stroke) => {
                    if stroke.points.len() == 1 {
                        let p = stroke.points[0];
                        cr.arc(
                            f64::from(p.x) + 0.5,
                            f64::from(p.y) + 0.5,
                            f64::from(stroke.width) / 2.,
                            0.,
                            std::f64::consts::TAU,
                        );
                        cr.fill()?;
                    } else {
                        // Only submit segments that can affect the dirty area, including their round caps.
                        for pair in stroke.points.windows(2) {
                            if segment_bounds(pair[0], pair[1], stroke.width).overlaps(damage) {
                                cr.move_to(f64::from(pair[0].x) + 0.5, f64::from(pair[0].y) + 0.5);
                                cr.line_to(f64::from(pair[1].x) + 0.5, f64::from(pair[1].y) + 0.5);
                            }
                        }
                        cr.stroke()?;
                    }
                }
                Edit::Arrow(arrow) => draw_arrow(&cr, arrow)?,
                Edit::Rectangle(rect_edit) => draw_rect(&cr, rect_edit)?,
                Edit::Blur(_) => unreachable!(),
            }
        }
        drop(cr);
        let pixels = raster.surface.data()?;
        if let Some(buffer) = &raster.buffer {
            renderer.update_memory(
                buffer.texture(),
                &pixels,
                damage
                    .to_logical(1)
                    .to_buffer(1, Transform::Normal, &size.to_logical(1)),
            )?;
            // A fresh element identity invalidates renderer damage without duplicating the texture.
            raster.buffer = Some(TextureBuffer::from_texture(
                renderer,
                buffer.texture().clone(),
                scale,
                Transform::Normal,
                Vec::new(),
            ));
        } else {
            raster.buffer = Some(TextureBuffer::from_memory(
                renderer,
                &pixels,
                Fourcc::Argb8888,
                (size.w, size.h),
                false,
                scale,
                Transform::Normal,
                Vec::new(),
            )?);
        }
        self.damage.borrow_mut().take();
        Ok(())
    }

    pub fn elements<'a>(
        &self,
        renderer: &mut GlesRenderer,
        edits: impl Iterator<Item = &'a Edit> + Clone,
        source: &GlesTexture,
        target: usize,
        scale: f64,
        push: &mut dyn FnMut(PrimaryGpuTextureRenderElement),
    ) -> Result<()> {
        if let Some(raster) = self.raster.borrow().as_ref() {
            if let Some(buffer) = &raster.buffer {
                push(element(buffer.clone(), (0., 0.), None, None));
            }
        }
        // Front-to-back: stronger blur always covers weaker blur, independently
        // of history order. Three fixed passes avoid sorting or allocating.
        for strength in [BlurStrength::High, BlurStrength::Medium, BlurStrength::Low] {
            for edit in edits.clone() {
                let Edit::Blur(blur) = edit else {
                    continue;
                };
                if blur.strength != strength {
                    continue;
                }
                let slot = &self.blurred[target][strength_index(strength)];
                let blurred = match slot.get() {
                    Some(buffer) => buffer,
                    None => {
                        let mut blur_fx =
                            Blur::new(renderer).context("screenshot blur shader is unavailable")?;
                        let options = blur_options(strength);
                        blur_fx.prepare_textures(
                            |format, size| renderer.create_buffer(format, size),
                            source,
                            options,
                        )?;
                        let texture = blur_fx.render(renderer, source, options)?;
                        let buffer = TextureBuffer::from_texture(
                            renderer,
                            texture,
                            scale,
                            Transform::Normal,
                            Vec::new(),
                        );
                        let _ = slot.set(buffer);
                        slot.get().unwrap()
                    }
                };
                let rect = blur.rect.to_f64().to_logical(scale);
                push(element(
                    blurred.clone(),
                    rect.loc,
                    Some(rect),
                    Some(rect.size),
                ));
            }
        }
        Ok(())
    }
}

pub fn element(
    buffer: TextureBuffer<GlesTexture>,
    location: impl Into<Point<f64, smithay::utils::Logical>>,
    src: Option<Rectangle<f64, smithay::utils::Logical>>,
    size: Option<Size<f64, smithay::utils::Logical>>,
) -> PrimaryGpuTextureRenderElement {
    PrimaryGpuTextureRenderElement(TextureRenderElement::from_texture_buffer(
        buffer,
        location,
        1.,
        src,
        size,
        Kind::Unspecified,
    ))
}

pub const CONTROLS: [Control; 11] = [
    Control::Tool(ScreenshotTool::Select),
    Control::Tool(ScreenshotTool::Draw),
    Control::Tool(ScreenshotTool::Arrow),
    Control::Tool(ScreenshotTool::Rectangle),
    Control::Tool(ScreenshotTool::Blur),
    Control::Undo,
    Control::Redo,
    Control::Pointer,
    Control::Axes,
    Control::Copy,
    Control::Save,
];

/// Toolbar icon SVGs in `CONTROLS` order. Each is rasterized once per pixel
/// size into a cached alpha mask and tinted by cairo, so pointer motion never
/// reparses or rerasterizes SVG.
const ICONS: [&[u8]; 11] = [
    include_bytes!("../../resources/icons/selection.svg"),
    include_bytes!("../../resources/icons/brush.svg"),
    include_bytes!("../../resources/icons/arrow.svg"),
    include_bytes!("../../resources/icons/rectangle.svg"),
    include_bytes!("../../resources/icons/blur.svg"),
    include_bytes!("../../resources/icons/undo.svg"),
    include_bytes!("../../resources/icons/redo.svg"),
    include_bytes!("../../resources/icons/cursor.svg"),
    include_bytes!("../../resources/icons/axes.svg"),
    include_bytes!("../../resources/icons/copy.svg"),
    include_bytes!("../../resources/icons/save.svg"),
];

// Layout constants without a dedicated config field, in logical pixels before
// the uniform narrow-output shrink factor. Configurable sizes (button, icon)
// come from `settings.toolbar`.
const PAD: f64 = 4.;
const GROUP_GAP: f64 = 12.;
const POP_CELL: f64 = 28.;
const POP_PAD: f64 = 6.;
/// Palette popover swatches per row; a configured palette wraps into rows.
const POP_COLS: usize = 4;
/// Gap between the bar, the popover and the tooltip strip.
const GAP: f64 = 4.;
const STRIPE: f64 = 3.;
const ERROR_TEXT: &str = "Couldn't render screenshot preview";

thread_local! {
    /// Icon alpha mask surfaces keyed by (icon index, edge in pixels); an
    /// `ImageSurface` clone is a cheap reference bump, so pointer motion and
    /// hover redraws never reparse or rerasterize SVG.
    static ICON_MASKS: RefCell<HashMap<(usize, u32), ImageSurface>> =
        RefCell::new(HashMap::new());
}

/// Index of the gap-bearing control groups in `CONTROLS`.
fn group_before(index: usize) -> bool {
    // [tools] [undo, redo] [pointer] [axes] [copy, save]
    matches!(index, 5 | 7 | 8 | 9)
}

/// Single-row bar layout in logical coordinates; `xs` are content offsets.
struct BarLayout {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    cell: f64,
    pad: f64,
    k: f64,
    xs: [f64; 11],
}

fn bar_layout(size: Size<i32, Physical>, scale: f64, settings: &ScreenshotConfig) -> BarLayout {
    let toolbar = &settings.toolbar;
    let margin = &toolbar.margin;
    let (mt, mr, mb, ml) = (margin.top, margin.right, margin.bottom, margin.left);
    let width = f64::from(size.w) / scale;
    let height = f64::from(size.h) / scale;
    let content = CONTROLS.len() as f64 * toolbar.button_size + 4. * GROUP_GAP + 2. * PAD;
    let avail_w = (width - ml - mr).max(1.);
    let avail_h = (height - mt - mb).max(1.);
    // Shrink uniformly on small outputs; the bar never wraps to a second row.
    let k_w = avail_w / content;
    let k_h = avail_h / (toolbar.button_size + 2. * PAD);
    let k = k_w.min(k_h).clamp(0.05, 1.);
    let cell = toolbar.button_size * k;
    let pad = PAD * k;
    let gap = GROUP_GAP * k;
    let mut xs = [0.; 11];
    let mut x = 0.;
    for (i, slot) in xs.iter_mut().enumerate() {
        if group_before(i) {
            x += gap;
        }
        *slot = x;
        x += cell;
    }
    let w = x + 2. * pad;
    // Align inside the margin-bounded rectangle.
    let x = match toolbar.align {
        ScreenshotToolbarAlign::Left => ml,
        ScreenshotToolbarAlign::Right => (width - mr - w).max(ml),
        ScreenshotToolbarAlign::Center => {
            (ml + (avail_w - w) / 2.).clamp(ml, (width - mr - w).max(ml))
        }
    };
    let y = match toolbar.position {
        ScreenshotToolbarPosition::Top => mt,
        ScreenshotToolbarPosition::Bottom => (height - mb - (cell + 2. * pad)).max(mt),
    };
    BarLayout {
        x,
        y,
        w,
        h: cell + 2. * pad,
        cell,
        pad,
        k,
        xs,
    }
}

/// Palette popover layout in logical coordinates.
struct PopLayout {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    cell: f64,
    pad: f64,
}

fn pop_layout(
    size: Size<i32, Physical>,
    scale: f64,
    settings: &ScreenshotConfig,
    tool: ScreenshotTool,
    bar: &BarLayout,
) -> PopLayout {
    let margin = &settings.toolbar.margin;
    let (mt, mr, mb, ml) = (margin.top, margin.right, margin.bottom, margin.left);
    let width = f64::from(size.w) / scale;
    let height = f64::from(size.h) / scale;
    let len = tool_palette(settings, tool).len();
    // Compact grid of at most four columns; a shorter palette just has fewer.
    let cols = len.clamp(1, POP_COLS) as f64;
    let rows = len.div_ceil(POP_COLS).max(1) as f64;
    let k = ((width - ml - mr).max(1.) / (cols * POP_CELL + 2. * POP_PAD)).clamp(0.1, 1.);
    let cell = POP_CELL * k;
    let pad = POP_PAD * k;
    let w = cols * cell + 2. * pad;
    let h = rows * cell + 2. * pad;
    // Anchor on the tool's own button, opening toward the screen center,
    // clamped to the margin-bounded area.
    let anchor = CONTROLS
        .iter()
        .position(|c| *c == Control::Tool(tool))
        .unwrap_or(0);
    let palette_cx = bar.x + bar.pad + bar.xs[anchor] + bar.cell / 2.;
    let x = (palette_cx - w / 2.).clamp(ml, (width - w - mr).max(ml));
    let y = match settings.toolbar.position {
        ScreenshotToolbarPosition::Top => bar.y + bar.h + GAP,
        ScreenshotToolbarPosition::Bottom => bar.y - GAP - h,
    }
    .clamp(mt, (height - h - mb).max(mt));
    PopLayout {
        x,
        y,
        w,
        h,
        cell,
        pad,
    }
}

fn phys_rect(x: f64, y: f64, w: f64, h: f64, scale: f64) -> Rectangle<i32, Physical> {
    Rectangle::from_extremities(
        ((x * scale).round() as i32, (y * scale).round() as i32),
        (
            ((x + w) * scale).round() as i32,
            ((y + h) * scale).round() as i32,
        ),
    )
}

/// Physical rectangle of a toolbar control. Indices `0..CONTROLS.len()` are
/// bar buttons; `CONTROLS.len() + i` is the palette popover swatch for
/// `tool`'s palette swatch at index `i` (geometry is defined whether or
/// is open).
pub fn control_rect(
    index: usize,
    size: Size<i32, Physical>,
    scale: f64,
    settings: &ScreenshotConfig,
    tool: ScreenshotTool,
) -> Rectangle<i32, Physical> {
    let bar = bar_layout(size, scale, settings);
    if index < CONTROLS.len() {
        phys_rect(
            bar.x + bar.pad + bar.xs[index],
            bar.y + bar.pad,
            bar.cell,
            bar.cell,
            scale,
        )
    } else if index < CONTROLS.len() + tool_palette(settings, tool).len() {
        let pop = pop_layout(size, scale, settings, tool, &bar);
        let swatch = index - CONTROLS.len();
        phys_rect(
            pop.x + pop.pad + (swatch % POP_COLS) as f64 * pop.cell,
            pop.y + pop.pad + (swatch / POP_COLS) as f64 * pop.cell,
            pop.cell,
            pop.cell,
            scale,
        )
    } else {
        Rectangle::new((0, 0).into(), (0, 0).into())
    }
}

pub fn hit_control(
    point: Point<i32, Physical>,
    size: Size<i32, Physical>,
    scale: f64,
    palette_open: bool,
    settings: &ScreenshotConfig,
    tool: ScreenshotTool,
) -> Option<Control> {
    if palette_open {
        for (i, color) in tool_palette(settings, tool).iter().enumerate() {
            if control_rect(CONTROLS.len() + i, size, scale, settings, tool).contains(point) {
                return Some(Control::Color(rgb8(*color)));
            }
        }
    }
    CONTROLS.iter().enumerate().find_map(|(index, control)| {
        control_rect(index, size, scale, settings, tool)
            .contains(point)
            .then_some(*control)
    })
}

/// Whether the point is on the toolbar panel itself (bar or open popover),
/// absorbing background clicks instead of starting a canvas edit.
pub fn overlay_contains(
    point: Point<i32, Physical>,
    size: Size<i32, Physical>,
    scale: f64,
    palette_open: bool,
    settings: &ScreenshotConfig,
    tool: ScreenshotTool,
) -> bool {
    let bar = bar_layout(size, scale, settings);
    if phys_rect(bar.x, bar.y, bar.w, bar.h, scale).contains(point) {
        return true;
    }
    if palette_open {
        let pop = pop_layout(size, scale, settings, tool, &bar);
        if phys_rect(pop.x, pop.y, pop.w, pop.h, scale).contains(point) {
            return true;
        }
    }
    false
}

pub fn control_enabled(control: Control, editor: &Editor, interaction: &Interaction) -> bool {
    match control {
        // The palette and its colors only make sense for colored tools.
        Control::Color(color) => {
            editor.color_tool().is_some()
                && editor.palette().iter().any(|swatch| rgb8(*swatch) == color)
        }
        // An in-progress canvas gesture can be cancelled by undo.
        Control::Undo => {
            !editor.history.is_empty() || matches!(interaction, Interaction::Canvas { .. })
        }
        Control::Redo => !editor.redo.is_empty(),
        _ => true,
    }
}

/// Combined bar + popover index for `control_rect`.
fn control_index(control: Control, settings: &ScreenshotConfig, tool: ScreenshotTool) -> usize {
    if let Some(index) = CONTROLS.iter().position(|c| *c == control) {
        index
    } else if let Control::Color(color) = control {
        CONTROLS.len()
            + tool_palette(settings, tool)
                .iter()
                .position(|c| rgb8(*c) == color)
                .unwrap_or(0)
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct ToolbarState {
    tool: ScreenshotTool,
    /// Color of each colored tool: [brush, arrow, rectangle].
    colors: [ScreenshotColor; 3],
    /// Width of each colored tool, same order.
    widths: [u32; 3],
    blur_strength: BlurStrength,
    axes_enabled: bool,
    palette_open: bool,
    palette_len: usize,
    focus: Option<Control>,
    hover: Option<Control>,
    pressed: Option<Control>,
    pointer: bool,
    error: bool,
    colored: bool,
    undo: bool,
    redo: bool,
    settings_revision: u64,
}

impl ToolbarState {
    pub fn new(
        editor: &Editor,
        interaction: &Interaction,
        pointer: bool,
        error: bool,
        hover: Option<Control>,
    ) -> Self {
        let pressed = match interaction {
            Interaction::Toolbar { control, .. } => Some(*control),
            _ => None,
        };
        Self {
            tool: editor.tool,
            colors: [
                editor.color,
                editor.arrow_color,
                editor.rectangle_color,
            ],
            widths: [
                editor.width,
                editor.arrow_width,
                editor.rectangle_width,
            ],
            blur_strength: editor.blur_strength,
            axes_enabled: editor.axes_enabled,
            palette_open: editor.palette_open,
            palette_len: editor.palette().len(),
            focus: editor.focus,
            hover,
            pressed,
            pointer,
            error,
            colored: editor.color_tool().is_some(),
            undo: control_enabled(Control::Undo, editor, interaction),
            redo: control_enabled(Control::Redo, editor, interaction),
            settings_revision: editor.settings_revision,
        }
    }

    /// Mirrors `control_enabled` on the captured flags; swatch membership is
    /// covered by only drawing `palette_len` swatches.
    fn enabled(&self, control: Control) -> bool {
        match control {
            Control::Color(_) => self.colored,
            Control::Undo => self.undo,
            Control::Redo => self.redo,
            _ => true,
        }
    }
}

/// The tooltip actually shown for a frame. Hidden while the element fades
/// (`alpha < 1`), so gesture start/end with nothing hovered or focused reuses
/// the cached texture without a raster upload.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tip {
    None,
    Control(Control),
    Error,
}

fn tooltip_text(state: &ToolbarState, editor: &Editor, control: Control) -> String {
    let suffix = |control: Control| match editor.shortcut(control) {
        Some(keys) => format!(" · {keys}"),
        None => String::new(),
    };
    match control {
        Control::Tool(ScreenshotTool::Select) => format!("Select{}", suffix(control)),
        Control::Tool(ScreenshotTool::Draw)
        | Control::Tool(ScreenshotTool::Arrow)
        | Control::Tool(ScreenshotTool::Rectangle) => {
            let Control::Tool(tool) = control else {
                unreachable!()
            };
            let name = match tool {
                ScreenshotTool::Draw => "Draw",
                ScreenshotTool::Arrow => "Arrow",
                _ => "Rectangle",
            };
            format!(
                "{name}, width {}{}",
                editor.tool_width(tool),
                suffix(control)
            )
        }
        Control::Tool(ScreenshotTool::Blur) => {
            let strength = match state.blur_strength {
                BlurStrength::Low => "Low",
                BlurStrength::Medium => "Medium",
                BlurStrength::High => "High",
            };
            format!("Blur {strength}{}", suffix(control))
        }
        Control::Undo => {
            if state.undo {
                format!("Undo{}", suffix(control))
            } else {
                format!("Nothing to undo{}", suffix(control))
            }
        }
        Control::Redo => {
            if state.redo {
                format!("Redo{}", suffix(control))
            } else {
                format!("Nothing to redo{}", suffix(control))
            }
        }
        Control::Pointer => format!(
            "Cursor in screenshot: {}{}",
            if state.pointer { "on" } else { "off" },
            suffix(control),
        ),
        Control::Axes => format!(
            "Axes: {}{}",
            if state.axes_enabled { "on" } else { "off" },
            suffix(control),
        ),
        Control::Copy => format!("Copy{}", suffix(control)),
        Control::Save => {
            if state.error {
                ERROR_TEXT.into()
            } else {
                format!("Save{}", suffix(control))
            }
        }
        Control::Color(color) => {
            format!(
                "#{:02x}{:02x}{:02x}{}",
                color.r,
                color.g,
                color.b,
                suffix(control)
            )
        }
    }
}

/// Rasterize an icon into a `px`-square A8 alpha mask surface, cached forever.
fn icon_mask(index: usize, px: u32) -> Result<ImageSurface> {
    let key = (index, px);
    if let Some(surface) = ICON_MASKS.with(|masks| masks.borrow().get(&key).cloned()) {
        return Ok(surface);
    }
    let tree = resvg::usvg::Tree::from_data(ICONS[index], &resvg::usvg::Options::default())
        .context("failed to parse toolbar icon SVG")?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(px, px).context("failed to allocate icon pixmap")?;
    let size = tree.size();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            px as f32 / size.width(),
            px as f32 / size.height(),
        ),
        &mut pixmap.as_mut(),
    );
    // The icons are monochrome `currentColor` art: keep only the alpha channel.
    // A8 strides are 4-byte aligned, so pad each row to the stride.
    let stride = (px + 3) & !3;
    let mut data = vec![0u8; stride as usize * px as usize];
    for (row, rgba_row) in pixmap.data().chunks_exact(px as usize * 4).enumerate() {
        for (x, rgba) in rgba_row.chunks_exact(4).enumerate() {
            data[row * stride as usize + x] = rgba[3];
        }
    }
    let surface = ImageSurface::create_for_data(
        data,
        cairo::Format::A8,
        px as i32,
        px as i32,
        stride as i32,
    )?;
    ICON_MASKS.with(|masks| masks.borrow_mut().insert(key, surface.clone()));
    Ok(surface)
}

pub struct Toolbar {
    state: ToolbarState,
    tip: Tip,
    size: Size<i32, Physical>,
    scale: f64,
    origin: Point<f64, Logical>,
    buffer: TextureBuffer<GlesTexture>,
}

impl Toolbar {
    pub fn render(
        cache: &RefCell<Option<Self>>,
        renderer: &mut GlesRenderer,
        state: ToolbarState,
        editor: &Editor,
        size: Size<i32, Physical>,
        scale: f64,
        alpha: f32,
    ) -> Result<PrimaryGpuTextureRenderElement> {
        // The tooltip contributes pixels only when the element is opaque; while
        // fading it collapses to `Tip::None` so the same texture is reused.
        let tip = if alpha >= 1. {
            match state.focus.or(state.hover) {
                Some(control) if editor.settings.tooltips.enabled => Tip::Control(control),
                _ if state.error => Tip::Error,
                _ => Tip::None,
            }
        } else {
            Tip::None
        };
        // Rasterization keys off state, tooltip, geometry and the settings
        // revision; `alpha` stays a pure render-element property so a fade
        // alone never rerasterizes.
        let mut cache = cache.borrow_mut();
        let stale = cache.as_ref().is_none_or(|toolbar| {
            toolbar.state != state
                || toolbar.tip != tip
                || toolbar.size != size
                || toolbar.scale != scale
        });
        if stale {
            *cache = Some(Self::rasterize(renderer, state, tip, editor, size, scale)?);
        }
        let toolbar = cache.as_ref().unwrap();
        Ok(PrimaryGpuTextureRenderElement(
            TextureRenderElement::from_texture_buffer(
                toolbar.buffer.clone(),
                toolbar.origin,
                alpha,
                None,
                None,
                Kind::Unspecified,
            ),
        ))
    }

    fn rasterize(
        renderer: &mut GlesRenderer,
        state: ToolbarState,
        tip: Tip,
        editor: &Editor,
        size: Size<i32, Physical>,
        scale: f64,
    ) -> Result<Self> {
        let settings = &editor.settings;
        let theme = &settings.toolbar;
        let margin = &theme.margin;
        let (mt, mr, mb, ml) = (margin.top, margin.right, margin.bottom, margin.left);
        let bar = bar_layout(size, scale, settings);
        let pop = state
            .palette_open
            .then(|| pop_layout(size, scale, settings, state.tool, &bar));
        let width = f64::from(size.w) / scale;
        let height = f64::from(size.h) / scale;

        // Measure the tooltip text on a scratch surface; the chip rect feeds
        // into the unioned raster bounds.
        let measure = ImageSurface::create(cairo::Format::ARgb32, 1, 1)?;
        let mcr = cairo::Context::new(&measure)?;
        let measure_layout = pangocairo::functions::create_layout(&mcr);
        let mut font = pango::FontDescription::from_string(&settings.tooltips.font);
        font.set_absolute_size(settings.tooltips.font_size * scale * f64::from(pango::SCALE));
        measure_layout.set_font_description(Some(&font));
        // Width labels on the colored tool buttons reuse the tooltip face at
        // a reduced size that follows the bar's shrink factor.
        let number_layout = pangocairo::functions::create_layout(&mcr);
        let mut number_font = font.clone();
        number_font.set_absolute_size(
            (settings.tooltips.font_size * 0.62 * bar.k).max(6.)
                * scale
                * f64::from(pango::SCALE),
        );
        number_layout.set_font_description(Some(&number_font));
        let (tip_text, tip_anchor) = match tip {
            Tip::None => (None, None),
            Tip::Error => (Some(ERROR_TEXT.to_string()), None),
            Tip::Control(control) => (Some(tooltip_text(&state, editor, control)), Some(control)),
        };
        let mut tip_box = None;
        if let Some(text) = &tip_text {
            measure_layout.set_text(text);
            let (tw, th) = measure_layout.pixel_size();
            let w = f64::from(tw) / scale + 14.;
            let h = f64::from(th) / scale + 8.;
            // The strip opens toward the screen center, below the lowest
            // top-side element or above the highest bottom-side one.
            let top_edge = pop.as_ref().map_or(bar.y, |pop| pop.y.min(bar.y));
            let bot_edge = pop
                .as_ref()
                .map_or(bar.y + bar.h, |pop| (pop.y + pop.h).max(bar.y + bar.h));
            let strip_y = match settings.toolbar.position {
                ScreenshotToolbarPosition::Top => bot_edge + GAP,
                ScreenshotToolbarPosition::Bottom => top_edge - GAP - h,
            }
            .clamp(mt, (height - h - mb).max(mt));
            let x = match tip_anchor {
                Some(control) => {
                    let rect = control_rect(
                        control_index(control, settings, state.tool),
                        size,
                        scale,
                        settings,
                        state.tool,
                    );
                    let cx = (f64::from(rect.loc.x) + f64::from(rect.size.w) / 2.) / scale;
                    (cx - w / 2.).clamp(ml, (width - w - mr).max(ml))
                }
                None => (ml + (width - ml - mr - w) / 2.).clamp(ml, (width - w - mr).max(ml)),
            };
            tip_box = Some((x, strip_y, w, h, f64::from(tw), f64::from(th)));
        }

        // Union of everything drawn, expanded by the border stroke.
        let spill = theme.border_width / 2. + 2.;
        let (mut x1, mut y1) = (bar.x - spill, bar.y - spill);
        let (mut x2, mut y2) = (bar.x + bar.w + spill, bar.y + bar.h + spill);
        if let Some(pop) = &pop {
            x1 = x1.min(pop.x - spill);
            y1 = y1.min(pop.y - spill);
            x2 = x2.max(pop.x + pop.w + spill);
            y2 = y2.max(pop.y + pop.h + spill);
        }
        if let Some((x, y, w, h, ..)) = tip_box {
            x1 = x1.min(x - spill);
            y1 = y1.min(y - spill);
            x2 = x2.max(x + w + spill);
            y2 = y2.max(y + h + spill);
        }
        let origin_x = (x1 * scale).floor();
        let origin_y = (y1 * scale).floor();
        let surf_w = ((x2 * scale).ceil() - origin_x).max(1.) as i32;
        let surf_h = ((y2 * scale).ceil() - origin_y).max(1.) as i32;

        let mut surface = ImageSurface::create(cairo::Format::ARgb32, surf_w, surf_h)?;
        let cr = cairo::Context::new(&surface)?;
        // All coordinates below are logical units converted to device space.
        let phys = |x: f64, y: f64| (x * scale - origin_x, y * scale - origin_y);

        let fill = |x: f64, y: f64, w: f64, h: f64, rgba: (f64, f64, f64, f64)| -> Result<()> {
            let (px, py) = phys(x, y);
            cr.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
            cr.rectangle(px, py, w * scale, h * scale);
            cr.fill()?;
            Ok(())
        };
        let stroke =
            |x: f64, y: f64, w: f64, h: f64, lw: f64, rgba: (f64, f64, f64, f64)| -> Result<()> {
                let lw = (lw * scale).round().max(1.);
                let (px, py) = phys(x, y);
                cr.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
                cr.set_line_width(lw);
                cr.rectangle(px + lw / 2., py + lw / 2., w * scale - lw, h * scale - lw);
                cr.stroke()?;
                Ok(())
            };

        let border = theme.border_width;
        // Panel fill and stroke share the theme with the popover and tooltips;
        // the whole-element alpha applies `opacity`/`drag-opacity`.
        fill(bar.x, bar.y, bar.w, bar.h, rgba(theme.background_color))?;
        if border > 0. {
            stroke(bar.x, bar.y, bar.w, bar.h, border, rgba(theme.border_color))?;
        }

        for (i, control) in CONTROLS.iter().enumerate() {
            let enabled = state.enabled(*control);
            let hovered = state.hover == Some(*control);
            let pressed = state.pressed == Some(*control);
            let focused = state.focus == Some(*control);
            let bx = bar.x + bar.pad + bar.xs[i];
            let by = bar.y + bar.pad;
            let cell = bar.cell;
            let stripe = STRIPE * bar.k;

            if pressed {
                fill(bx, by, cell, cell, rgba(theme.pressed_color))?;
            } else if hovered {
                fill(bx, by, cell, cell, rgba(theme.hover_color))?;
            }
            if *control == Control::Save {
                // Subtle emphasis for the primary action.
                let mut emphasis = rgba(theme.accent_color);
                emphasis.3 *= if hovered || pressed { 0.16 } else { 0.10 };
                fill(bx, by, cell, cell, emphasis)?;
            }
            let toggled = (*control == Control::Pointer && state.pointer)
                || (*control == Control::Axes && state.axes_enabled);
            if toggled {
                // Active toggles: neutral icon, accent bottom stripe.
                let (px, py) = phys(bx, by + cell - stripe);
                let accent = rgba(theme.accent_color);
                cr.set_source_rgba(accent.0, accent.1, accent.2, accent.3 * 0.9);
                cr.rectangle(px, py, cell * scale, stripe * scale);
                cr.fill()?;
            }
            if let Control::Tool(tool) = *control {
                if let Some(index) = colored_tool(tool).map(color_index) {
                    // The tool's own color stripe, same geometry as the
                    // toggle one.
                    let (r, g, b) = state.colors[index].rgb();
                    fill(bx, by + cell - stripe, cell, stripe, (r, g, b, 1.))?;
                }
            }
            if *control == Control::Tool(ScreenshotTool::Blur) {
                // Three bottom segments fill from left to right.
                let filled = strength_index(state.blur_strength) + 1;
                let gap = bar.k;
                let seg = (cell - 2. * gap) / 3.;
                let accent = rgba(theme.accent_color);
                let ink = rgba(theme.icon_color);
                for s in 0..3 {
                    let on = s < filled;
                    let (r, g, b, a) = if on { accent } else { ink };
                    let alpha = if on { a * 0.9 } else { a * 0.25 };
                    let sx = bx + s as f64 * (seg + gap);
                    fill(sx, by + cell - stripe, seg, stripe, (r, g, b, alpha))?;
                }
            }

            let icon_logical = theme.icon_size * bar.k;
            let icon_px = (icon_logical * scale).round().max(1.) as u32;
            let ix = bx + (cell - icon_logical) / 2.;
            let iy = by + (cell - icon_logical) / 2.;
            let (r, g, b, a) = if !enabled {
                rgba(theme.disabled_icon_color)
            } else if matches!(control, Control::Tool(tool) if *tool == state.tool) {
                // Active tool tints the icon only, no stripe.
                rgba(theme.accent_color)
            } else if *control == Control::Save && state.error {
                (1., 0.42, 0.42, 1.)
            } else {
                rgba(theme.icon_color)
            };
            let mask = icon_mask(i, icon_px)?;
            let (mx, my) = phys(ix, iy);
            cr.set_source_rgba(r, g, b, a);
            cr.mask_surface(&mask, mx, my)?;

            if let Control::Tool(tool) = *control {
                if colored_tool(tool).is_some() {
                    // The tool's own width as small text in the top-right
                    // corner of its button.
                    let label = state.widths[color_index(tool)].to_string();
                    number_layout.set_text(&label);
                    let (tw, _th) = number_layout.pixel_size();
                    let (tx, ty) = phys(bx + cell - 2., by + 1.);
                    let ink = if enabled {
                        rgba(theme.icon_color)
                    } else {
                        rgba(theme.disabled_icon_color)
                    };
                    cr.set_source_rgba(ink.0, ink.1, ink.2, ink.3 * 0.8);
                    cr.move_to(tx - f64::from(tw), ty);
                    pangocairo::functions::show_layout(&cr, &number_layout);
                }
            }
            if focused {
                let accent = rgba(theme.accent_color);
                stroke(
                    bx + 1.5,
                    by + 1.5,
                    cell - 3.,
                    cell - 3.,
                    1.5,
                    (accent.0, accent.1, accent.2, accent.3 * 0.9),
                )?;
            }
        }

        if let Some(pop) = &pop {
            fill(pop.x, pop.y, pop.w, pop.h, rgba(theme.background_color))?;
            if border > 0. {
                stroke(pop.x, pop.y, pop.w, pop.h, border, rgba(theme.border_color))?;
            }
            for (i, swatch) in tool_palette(settings, state.tool).iter().enumerate() {
                let color = rgb8(*swatch);
                let control = Control::Color(color);
                let hovered = state.hover == Some(control);
                let pressed = state.pressed == Some(control);
                let focused = state.focus == Some(control);
                let selected = color == state.colors[color_index(state.tool)];
                let enabled = state.enabled(control);
                let cx = pop.x + pop.pad + (i % POP_COLS) as f64 * pop.cell;
                let cy = pop.y + pop.pad + (i / POP_COLS) as f64 * pop.cell;

                if pressed {
                    fill(cx, cy, pop.cell, pop.cell, rgba(theme.pressed_color))?;
                } else if hovered {
                    fill(cx, cy, pop.cell, pop.cell, rgba(theme.hover_color))?;
                }
                let inset = 2.;
                let (r, g, b) = color.rgb();
                let alpha = if enabled { 1. } else { 0.35 };
                fill(
                    cx + inset,
                    cy + inset,
                    pop.cell - 2. * inset,
                    pop.cell - 2. * inset,
                    (r, g, b, alpha),
                )?;
                if selected {
                    // Border plus a check picked for contrast on the swatch.
                    let dark = 0.299 * r + 0.587 * g + 0.114 * b > 0.6;
                    let ink = if dark {
                        (0.05, 0.05, 0.08)
                    } else {
                        (1., 1., 1.)
                    };
                    stroke(
                        cx + inset,
                        cy + inset,
                        pop.cell - 2. * inset,
                        pop.cell - 2. * inset,
                        1.5,
                        (ink.0, ink.1, ink.2, 0.9 * alpha),
                    )?;
                    let (ax, ay) = phys(cx, cy);
                    let s = pop.cell * scale;
                    cr.set_source_rgba(ink.0, ink.1, ink.2, alpha);
                    cr.set_line_width((2. * scale).round().max(1.5));
                    cr.set_line_cap(cairo::LineCap::Round);
                    cr.move_to(ax + 0.30 * s, ay + 0.55 * s);
                    cr.line_to(ax + 0.45 * s, ay + 0.72 * s);
                    cr.line_to(ax + 0.74 * s, ay + 0.32 * s);
                    cr.stroke()?;
                }
                if focused {
                    let accent = rgba(theme.accent_color);
                    stroke(
                        cx + inset,
                        cy + inset,
                        pop.cell - 2. * inset,
                        pop.cell - 2. * inset,
                        1.5,
                        (accent.0, accent.1, accent.2, accent.3 * 0.9),
                    )?;
                }
            }
        }

        if let (Some(text), Some((x, y, w, h, tw, th))) = (&tip_text, tip_box) {
            fill(x, y, w, h, rgba(theme.background_color))?;
            if border > 0. {
                stroke(x, y, w, h, border, rgba(theme.border_color))?;
            }
            let layout = pangocairo::functions::create_layout(&cr);
            layout.set_font_description(Some(&font));
            layout.set_text(text);
            let (px, py) = phys(x, y);
            if tip == Tip::Error {
                cr.set_source_rgba(1., 0.45, 0.45, 0.95);
            } else {
                let ink = rgba(theme.icon_color);
                cr.set_source_rgba(ink.0, ink.1, ink.2, ink.3 * 0.9);
            }
            cr.move_to(px + (w * scale - tw) / 2., py + (h * scale - th) / 2.);
            pangocairo::functions::show_layout(&cr, &layout);
        }

        drop(cr);
        let pixels = surface.data()?;
        let buffer = TextureBuffer::from_memory(
            renderer,
            &pixels,
            Fourcc::Argb8888,
            (surf_w, surf_h),
            false,
            scale,
            Transform::Normal,
            Vec::new(),
        )?;
        drop(pixels);
        Ok(Self {
            state,
            tip,
            size,
            scale,
            origin: Point::from((origin_x / scale, origin_y / scale)),
            buffer,
        })
    }
}

#[cfg(test)]
mod tests {
    use niri_ipc::ScreenshotValueChange;

    use super::*;

    fn size() -> Size<i32, Physical> {
        Size::from((1920, 1080))
    }

    #[test]
    fn default_bar_hit_test_matches_rendered_rect() {
        let settings = ScreenshotConfig::default();
        for (i, control) in CONTROLS.iter().enumerate() {
            let rect = control_rect(i, size(), 1., &settings, ScreenshotTool::Draw);
            let center = rect.loc + rect.size.to_point().downscale(2);
            assert_eq!(
                hit_control(center, size(), 1., false, &settings, ScreenshotTool::Draw),
                Some(*control)
            );
            assert!(overlay_contains(center, size(), 1., false, &settings, ScreenshotTool::Draw));
        }
    }

    #[test]
    fn bar_respects_position_align_and_margins() {
        let mut settings = ScreenshotConfig::default();
        // Asymmetric margins shift the center.
        settings.toolbar.margin.left = 100.;
        settings.toolbar.margin.right = 20.;

        let top = control_rect(0, size(), 1., &settings, ScreenshotTool::Draw);
        // Button sits pad-inside the bar; margin 8 + pad 4.
        assert_eq!(top.loc.y, 8 + 4);

        settings.toolbar.position = ScreenshotToolbarPosition::Bottom;
        let top = control_rect(0, size(), 1., &settings, ScreenshotTool::Draw);
        assert_eq!(top.loc.y + top.size.h, 1080 - 8 - 4);

        settings.toolbar.align = ScreenshotToolbarAlign::Left;
        let top = control_rect(0, size(), 1., &settings, ScreenshotTool::Draw);
        assert_eq!(top.loc.x, 100 + 4);

        settings.toolbar.align = ScreenshotToolbarAlign::Right;
        let last = control_rect(CONTROLS.len() - 1, size(), 1., &settings, ScreenshotTool::Draw);
        assert_eq!(last.loc.x + last.size.w, 1920 - 20 - 4);
    }

    #[test]
    fn palette_switches_hit_testing_when_open() {
        let settings = ScreenshotConfig::default();
        let swatch = control_rect(CONTROLS.len(), size(), 1., &settings, ScreenshotTool::Draw);
        let center = swatch.loc + swatch.size.to_point().downscale(2);
        // Closed palette never reports a swatch even over its would-be rect.
        assert!(!matches!(
            hit_control(center, size(), 1., false, &settings, ScreenshotTool::Draw),
            Some(Control::Color(_))
        ));
        assert!(overlay_contains(center, size(), 1., true, &settings, ScreenshotTool::Draw));
        assert!(matches!(
            hit_control(center, size(), 1., true, &settings, ScreenshotTool::Draw),
            Some(Control::Color(_))
        ));
    }

    #[test]
    fn popover_wraps_after_four_columns() {
        let settings = ScreenshotConfig::default();
        let first = control_rect(CONTROLS.len(), size(), 1., &settings, ScreenshotTool::Draw);
        let fifth = control_rect(CONTROLS.len() + 4, size(), 1., &settings, ScreenshotTool::Draw);
        assert_eq!(first.loc.x, fifth.loc.x);
        assert_eq!(fifth.loc.y, first.loc.y + first.size.h);

        // One-swatch palette is a single cell wide.
        let mut settings = ScreenshotConfig::default();
        settings.brush.palette.truncate(1);
        let single = control_rect(CONTROLS.len(), size(), 1., &settings, ScreenshotTool::Draw);
        assert_eq!(single.size, first.size);
        // A swatch index outside the palette produces an empty rect.
        let outside = control_rect(CONTROLS.len() + 1, size(), 1., &settings, ScreenshotTool::Draw);
        assert_eq!(outside.size, Size::from((0, 0)));
    }

    #[test]
    fn magnifier_zoom_steps_and_bounds() {
        let settings = ScreenshotConfig::default();
        let mut editor = Editor::new(settings, ScreenshotTool::Select);
        assert_eq!(editor.magnifier_zoom.level(), 1);

        assert!(editor
            .magnifier_zoom
            .change(ScreenshotValueChange::Adjust(1)));
        assert_eq!(editor.magnifier_zoom.level(), 2);
        // A large step clamps at the 32x maximum.
        for _ in 0..8 {
            editor
                .magnifier_zoom
                .change(ScreenshotValueChange::Adjust(2));
        }
        assert_eq!(editor.magnifier_zoom.level(), 32);
        // No change at the bound.
        assert!(!editor
            .magnifier_zoom
            .change(ScreenshotValueChange::Adjust(1)));

        editor.magnifier_zoom.change(ScreenshotValueChange::Set(4));
        assert_eq!(editor.magnifier_zoom.level(), 4);
        // Absolute values clamp to the configured range.
        editor.magnifier_zoom.change(ScreenshotValueChange::Set(64));
        assert_eq!(editor.magnifier_zoom.level(), 32);
        editor.magnifier_zoom.change(ScreenshotValueChange::Set(0));
        assert_eq!(editor.magnifier_zoom.level(), 1);

        // A lower max-zoom on reload clamps the live zoom.
        editor.magnifier_zoom.change(ScreenshotValueChange::Set(16));
        editor.magnifier_zoom.set_max_zoom(4);
        assert_eq!(editor.magnifier_zoom.level(), 4);
    }

    #[test]
    fn apply_settings_preserves_session_and_fixes_swatch_focus() {
        let settings = ScreenshotConfig::default();
        let palette_color = rgb8(settings.brush.palette[0]);
        let mut editor = Editor::new(settings.clone(), ScreenshotTool::Draw);
        editor.color = ScreenshotColor::new(1, 2, 3);
        editor.width = 17;
        // Focus a swatch color that is not in the default palette.
        editor.focus = Some(Control::Color(editor.color));

        let mut updated = settings;
        updated.toolbar.position = ScreenshotToolbarPosition::Bottom;
        editor.apply_settings(updated);

        // Session state survives; the swatch missing from the palette drops
        // focus back to the brush button.
        assert_eq!(editor.color, ScreenshotColor::new(1, 2, 3));
        assert_eq!(editor.width, 17);
        assert_eq!(editor.tool, ScreenshotTool::Draw);
        assert_eq!(editor.focus, Some(Control::Tool(ScreenshotTool::Draw)));

        // A focused swatch still present in the palette keeps its focus.
        editor.focus = Some(Control::Color(palette_color));
        let mut updated = editor.settings.clone();
        updated.toolbar.align = ScreenshotToolbarAlign::Left;
        editor.apply_settings(updated);
        assert_eq!(editor.focus, Some(Control::Color(palette_color)));
    }
}
