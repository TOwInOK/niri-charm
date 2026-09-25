use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use anyhow::{Context as _, Result};
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

const WIDTH: f64 = 4.;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Draw,
    Blur,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    #[default]
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    White,
    Black,
}

impl Color {
    pub fn rgb(self) -> (f64, f64, f64) {
        let (r, g, b) = match self {
            Self::Red => (255, 0, 0),
            Self::Orange => (255, 128, 0),
            Self::Yellow => (255, 224, 0),
            Self::Green => (0, 192, 64),
            Self::Blue => (0, 128, 255),
            Self::White => (255, 255, 255),
            Self::Black => (0, 0, 0),
        };
        (
            f64::from(r) / 255.,
            f64::from(g) / 255.,
            f64::from(b) / 255.,
        )
    }
}

pub enum Edit {
    Stroke(Stroke),
    Blur(Rectangle<i32, Physical>),
}

pub struct Stroke {
    points: Vec<Point<i32, Physical>>,
    color: Color,
    bounds: Rectangle<i32, Physical>,
}

pub struct Command {
    pub output: Output,
    pub edit: Edit,
}

pub enum Gesture {
    Canvas {
        output: Output,
        slot: Option<TouchSlot>,
        origin: Point<i32, Physical>,
        edit: Edit,
    },
    Toolbar {
        output: Output,
        slot: Option<TouchSlot>,
        control: Control,
    },
    Overlay {
        output: Output,
        slot: Option<TouchSlot>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Tool(Tool),
    Color(Color),
    Copy,
    Undo,
    Redo,
    Pointer,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOutcome {
    Changed,
    Save,
    Copy,
}

#[derive(Default)]
pub struct Editor {
    pub tool: Tool,
    pub color: Color,
    pub gesture: Option<Gesture>,
    pub pointer: Option<(Output, Point<i32, Physical>)>,
    pub history: Vec<Command>,
    pub redo: Vec<Command>,
    pub revision: u64,
    pub magnifier_zoom: MagnifierZoom,
    pub palette_open: bool,
    pub focus: Option<Control>,
    pub space_down: bool,
}

pub struct MagnifierZoom {
    exponent: i16,
    scroll: crate::input::scroll_tracker::ScrollTracker,
}

impl Default for MagnifierZoom {
    fn default() -> Self {
        Self {
            exponent: 0,
            scroll: crate::input::scroll_tracker::ScrollTracker::new(120),
        }
    }
}

impl MagnifierZoom {
    pub fn level(&self) -> u32 {
        1 << self.exponent
    }

    pub fn scroll(&mut self, amount: f64) -> bool {
        let next = (self.exponent - i16::from(self.scroll.accumulate(amount))).clamp(0, 5);
        let changed = next != self.exponent;
        self.exponent = next;
        changed
    }
}

impl Editor {
    pub fn edits<'a>(&'a self, output: &'a Output) -> impl Iterator<Item = &'a Edit> {
        self.history
            .iter()
            .filter(move |c| &c.output == output)
            .map(|c| &c.edit)
            .chain(
                self.gesture
                    .iter()
                    .filter_map(move |gesture| match gesture {
                        Gesture::Canvas {
                            output: owner,
                            edit,
                            ..
                        } if owner == output => Some(edit),
                        _ => None,
                    }),
            )
    }

    pub fn begin(&mut self, output: Output, point: Point<i32, Physical>, slot: Option<TouchSlot>) {
        let edit = match self.tool {
            Tool::Select => return,
            Tool::Draw => Edit::Stroke(Stroke {
                points: vec![point],
                color: self.color,
                bounds: segment_bounds(point, point),
            }),
            Tool::Blur => Edit::Blur(rectangle(point, point)),
        };
        self.gesture = Some(Gesture::Canvas {
            output,
            slot,
            origin: point,
            edit,
        });
    }

    pub fn motion(
        &mut self,
        point: Point<i32, Physical>,
        slot: Option<TouchSlot>,
    ) -> Option<Rectangle<i32, Physical>> {
        let Gesture::Canvas {
            slot: owner,
            origin,
            edit,
            ..
        } = self.gesture.as_mut()?
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
                let damage = segment_bounds(previous, point);
                stroke.bounds = stroke.bounds.merge(damage);
                Some(damage)
            }
            Edit::Blur(rect) => {
                *rect = rectangle(*origin, point);
                None
            }
        }
    }
}

impl Edit {
    pub fn stroke_bounds(&self) -> Option<Rectangle<i32, Physical>> {
        match self {
            Self::Stroke(stroke) => Some(stroke.bounds),
            Self::Blur(_) => None,
        }
    }
}

pub fn rectangle(a: Point<i32, Physical>, b: Point<i32, Physical>) -> Rectangle<i32, Physical> {
    Rectangle::from_extremities(
        (a.x.min(b.x), a.y.min(b.y)),
        (a.x.max(b.x) + 1, a.y.max(b.y) + 1),
    )
}

fn segment_bounds(a: Point<i32, Physical>, b: Point<i32, Physical>) -> Rectangle<i32, Physical> {
    let rect = rectangle(a, b);
    let pad = WIDTH as i32 / 2 + 1;
    Rectangle::new(
        rect.loc - Point::from((pad, pad)),
        rect.size + Size::from((pad * 2, pad * 2)),
    )
}

#[derive(Default)]
pub struct Canvas {
    raster: RefCell<Option<Raster>>,
    damage: RefCell<Option<Rectangle<i32, Physical>>>,
    blurred: [OnceCell<TextureBuffer<GlesTexture>>; 3],
}

struct Raster {
    surface: ImageSurface,
    buffer: Option<TextureBuffer<GlesTexture>>,
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
        cr.set_line_width(WIDTH);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        for edit in editor.edits(output) {
            let Edit::Stroke(stroke) = edit else {
                continue;
            };
            if !stroke.bounds.overlaps(damage) {
                continue;
            }
            let (r, g, b) = stroke.color.rgb();
            cr.set_source_rgb(r, g, b);
            if stroke.points.len() == 1 {
                let p = stroke.points[0];
                cr.arc(
                    f64::from(p.x) + 0.5,
                    f64::from(p.y) + 0.5,
                    WIDTH / 2.,
                    0.,
                    std::f64::consts::TAU,
                );
                cr.fill()?;
            } else {
                // Only submit segments that can affect the dirty area, including their round caps.
                for pair in stroke.points.windows(2) {
                    if segment_bounds(pair[0], pair[1]).overlaps(damage) {
                        cr.move_to(f64::from(pair[0].x) + 0.5, f64::from(pair[0].y) + 0.5);
                        cr.line_to(f64::from(pair[1].x) + 0.5, f64::from(pair[1].y) + 0.5);
                    }
                }
                cr.stroke()?;
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
        edits: impl IntoIterator<Item = &'a Edit>,
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
        for edit in edits {
            let Edit::Blur(rect) = edit else {
                continue;
            };
            let blurred = match self.blurred[target].get() {
                Some(buffer) => buffer,
                None => {
                    let mut blur =
                        Blur::new(renderer).context("screenshot blur shader is unavailable")?;
                    let options = BlurOptions {
                        passes: 3,
                        offset: 3.,
                    };
                    blur.prepare_textures(
                        |format, size| renderer.create_buffer(format, size),
                        source,
                        options,
                    )?;
                    let texture = blur.render(renderer, source, options)?;
                    let buffer = TextureBuffer::from_texture(
                        renderer,
                        texture,
                        scale,
                        Transform::Normal,
                        Vec::new(),
                    );
                    let _ = self.blurred[target].set(buffer);
                    self.blurred[target].get().unwrap()
                }
            };
            let rect = rect.to_f64().to_logical(scale);
            push(element(
                blurred.clone(),
                rect.loc,
                Some(rect),
                Some(rect.size),
            ));
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

pub const CONTROLS: [Control; 8] = [
    Control::Tool(Tool::Select),
    Control::Tool(Tool::Draw),
    Control::Tool(Tool::Blur),
    Control::Undo,
    Control::Redo,
    Control::Pointer,
    Control::Copy,
    Control::Save,
];

pub const COLORS: [Color; 7] = [
    Color::Red,
    Color::Orange,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::White,
    Color::Black,
];

/// Toolbar icon SVGs in `CONTROLS` order. Each is rasterized once per pixel
/// size into a cached alpha mask and tinted by cairo, so pointer motion never
/// reparses or rerasterizes SVG.
const ICONS: [&[u8]; 8] = [
    include_bytes!("../../resources/icons/selection.svg"),
    include_bytes!("../../resources/icons/brush.svg"),
    include_bytes!("../../resources/icons/blur.svg"),
    include_bytes!("../../resources/icons/undo.svg"),
    include_bytes!("../../resources/icons/redo.svg"),
    include_bytes!("../../resources/icons/cursor.svg"),
    include_bytes!("../../resources/icons/copy.svg"),
    include_bytes!("../../resources/icons/save.svg"),
];

// Logical layout constants before the uniform narrow-output shrink factor.
const BAR_TOP: f64 = 8.;
const CELL: f64 = 40.;
const PAD: f64 = 4.;
const GROUP_GAP: f64 = 12.;
const ICON_SIZE: f64 = 24.;
const POP_CELL: f64 = 28.;
const POP_PAD: f64 = 6.;
const MARGIN: f64 = 8.;
const STRIPE: f64 = 3.;
const ACCENT: (f64, f64, f64) = (0.50, 0.78, 1.00);
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
    // [tools] [undo, redo] [pointer] [copy, save]
    matches!(index, 3 | 5 | 6)
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
    xs: [f64; 8],
}

fn bar_layout(size: Size<i32, Physical>, scale: f64) -> BarLayout {
    let width = f64::from(size.w) / scale;
    // Shrink uniformly on narrow outputs; the bar never wraps to a second row.
    let content = CONTROLS.len() as f64 * CELL + 3. * GROUP_GAP + 2. * PAD;
    let k = ((width - 2. * MARGIN) / content).clamp(0.05, 1.);
    let cell = CELL * k;
    let pad = PAD * k;
    let gap = GROUP_GAP * k;
    let mut xs = [0.; 8];
    let mut x = 0.;
    for (i, slot) in xs.iter_mut().enumerate() {
        if group_before(i) {
            x += gap;
        }
        *slot = x;
        x += cell;
    }
    BarLayout {
        x: ((width - (x + 2. * pad)) / 2.).max(0.),
        y: BAR_TOP,
        w: x + 2. * pad,
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

fn pop_layout(size: Size<i32, Physical>, scale: f64, bar: &BarLayout) -> PopLayout {
    let width = f64::from(size.w) / scale;
    let height = f64::from(size.h) / scale;
    let k =
        ((width - 2. * MARGIN) / (COLORS.len() as f64 * POP_CELL + 2. * POP_PAD)).clamp(0.1, 1.);
    let cell = POP_CELL * k;
    let pad = POP_PAD * k;
    let w = COLORS.len() as f64 * cell + 2. * pad;
    let h = cell + 2. * pad;
    // Anchor under the brush button, clamped to the output margins.
    let palette_cx = bar.x + bar.pad + bar.xs[1] + bar.cell / 2.;
    let x = (palette_cx - w / 2.).clamp(MARGIN, (width - w - MARGIN).max(MARGIN));
    let y = (bar.y + bar.h + 4.).clamp(MARGIN, (height - h - MARGIN).max(MARGIN));
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
/// `COLORS[i]` (geometry is defined whether or not the popover is open).
pub fn control_rect(
    index: usize,
    size: Size<i32, Physical>,
    scale: f64,
) -> Rectangle<i32, Physical> {
    let bar = bar_layout(size, scale);
    if index < CONTROLS.len() {
        phys_rect(
            bar.x + bar.pad + bar.xs[index],
            bar.y + bar.pad,
            bar.cell,
            bar.cell,
            scale,
        )
    } else if index < CONTROLS.len() + COLORS.len() {
        let pop = pop_layout(size, scale, &bar);
        phys_rect(
            pop.x + pop.pad + (index - CONTROLS.len()) as f64 * pop.cell,
            pop.y + pop.pad,
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
) -> Option<Control> {
    if palette_open {
        for (i, color) in COLORS.iter().enumerate() {
            if control_rect(CONTROLS.len() + i, size, scale).contains(point) {
                return Some(Control::Color(*color));
            }
        }
    }
    CONTROLS.iter().enumerate().find_map(|(index, control)| {
        control_rect(index, size, scale)
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
) -> bool {
    let bar = bar_layout(size, scale);
    if phys_rect(bar.x, bar.y, bar.w, bar.h, scale).contains(point) {
        return true;
    }
    if palette_open {
        let pop = pop_layout(size, scale, &bar);
        if phys_rect(pop.x, pop.y, pop.w, pop.h, scale).contains(point) {
            return true;
        }
    }
    false
}

pub fn control_enabled(control: Control, editor: &Editor) -> bool {
    match control {
        // The palette and its colors only make sense for the draw tool.
        Control::Color(_) => editor.tool == Tool::Draw,
        // An in-progress canvas gesture can be cancelled by undo.
        Control::Undo => {
            !editor.history.is_empty() || matches!(editor.gesture, Some(Gesture::Canvas { .. }))
        }
        Control::Redo => !editor.redo.is_empty(),
        _ => true,
    }
}

/// Combined bar + popover index for `control_rect`.
fn control_index(control: Control) -> usize {
    if let Some(index) = CONTROLS.iter().position(|c| *c == control) {
        index
    } else if let Control::Color(color) = control {
        CONTROLS.len() + COLORS.iter().position(|c| *c == color).unwrap_or(0)
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ToolbarState {
    tool: Tool,
    color: Color,
    palette_open: bool,
    focus: Option<Control>,
    hover: Option<Control>,
    pressed: Option<Control>,
    pointer: bool,
    error: bool,
    draw_tool: bool,
    undo: bool,
    redo: bool,
}

impl ToolbarState {
    pub fn new(editor: &Editor, pointer: bool, error: bool, hover: Option<Control>) -> Self {
        let pressed = match editor.gesture {
            Some(Gesture::Toolbar { control, .. }) => Some(control),
            _ => None,
        };
        Self {
            tool: editor.tool,
            color: editor.color,
            palette_open: editor.palette_open,
            focus: editor.focus,
            hover,
            pressed,
            pointer,
            error,
            draw_tool: editor.tool == Tool::Draw,
            undo: control_enabled(Control::Undo, editor),
            redo: control_enabled(Control::Redo, editor),
        }
    }

    /// Mirrors `control_enabled` on the captured flags.
    fn enabled(&self, control: Control) -> bool {
        match control {
            Control::Color(_) => self.draw_tool,
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

fn tooltip_text(state: &ToolbarState, control: Control) -> String {
    match control {
        Control::Tool(Tool::Select) => "Select · V".into(),
        Control::Tool(Tool::Draw) => "Draw · B / Palette · C".into(),
        Control::Tool(Tool::Blur) => "Blur · R".into(),
        Control::Undo => {
            if state.undo {
                "Undo · Ctrl+Z".into()
            } else {
                "Nothing to undo · Ctrl+Z".into()
            }
        }
        Control::Redo => {
            if state.redo {
                "Redo · Ctrl+Shift+Z".into()
            } else {
                "Nothing to redo · Ctrl+Shift+Z".into()
            }
        }
        Control::Pointer => format!(
            "Cursor in screenshot: {} · P",
            if state.pointer { "on" } else { "off" }
        ),
        Control::Copy => "Copy · Ctrl+C".into(),
        Control::Save => {
            if state.error {
                ERROR_TEXT.into()
            } else {
                "Save · Ctrl+S".into()
            }
        }
        Control::Color(color) => {
            let (name, key) = match color {
                Color::Red => ("Red", 1),
                Color::Orange => ("Orange", 2),
                Color::Yellow => ("Yellow", 3),
                Color::Green => ("Green", 4),
                Color::Blue => ("Blue", 5),
                Color::White => ("White", 6),
                Color::Black => ("Black", 7),
            };
            format!("{name} · {key}")
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
        size: Size<i32, Physical>,
        scale: f64,
        alpha: f32,
    ) -> Result<PrimaryGpuTextureRenderElement> {
        // The tooltip contributes pixels only when the element is opaque; while
        // fading it collapses to `Tip::None` so the same texture is reused.
        let tip = if alpha >= 1. {
            match state.focus.or(state.hover) {
                Some(control) => Tip::Control(control),
                None if state.error => Tip::Error,
                None => Tip::None,
            }
        } else {
            Tip::None
        };
        let mut cache = cache.borrow_mut();
        let stale = cache.as_ref().is_none_or(|toolbar| {
            toolbar.state != state
                || toolbar.tip != tip
                || toolbar.size != size
                || toolbar.scale != scale
        });
        if stale {
            *cache = Some(Self::rasterize(renderer, state, tip, size, scale)?);
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
        size: Size<i32, Physical>,
        scale: f64,
    ) -> Result<Self> {
        let bar = bar_layout(size, scale);
        let pop = state.palette_open.then(|| pop_layout(size, scale, &bar));
        let width = f64::from(size.w) / scale;

        // Measure the tooltip text on a scratch surface; the chip rect feeds
        // into the unioned raster bounds.
        let measure = ImageSurface::create(cairo::Format::ARgb32, 1, 1)?;
        let mcr = cairo::Context::new(&measure)?;
        let measure_layout = pangocairo::functions::create_layout(&mcr);
        let mut font = pango::FontDescription::from_string("sans");
        font.set_absolute_size(11. * scale * f64::from(pango::SCALE));
        measure_layout.set_font_description(Some(&font));
        let (tip_text, tip_anchor) = match tip {
            Tip::None => (None, None),
            Tip::Error => (Some(ERROR_TEXT.to_string()), None),
            Tip::Control(control) => (Some(tooltip_text(&state, control)), Some(control)),
        };
        let mut tip_box = None;
        if let Some(text) = &tip_text {
            measure_layout.set_text(text);
            let (tw, th) = measure_layout.pixel_size();
            let w = f64::from(tw) / scale + 14.;
            let h = f64::from(th) / scale + 8.;
            let strip_y = pop.as_ref().map_or(bar.y + bar.h, |pop| pop.y + pop.h) + 4.;
            let x = match tip_anchor {
                Some(control) => {
                    let rect = control_rect(control_index(control), size, scale);
                    let cx = (f64::from(rect.loc.x) + f64::from(rect.size.w) / 2.) / scale;
                    (cx - w / 2.).clamp(MARGIN, (width - w - MARGIN).max(MARGIN))
                }
                None => ((width - w) / 2.).max(MARGIN),
            };
            tip_box = Some((x, strip_y, w, h, f64::from(tw), f64::from(th)));
        }

        // Union of everything drawn, expanded by the border stroke.
        let (mut x1, mut y1) = (bar.x - 2., bar.y - 2.);
        let (mut x2, mut y2) = (bar.x + bar.w + 2., bar.y + bar.h + 2.);
        if let Some(pop) = &pop {
            x1 = x1.min(pop.x - 2.);
            y1 = y1.min(pop.y - 2.);
            x2 = x2.max(pop.x + pop.w + 2.);
            y2 = y2.max(pop.y + pop.h + 2.);
        }
        if let Some((x, y, w, h, ..)) = tip_box {
            x1 = x1.min(x - 2.);
            y1 = y1.min(y - 2.);
            x2 = x2.max(x + w + 2.);
            y2 = y2.max(y + h + 2.);
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

        // Rectangular dark bar with a thin border; opaque at rest, the whole
        // element alpha dims it to 0.5 during canvas gestures.
        fill(bar.x, bar.y, bar.w, bar.h, (0.07, 0.07, 0.10, 1.))?;
        stroke(bar.x, bar.y, bar.w, bar.h, 1., (1., 1., 1., 0.16))?;

        for (i, control) in CONTROLS.iter().enumerate() {
            let enabled = state.enabled(*control);
            let hovered = state.hover == Some(*control);
            let pressed = state.pressed == Some(*control);
            let focused = state.focus == Some(*control);
            let bx = bar.x + bar.pad + bar.xs[i];
            let by = bar.y + bar.pad;
            let cell = bar.cell;

            if pressed {
                fill(bx, by, cell, cell, (0., 0., 0., 0.35))?;
            } else if hovered {
                fill(bx, by, cell, cell, (1., 1., 1., 0.12))?;
            }
            if *control == Control::Save {
                // Subtle emphasis for the primary action.
                let alpha = if hovered || pressed { 0.16 } else { 0.10 };
                fill(bx, by, cell, cell, (ACCENT.0, ACCENT.1, ACCENT.2, alpha))?;
            }
            if *control == Control::Pointer && state.pointer {
                // Active pointer toggle: neutral icon, accent bottom stripe.
                let (px, py) = phys(bx, by + cell - STRIPE);
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.9);
                cr.rectangle(px, py, cell * scale, STRIPE * scale);
                cr.fill()?;
            }

            let icon_px = (ICON_SIZE * bar.k * scale).round().max(1.) as u32;
            let ix = bx + (cell - ICON_SIZE * bar.k) / 2.;
            let iy = by + (cell - ICON_SIZE * bar.k) / 2.;
            let (r, g, b, a) = if !enabled {
                (1., 1., 1., 0.30)
            } else if matches!(control, Control::Tool(tool) if *tool == state.tool) {
                // Active tool tints the icon only, no stripe.
                (ACCENT.0, ACCENT.1, ACCENT.2, 1.)
            } else if *control == Control::Save && state.error {
                (1., 0.42, 0.42, 1.)
            } else {
                (1., 1., 1., 0.92)
            };
            let mask = icon_mask(i, icon_px)?;
            let (mx, my) = phys(ix, iy);
            cr.set_source_rgba(r, g, b, a);
            cr.mask_surface(&mask, mx, my)?;

            if *control == Control::Tool(Tool::Draw) {
                let (r, g, b) = state.color.rgb();
                fill(bx, by + cell - STRIPE, cell, STRIPE, (r, g, b, 1.))?;
            }
            if focused {
                stroke(
                    bx + 1.5,
                    by + 1.5,
                    cell - 3.,
                    cell - 3.,
                    1.5,
                    (ACCENT.0, ACCENT.1, ACCENT.2, 0.9),
                )?;
            }
        }

        if let Some(pop) = &pop {
            fill(pop.x, pop.y, pop.w, pop.h, (0.07, 0.07, 0.10, 1.))?;
            stroke(pop.x, pop.y, pop.w, pop.h, 1., (1., 1., 1., 0.16))?;
            for (i, color) in COLORS.iter().enumerate() {
                let control = Control::Color(*color);
                let hovered = state.hover == Some(control);
                let pressed = state.pressed == Some(control);
                let focused = state.focus == Some(control);
                let selected = *color == state.color;
                let enabled = state.enabled(control);
                let cx = pop.x + pop.pad + i as f64 * pop.cell;
                let cy = pop.y + pop.pad;

                if pressed {
                    fill(cx, cy, pop.cell, pop.cell, (0., 0., 0., 0.35))?;
                } else if hovered {
                    fill(cx, cy, pop.cell, pop.cell, (1., 1., 1., 0.12))?;
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
                    stroke(
                        cx + inset,
                        cy + inset,
                        pop.cell - 2. * inset,
                        pop.cell - 2. * inset,
                        1.5,
                        (ACCENT.0, ACCENT.1, ACCENT.2, 0.9),
                    )?;
                }
            }
        }

        if let (Some(text), Some((x, y, w, h, tw, th))) = (&tip_text, tip_box) {
            fill(x, y, w, h, (0.07, 0.07, 0.10, 1.))?;
            stroke(x, y, w, h, 1., (1., 1., 1., 0.16))?;
            let layout = pangocairo::functions::create_layout(&cr);
            layout.set_font_description(Some(&font));
            layout.set_text(text);
            let (px, py) = phys(x, y);
            if tip == Tip::Error {
                cr.set_source_rgba(1., 0.45, 0.45, 0.95);
            } else {
                cr.set_source_rgba(1., 1., 1., 0.9);
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
