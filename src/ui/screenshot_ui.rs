use std::cell::RefCell;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::iter::zip;
use std::rc::Rc;

use anyhow::Context;
use niri_config::{Action, Config, ModKey};
use niri_ipc::{ScreenshotColor as Color, ScreenshotTool as Tool, SizeChange};
use smithay::backend::allocator::Fourcc;
use smithay::backend::input::{Keycode, TouchSlot};
use smithay::backend::renderer::element::utils::{Relocate, RelocateRenderElement};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture, Uniform};
use smithay::backend::renderer::{ExportMem, Texture as _};
use smithay::output::{Output, WeakOutput};
use smithay::utils::{Physical, Point, Rectangle, Scale, Size, Transform};

use super::screenshot_axes::Axes;
use super::screenshot_editor::{
    self, Canvas, Command, Control, Edit, Editor, EditorOutcome, Toolbar, ToolbarState,
};
use super::screenshot_guides::Guides;
use crate::animation::{Animation, Clock};
use crate::layout::floating::DIRECTIONAL_MOVE_PX;
use crate::niri_render_elements;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::sampling::SamplingRenderElement;
use crate::render_helpers::shader_element::ShaderRenderElement;
use crate::render_helpers::shaders::{ProgramType, Shaders};
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::render_helpers::{render_to_texture, RenderTarget};
use crate::utils::screenshot::ExportRequest;
use crate::utils::to_physical_precise_round;

#[derive(Debug, PartialEq)]
pub enum EditorCommand {
    Export(ExportRequest),
    Close,
}

type Selection = (Output, Point<i32, Physical>, Point<i32, Physical>);

enum ScreenshotContext {
    Tool(Tool),
    Canvas,
    Overlay,
}

// Ideally the screenshot UI should support cross-output selections. However, that poses some
// technical challenges when the outputs have different scales and such. So, this implementation
// allows only single-output selections for now.
//
// As a consequence of this, selection coordinates are in output-local coordinate space.
#[allow(clippy::large_enum_variant)]
pub enum ScreenshotUi {
    Closed {
        last_selection: Option<(WeakOutput, Rectangle<i32, Physical>)>,
        clock: Clock,
        config: Rc<RefCell<Config>>,
    },
    Open {
        selection: Selection,
        output_data: HashMap<Output, OutputData>,
        interaction: Interaction,
        editor: Editor,
        show_pointer: bool,
        open_anim: Animation,
        clock: Clock,
        config: Rc<RefCell<Config>>,
        path: Option<String>,
        pending_config: bool,
        mod_key: ModKey,
        hold_key: Option<Keycode>,
        axes: Axes,
    },
}

/// State for moving the selection (as opposed to just drawing).
pub struct MoveState {
    // Cursor offset from selection.1 when starting the move.
    pointer_offset: Point<i32, Physical>,
    // If the move is initiated by a touch, this is the slot. If `None`, the move was initiated by
    // holding the configured selection-move key.
    touch_slot: Option<TouchSlot>,
}

pub enum Interaction {
    Idle,
    Selection(SelectionDrag),
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
pub struct SelectionDrag {
    previous: Selection,
    touch_slot: Option<TouchSlot>,
    last_pos: (Output, Point<i32, Physical>),
    mode: SelectionMode,
}
enum SelectionMode {
    Select,
    Move(MoveState),
}

pub struct OutputData {
    size: Size<i32, Physical>,
    scale: f64,
    transform: Transform,
    // Output, screencast, screen capture.
    screenshot: [OutputScreenshot; 3],
    buffers: [SolidColorBuffer; 8],
    locations: [Point<i32, Physical>; 8],
    canvas: Canvas,
    toolbar: RefCell<Option<Toolbar>>,
    guides: RefCell<Option<Guides>>,
    magnifier: [RefCell<Option<Magnifier>>; 3],
    composition: RefCell<Vec<PrimaryGpuTextureRenderElement>>,
}

struct Magnifier {
    point: Point<i32, Physical>,
    revision: u64,
    show_pointer: bool,
    zoom: u32,
    lens: ShaderRenderElement,
}

pub struct OutputScreenshot {
    texture: GlesTexture,
    buffer: PrimaryGpuTextureRenderElement,
    pointer: Option<PrimaryGpuTextureRenderElement>,
}

niri_render_elements! {
    ScreenshotUiRenderElement => {
    Magnifier = SamplingRenderElement<ShaderRenderElement>,
        Screenshot = PrimaryGpuTextureRenderElement,
        SolidColor = SolidColorRenderElement,
    }
}

impl ScreenshotUi {
    pub fn new(clock: Clock, config: Rc<RefCell<Config>>) -> Self {
        Self::Closed {
            last_selection: None,
            clock,
            config,
        }
    }

    pub fn open(
        &mut self,
        // Output, screencast, screen capture.
        screenshots: HashMap<Output, [OutputScreenshot; 3]>,
        default_output: Output,
        show_pointer: bool,
        path: Option<String>,
        tool: Tool,
        mod_key: ModKey,
    ) -> bool {
        if screenshots.is_empty() {
            return false;
        }

        let Self::Closed {
            last_selection,
            clock,
            config,
        } = self
        else {
            return false;
        };
        let settings = config.borrow().screenshot.clone();
        let last_selection = last_selection
            .take()
            .filter(|_| settings.selection.remember_selection)
            .and_then(|(weak, sel)| weak.upgrade().map(|output| (output, sel)));
        let selection = match last_selection {
            Some(selection) if screenshots.contains_key(&selection.0) => selection,
            _ => {
                let output = default_output;
                let output_transform = output.current_transform();
                let output_mode = output.current_mode().unwrap();
                let size = output_transform.transform_size(output_mode.size);
                (
                    output,
                    Rectangle::new(
                        Point::from((size.w / 4, size.h / 4)),
                        Size::from((size.w / 2, size.h / 2)),
                    ),
                )
            }
        };

        let selection = (
            selection.0,
            selection.1.loc,
            selection.1.loc + selection.1.size - Size::from((1, 1)),
        );

        let output_data = screenshots
            .into_iter()
            .map(|(output, screenshot)| {
                let transform = output.current_transform();
                let output_mode = output.current_mode().unwrap();
                let size = transform.transform_size(output_mode.size);
                let scale = output.current_scale().fractional_scale();
                let buffers = [
                    SolidColorBuffer::new((0., 0.), [1., 1., 1., 1.]),
                    SolidColorBuffer::new((0., 0.), [1., 1., 1., 1.]),
                    SolidColorBuffer::new((0., 0.), [1., 1., 1., 1.]),
                    SolidColorBuffer::new((0., 0.), [1., 1., 1., 1.]),
                    SolidColorBuffer::new((0., 0.), [0., 0., 0., 0.5]),
                    SolidColorBuffer::new((0., 0.), [0., 0., 0., 0.5]),
                    SolidColorBuffer::new((0., 0.), [0., 0., 0., 0.5]),
                    SolidColorBuffer::new((0., 0.), [0., 0., 0., 0.5]),
                ];
                let locations = [Default::default(); 8];

                let data = OutputData {
                    size,
                    scale,
                    transform,
                    screenshot,
                    buffers,
                    locations,
                    canvas: Canvas::default(),
                    toolbar: RefCell::new(None),
                    guides: RefCell::new(None),
                    magnifier: Default::default(),
                    composition: RefCell::new(Vec::new()),
                };
                (output, data)
            })
            .collect();

        let open_anim = {
            let c = config.borrow();
            Animation::new(clock.clone(), 0., 1., 0., c.animations.screenshot_ui_open.0)
        };

        *self = Self::Open {
            selection,
            output_data,
            interaction: Interaction::Idle,
            editor: Editor::new(settings, tool),
            show_pointer,
            open_anim,
            clock: clock.clone(),
            config: config.clone(),
            path,
            pending_config: false,
            mod_key,
            hold_key: None,
            axes: Axes::default(),
        };
        self.refresh_config(mod_key);

        self.update_buffers();

        true
    }

    pub fn close(&mut self) -> bool {
        let Self::Open {
            selection,
            clock,
            config,
            ..
        } = self
        else {
            return false;
        };

        let last_selection = Some((
            selection.0.downgrade(),
            rect_from_corner_points(selection.1, selection.2),
        ));

        *self = Self::Closed {
            last_selection,
            clock: clock.clone(),
            config: config.clone(),
        };

        true
    }

    pub fn begin_axes_hold(&mut self, key: Keycode) {
        if let Self::Open { axes, editor, .. } = self {
            axes.begin_hold(key, editor.pointer.as_ref().map(|(_, p)| *p));
            editor.axes_enabled = axes.enabled();
        }
    }

    pub fn end_axes_hold(&mut self, key: Keycode) {
        if let Self::Open { axes, editor, .. } = self {
            axes.end_hold(key, editor.pointer.as_ref().map(|(_, p)| *p));
            editor.axes_enabled = axes.enabled();
        }
    }

    pub fn set_axes_hold(&mut self, button: super::screenshot_axes::HoldButton, held: bool) {
        if let Self::Open { axes, editor, .. } = self {
            axes.pointer_hold(button, held, editor.pointer.as_ref().map(|(_, p)| *p));
            editor.axes_enabled = axes.enabled();
        }
    }
    pub fn clear_axes_holds(&mut self) -> bool {
        let Self::Open { axes, editor, .. } = self else {
            return false;
        };
        let changed = axes.clear_holds(editor.pointer.as_ref().map(|(_, p)| *p));
        editor.axes_enabled = axes.enabled();
        changed
    }

    pub fn toggle_pointer(&mut self) {
        if let Self::Open { show_pointer, .. } = self {
            *show_pointer = !*show_pointer;
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, ScreenshotUi::Open { .. })
    }

    pub fn begin_move_hold(&mut self, key: Keycode) {
        let Self::Open { hold_key, .. } = self else {
            return;
        };
        if hold_key.is_none() {
            *hold_key = Some(key);
            self.set_space_down(true);
        }
    }

    pub fn end_move_hold(&mut self, key: Keycode) {
        let Self::Open { hold_key, .. } = self else {
            return;
        };
        if *hold_key == Some(key) {
            *hold_key = None;
            self.set_space_down(false);
        }
    }

    pub fn set_space_down(&mut self, down: bool) {
        if let Self::Open { editor, .. } = self {
            editor.space_down = down;
        }
        if let Self::Open {
            selection,
            interaction: Interaction::Selection(SelectionDrag { mode, last_pos, .. }),
            ..
        } = self
        {
            if down {
                if matches!(mode, SelectionMode::Select) {
                    *mode = SelectionMode::Move(MoveState {
                        pointer_offset: last_pos.1 - selection.1,
                        touch_slot: None,
                    });
                }
            } else {
                // Only clear a move owned by the keyboard, not a touch.
                if let SelectionMode::Move(MoveState {
                    touch_slot: None, ..
                }) = mode
                {
                    *mode = SelectionMode::Select;
                }
            }
        }
    }

    pub fn move_left(&mut self) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };

        let data = &output_data[output];

        let delta: i32 = to_physical_precise_round(data.scale, DIRECTIONAL_MOVE_PX);
        let delta = min(delta, min(a.x, b.x));
        a.x -= delta;
        b.x -= delta;

        self.update_buffers();
    }

    pub fn move_right(&mut self) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };

        let data = &output_data[output];

        let delta: i32 = to_physical_precise_round(data.scale, DIRECTIONAL_MOVE_PX);
        let delta = min(delta, data.size.w - max(a.x, b.x) - 1);
        a.x += delta;
        b.x += delta;

        self.update_buffers();
    }

    pub fn move_up(&mut self) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };

        let data = &output_data[output];

        let delta: i32 = to_physical_precise_round(data.scale, DIRECTIONAL_MOVE_PX);
        let delta = min(delta, min(a.y, b.y));
        a.y -= delta;
        b.y -= delta;

        self.update_buffers();
    }

    pub fn move_down(&mut self) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };

        let data = &output_data[output];

        let delta: i32 = to_physical_precise_round(data.scale, DIRECTIONAL_MOVE_PX);
        let delta = min(delta, data.size.h - max(a.y, b.y) - 1);
        a.y += delta;
        b.y += delta;

        self.update_buffers();
    }

    /// Moves the screenshot selection to a different output.
    ///
    /// This preserves the relative position while keeping logical size. It is (intentionally) very
    /// similar to how floating windows move across monitors, but with one difference: floating
    /// windows can go partially outside the view, while the screenshot selection cannot. So, we
    /// clamp it to new output bounds, trying to preserve the size if possible.
    pub fn move_to_output(&mut self, new_output: Output) {
        let Self::Open {
            selection,
            output_data,
            ..
        } = self
        else {
            return;
        };

        let (current_output, current_a, current_b) = selection;

        if current_output == &new_output {
            return;
        }

        let Some(target_data) = output_data.get(&new_output) else {
            return;
        };

        let current_data = &output_data[current_output];

        let current_rect: Rectangle<_, Physical> = Rectangle::new(
            Point::from((current_a.x.min(current_b.x), current_a.y.min(current_b.y))),
            Size::from((
                (current_a.x.max(current_b.x) - current_a.x.min(current_b.x) + 1),
                (current_a.y.max(current_b.y) - current_a.y.min(current_b.y) + 1),
            )),
        );
        let current_rect = current_rect.to_f64();

        let rel_x = current_rect.loc.x / current_data.size.w as f64;
        let rel_y = current_rect.loc.y / current_data.size.h as f64;

        let factor = target_data.scale / current_data.scale;
        let mut new_width = (current_rect.size.w * factor).round() as i32;
        let mut new_height = (current_rect.size.h * factor).round() as i32;

        new_width = new_width.clamp(1, target_data.size.w);
        new_height = new_height.clamp(1, target_data.size.h);

        let new_x = (rel_x * target_data.size.w as f64).round() as i32;
        let new_y = (rel_y * target_data.size.h as f64).round() as i32;

        let max_x = target_data.size.w - new_width;
        let max_y = target_data.size.h - new_height;
        let new_x = new_x.clamp(0, max_x);
        let new_y = new_y.clamp(0, max_y);

        let new_rect = Rectangle::new(
            Point::from((new_x, new_y)),
            Size::from((new_width, new_height)),
        );

        *selection = (
            new_output,
            new_rect.loc,
            new_rect.loc + new_rect.size - Size::from((1, 1)),
        );

        self.update_buffers();
    }

    pub fn set_width(&mut self, change: SizeChange) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };

        let data = &output_data[output];

        let available_size = f64::from(data.size.w);
        let current_size = max(a.x, b.x) + 1 - min(a.x, b.x);

        let new_size = match change {
            SizeChange::SetFixed(fixed) => to_physical_precise_round(data.scale, fixed),
            SizeChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., 1.);
                (available_size * prop).round() as i32
            }
            SizeChange::AdjustFixed(delta) => {
                let delta = to_physical_precise_round(data.scale, delta);
                current_size.saturating_add(delta)
            }
            SizeChange::AdjustProportion(delta) => {
                let current_prop = f64::from(current_size) / available_size;
                let prop = (current_prop + delta / 100.).clamp(0., 1.);
                (available_size * prop).round() as i32
            }
        };
        let new_size = new_size.clamp(1, data.size.w - min(a.x, b.x)) - 1;
        a.x = min(a.x, b.x);
        b.x = a.x + new_size;

        self.update_buffers();
    }

    pub fn set_height(&mut self, change: SizeChange) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };

        let data = &output_data[output];

        let available_size = f64::from(data.size.h);
        let current_size = max(a.y, b.y) + 1 - min(a.y, b.y);

        let new_size = match change {
            SizeChange::SetFixed(fixed) => to_physical_precise_round(data.scale, fixed),
            SizeChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., 1.);
                (available_size * prop).round() as i32
            }
            SizeChange::AdjustFixed(delta) => {
                let delta = to_physical_precise_round(data.scale, delta);
                current_size.saturating_add(delta)
            }
            SizeChange::AdjustProportion(delta) => {
                let current_prop = f64::from(current_size) / available_size;
                let prop = (current_prop + delta / 100.).clamp(0., 1.);
                (available_size * prop).round() as i32
            }
        };
        let new_size = new_size.clamp(1, data.size.h - min(a.y, b.y)) - 1;
        a.y = min(a.y, b.y);
        b.y = a.y + new_size;

        self.update_buffers();
    }

    pub fn advance_animations(&mut self) {}

    pub fn are_animations_ongoing(&self) -> bool {
        let Self::Open { open_anim, .. } = self else {
            return false;
        };

        !open_anim.is_done()
    }

    fn update_buffers(&mut self) {
        let Self::Open {
            selection,
            output_data,
            editor,
            ..
        } = self
        else {
            panic!("screenshot UI must be open to update buffers");
        };

        let (selection_output, a, b) = selection;
        let mut rect = rect_from_corner_points(*a, *b);

        for (output, data) in output_data {
            let buffers = &mut data.buffers;
            let locations = &mut data.locations;
            let size = data.size;
            let scale = data.scale;

            let theme = &editor.settings.selection;
            let border = (theme.border_width * scale).round() as i32;
            let mut dim = theme.dim_color.to_array_unpremul();
            dim[3] *= theme.dim_opacity as f32;
            let dim = niri_config::Color::from_array_unpremul(dim);
            for buffer in &mut buffers[..4] {
                buffer.set_color(theme.border_color);
            }
            for buffer in &mut buffers[4..] {
                buffer.set_color(dim);
            }

            if output == selection_output {
                // Check if the selection is still valid. If not, reset it back to default.
                if !Rectangle::from_size(size).contains_rect(rect) {
                    rect = Rectangle::new(
                        Point::from((size.w / 4, size.h / 4)),
                        Size::from((size.w / 2, size.h / 2)),
                    );
                    *a = rect.loc;
                    *b = rect.loc + rect.size - Size::from((1, 1));
                }

                let resize = move |buffer: &mut SolidColorBuffer, w: i32, h: i32| {
                    let size = Size::<_, Physical>::from((w, h));
                    buffer.resize(size.to_f64().to_logical(scale));
                };

                resize(&mut buffers[0], rect.size.w + border * 2, border);
                resize(&mut buffers[1], rect.size.w + border * 2, border);
                resize(&mut buffers[2], border, rect.size.h);
                resize(&mut buffers[3], border, rect.size.h);

                resize(&mut buffers[4], size.w, rect.loc.y);
                resize(&mut buffers[5], size.w, size.h - rect.loc.y - rect.size.h);
                resize(&mut buffers[6], rect.loc.x, rect.size.h);
                resize(
                    &mut buffers[7],
                    size.w - rect.loc.x - rect.size.w,
                    rect.size.h,
                );

                locations[0] = Point::from((rect.loc.x - border, rect.loc.y - border));
                locations[1] = Point::from((rect.loc.x - border, rect.loc.y + rect.size.h));
                locations[2] = Point::from((rect.loc.x - border, rect.loc.y));
                locations[3] = Point::from((rect.loc.x + rect.size.w, rect.loc.y));

                locations[5] = Point::from((0, rect.loc.y + rect.size.h));
                locations[6] = Point::from((0, rect.loc.y));
                locations[7] = Point::from((rect.loc.x + rect.size.w, rect.loc.y));
            } else {
                buffers[0].resize((0., 0.));
                buffers[1].resize((0., 0.));
                buffers[2].resize((0., 0.));
                buffers[3].resize((0., 0.));

                buffers[4].resize(size.to_f64().to_logical(data.scale));
                buffers[5].resize((0., 0.));
                buffers[6].resize((0., 0.));
                buffers[7].resize((0., 0.));
            }
        }
    }

    pub fn render_output(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        target: RenderTarget,
        push: &mut dyn FnMut(ScreenshotUiRenderElement),
    ) {
        let Self::Open {
            output_data,
            editor,
            interaction,
            axes,
            show_pointer,
            open_anim,
            ..
        } = self
        else {
            return;
        };
        let Some(data) = output_data.get(output) else {
            return;
        };
        let index = match target {
            RenderTarget::Output => 0,
            RenderTarget::Screencast => 1,
            RenderTarget::ScreenCapture => 2,
        };
        let mut image = data.composition.borrow_mut();
        image.clear();
        let result = self.image_elements(renderer, output, index, &mut image);
        if let Err(err) = &result {
            warn!("error rendering screenshot editor: {err:?}");
            image.clear();
            image.push(data.screenshot[index].buffer.clone());
        }
        // Elements are emitted front-to-back. Only this method includes the overlay.
        let dragging = matches!(
            interaction,
            Interaction::Selection(_) | Interaction::Canvas { .. }
        );
        let hover = editor
            .pointer
            .as_ref()
            .filter(|(owner, _)| owner == output && !dragging)
            .and_then(|(_, point)| {
                screenshot_editor::hit_control(
                    *point,
                    data.size,
                    data.scale,
                    editor.palette_open,
                    &editor.settings,
                    editor.tool,
                )
            });
        match Toolbar::render(
            &data.toolbar,
            renderer,
            ToolbarState::new(editor, interaction, *show_pointer, result.is_err(), hover),
            editor,
            data.size,
            data.scale,
            if dragging {
                editor.settings.toolbar.drag_opacity as f32
            } else {
                editor.settings.toolbar.opacity as f32
            },
        ) {
            Ok(toolbar) => push(toolbar.into()),
            Err(err) => warn!("error rendering screenshot toolbar: {err:?}"),
        }
        if result.is_ok() {
            if let Some((owner, point)) = &editor.pointer {
                if owner == output
                    && !screenshot_editor::overlay_contains(
                        *point,
                        data.size,
                        data.scale,
                        editor.palette_open,
                        &editor.settings,
                        editor.tool,
                    )
                {
                    if let Err(err) =
                        self.render_magnifier(renderer, output, *point, index, &image, push)
                    {
                        warn!("error rendering screenshot magnifier: {err:?}");
                    }
                    let eligible =
                        matches!(editor.tool, Tool::Draw | Tool::Arrow | Tool::Rectangle);
                    let pen = axes.point().unwrap_or(*point);
                    let center =
                        (eligible && editor.axes_enabled).then(|| axes.origin().unwrap_or(*point));
                    let brush_width = (editor.tool == Tool::Draw).then_some(editor.width);
                    if center.is_some() || brush_width.is_some() {
                        if let Err(err) = Guides::render(
                            &data.guides,
                            renderer,
                            &editor.settings.axes,
                            editor.settings_revision,
                            data.size,
                            data.scale,
                            center,
                            pen,
                            brush_width,
                            &mut |element| push(element.into()),
                        ) {
                            warn!("error rendering screenshot guides: {err:?}");
                        }
                    }
                }
            }
        }
        let progress = open_anim.clamped_value().clamp(0., 1.) as f32;
        let canvas_len = result.unwrap_or(0);
        let mut image = image.drain(..);
        for (i, (buffer, loc)) in zip(&data.buffers, &data.locations).enumerate() {
            // Keep the selection border above edits, but dim only the frozen viewport.
            if i == 4 {
                for element in image.by_ref().take(canvas_len) {
                    push(element.into());
                }
            }
            push(
                SolidColorRenderElement::from_buffer(
                    buffer,
                    loc.to_f64().to_logical(data.scale),
                    progress,
                    Kind::Unspecified,
                )
                .into(),
            );
        }
        for element in image {
            push(element.into());
        }
    }

    fn image_elements(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        index: usize,
        elements: &mut Vec<PrimaryGpuTextureRenderElement>,
    ) -> anyhow::Result<usize> {
        let Self::Open {
            output_data,
            editor,
            interaction,
            show_pointer,
            ..
        } = self
        else {
            anyhow::bail!("screenshot editor is closed");
        };
        let data = output_data
            .get(output)
            .context("screenshot output is missing")?;
        data.canvas
            .prepare(renderer, editor, interaction, output, data.size, data.scale)?;
        let screenshot = &data.screenshot[index];
        data.canvas.elements(
            renderer,
            editor.edits(output, interaction),
            &screenshot.texture,
            index,
            data.scale,
            &mut |element| elements.push(element),
        )?;
        let canvas_len = elements.len();
        if *show_pointer {
            if let Some(pointer) = &screenshot.pointer {
                elements.push(pointer.clone());
            }
        }
        elements.push(screenshot.buffer.clone());
        Ok(canvas_len)
    }

    fn render_magnifier(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        point: Point<i32, Physical>,
        index: usize,
        image: &[PrimaryGpuTextureRenderElement],
        push: &mut dyn FnMut(ScreenshotUiRenderElement),
    ) -> anyhow::Result<()> {
        let Self::Open {
            output_data,
            editor,

            show_pointer,
            ..
        } = self
        else {
            return Ok(());
        };
        let data = &output_data[output];
        let zoom = editor.magnifier_zoom.level();
        let settings = &editor.settings.magnifier;
        if zoom < settings.show_from {
            return Ok(());
        }
        let diameter = ((settings.diameter * data.scale).round() as i32)
            .max(1)
            .min(data.size.w)
            .min(data.size.h);
        let extent = ((f64::from(diameter) / (f64::from(zoom) * data.scale)).ceil() as i32) | 1;
        // Do not clamp the source origin: the cursor pixel stays in the center at edges.
        let source = Rectangle::new(
            point - Point::from((extent / 2, extent / 2)),
            Size::from((extent, extent)),
        );
        let gap = (settings.pointer_gap * data.scale).round() as i32;
        let place = |p: i32, bound: i32| {
            let loc = if p + gap + diameter <= bound {
                p + gap
            } else {
                p - gap - diameter
            };
            loc.clamp(0, bound - diameter)
        };
        let loc: Point<i32, Physical> =
            (place(point.x, data.size.w), place(point.y, data.size.h)).into();
        let mut cache = data.magnifier[index].borrow_mut();
        if cache.as_ref().is_none_or(|cached| {
            cached.point != point
                || cached.revision != editor.revision
                || cached.show_pointer != *show_pointer
                || cached.zoom != zoom
        }) {
            anyhow::ensure!(
                Shaders::get(renderer)
                    .program(ProgramType::ScreenshotMagnifier)
                    .is_some(),
                "magnifier shader is unavailable"
            );
            let elements = image.iter().rev().cloned().map(|element| {
                RelocateRenderElement::from_element(
                    element,
                    source.loc.upscale(-1),
                    Relocate::Relative,
                )
            });
            let (texture, _) = render_to_texture(
                renderer,
                source.size,
                data.scale.into(),
                Transform::Normal,
                Fourcc::Abgr8888,
                elements,
            )?;
            let lens = ShaderRenderElement::new(
                ProgramType::ScreenshotMagnifier,
                Size::<f64, Physical>::from((f64::from(diameter), f64::from(diameter)))
                    .to_logical(data.scale),
                None,
                data.scale as f32,
                1.,
                Rc::new([
                    Uniform::new("source_size", extent as f32),
                    Uniform::new("zoom", zoom as f32),
                    Uniform::new("border_color", settings.border_color.to_array_unpremul()),
                    Uniform::new("border_width", settings.border_width as f32),
                    Uniform::new("grid_color", settings.grid_color.to_array_unpremul()),
                    Uniform::new("grid_from", settings.grid_from as f32),
                    Uniform::new("grid_enabled", if settings.grid { 1.0f32 } else { 0.0f32 }),
                    Uniform::new("cross_color", {
                        let mut color = settings.cross_highlight_color.to_array_unpremul();
                        if !settings.cross_highlight {
                            color[3] = 0.;
                        }
                        color
                    }),
                ]),
                HashMap::from([("image".to_owned(), texture)]),
                Kind::Unspecified,
            )
            .with_location(loc.to_f64().to_logical(data.scale));
            *cache = Some(Magnifier {
                point,
                revision: editor.revision,
                show_pointer: *show_pointer,
                zoom,
                lens,
            });
        }
        let cached = cache.as_ref().unwrap();
        push(SamplingRenderElement::new(cached.lens.clone(), true).into());
        Ok(())
    }

    pub fn capture(
        &self,
        renderer: &mut GlesRenderer,
    ) -> anyhow::Result<(Size<i32, Physical>, Vec<u8>)> {
        anyhow::ensure!(
            !self.has_active_gesture(),
            "screenshot gesture is unfinished"
        );
        let Self::Open {
            selection,
            output_data,
            ..
        } = self
        else {
            anyhow::bail!("screenshot editor is closed");
        };
        let data = &output_data[&selection.0];
        let rect = rect_from_corner_points(selection.1, selection.2);
        let mut elements = data.composition.borrow_mut();
        elements.clear();
        self.image_elements(renderer, &selection.0, 0, &mut elements)?;
        let elements = elements.drain(..).rev().map(|element| {
            RelocateRenderElement::from_element(element, rect.loc.upscale(-1), Relocate::Relative)
        });
        let (texture, _) = render_to_texture(
            renderer,
            rect.size,
            data.scale.into(),
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )
        .context("error compositing screenshot edits")?;
        let mapping = renderer.copy_texture(
            &texture,
            Rectangle::from_size(texture.size()),
            Fourcc::Abgr8888,
        )?;
        let pixels = renderer.map_texture(&mapping)?;
        Ok((rect.size, pixels.to_vec()))
    }
    pub fn has_active_gesture(&self) -> bool {
        matches!(self, Self::Open { interaction, .. } if !matches!(interaction, Interaction::Idle))
    }

    pub fn refresh_config(&mut self, key: ModKey) {
        let active = self.has_active_gesture();
        let Self::Open {
            editor,

            config,
            pending_config,
            mod_key,
            ..
        } = self
        else {
            return;
        };
        *mod_key = key;
        *pending_config = active;
        if active {
            return;
        }
        let config = config.borrow();
        editor.apply_settings(config.screenshot.clone());
        Self::update_shortcuts(editor, &config, key);
        drop(config);
        self.update_buffers();
    }

    fn update_shortcuts(editor: &mut Editor, config: &Config, key: ModKey) {
        let mut shortcuts: Vec<(Control, String)> = Vec::new();
        for bind in &config.binds.0 {
            let control = match &bind.action {
                Action::ScreenshotTool { tool } => Some(Control::Tool(*tool)),
                Action::ScreenshotTogglePalette => editor.color_tool().map(Control::Tool),
                Action::ScreenshotUndo => Some(Control::Undo),
                Action::ScreenshotRedo => Some(Control::Redo),
                Action::ScreenshotTogglePointer => Some(Control::Pointer),
                Action::ScreenshotToggleAxes(_) => Some(Control::Axes),
                Action::ConfirmScreenshot {
                    write_to_disk,
                    format: None,
                    quality: None,
                } => Some(if *write_to_disk {
                    Control::Save
                } else {
                    Control::Copy
                }),
                Action::ScreenshotPaletteColor { index } => {
                    let index = usize::from(*index).saturating_sub(1);
                    (index < editor.palette().len()).then_some(Control::Color(index))
                }
                _ => None,
            };
            if let Some(control) = control {
                let label = super::hotkey_overlay::key_name(false, key, &bind.key);
                if let Some((_, text)) = shortcuts.iter_mut().find(|(c, _)| *c == control) {
                    text.push_str(" / ");
                    text.push_str(&label);
                } else {
                    shortcuts.push((control, label));
                }
            }
        }
        editor.set_shortcuts(shortcuts);
    }

    fn apply_pending_config(&mut self) {
        if let Self::Open {
            pending_config: true,
            mod_key,
            ..
        } = self
        {
            let key = *mod_key;
            self.refresh_config(key);
        }
    }

    fn context(&self) -> Option<ScreenshotContext> {
        let Self::Open {
            editor,
            output_data,
            ..
        } = self
        else {
            return None;
        };
        if let Some(control) = editor.focus {
            return Some(match control {
                Control::Tool(tool) => ScreenshotContext::Tool(tool),
                _ => ScreenshotContext::Overlay,
            });
        }
        let (output, point) = editor.pointer.as_ref()?;
        let data = output_data.get(output)?;
        let control = screenshot_editor::hit_control(
            *point,
            data.size,
            data.scale,
            editor.palette_open,
            &editor.settings,
            editor.tool,
        );
        if let Some(Control::Tool(tool)) = control {
            return Some(ScreenshotContext::Tool(tool));
        }
        if control.is_some()
            || screenshot_editor::overlay_contains(
                *point,
                data.size,
                data.scale,
                editor.palette_open,
                &editor.settings,
                editor.tool,
            )
        {
            Some(ScreenshotContext::Overlay)
        } else {
            Some(ScreenshotContext::Canvas)
        }
    }

    /// Execute editor commands; only export and final close leave the editor domain.
    pub fn execute_action(&mut self, action: &Action) -> Option<EditorCommand> {
        if !self.is_open() {
            return None;
        }
        let active = self.has_active_gesture();
        if active
            && !matches!(
                action,
                Action::ScreenshotCancel
                    | Action::CancelScreenshot
                    | Action::ScreenshotUndo
                    | Action::ScreenshotMagnifierZoom { .. }
                    | Action::ScreenshotContext { .. }
                    | Action::ScreenshotTogglePointer
                    | Action::ScreenshotSetPointer { .. }
                    | Action::ScreenshotToggleAxes(_)
                    | Action::ScreenshotSetAxes { .. }
            )
        {
            return None;
        }
        let mut result = None;
        match action {
            Action::ScreenshotToggleAxes(false) => {
                if let Self::Open { axes, editor, .. } = self {
                    axes.toggle(editor.pointer.as_ref().map(|(_, p)| *p));
                    editor.axes_enabled = axes.enabled();
                }
            }
            Action::ScreenshotSetAxes { enabled } => {
                if let Self::Open { axes, editor, .. } = self {
                    axes.set_enabled(*enabled, editor.pointer.as_ref().map(|(_, p)| *p));
                    editor.axes_enabled = axes.enabled();
                }
            }
            Action::ScreenshotContext { change } => {
                let context = self.context();
                if let Self::Open { editor, .. } = self {
                    match context {
                        Some(ScreenshotContext::Tool(tool)) if !active => match tool {
                            Tool::Draw | Tool::Arrow | Tool::Rectangle => {
                                editor.change_tool_width(tool, *change);
                            }
                            Tool::Blur => {
                                editor.change_blur_strength(*change);
                            }
                            Tool::Select => {}
                        },
                        Some(ScreenshotContext::Canvas) => {
                            editor.magnifier_zoom.change(*change);
                        }
                        _ => {}
                    }
                }
            }
            Action::ScreenshotArrowColor { color } => {
                if let Self::Open { editor, .. } = self {
                    editor.set_tool_color(Tool::Arrow, *color);
                }
            }
            Action::ScreenshotRectangleColor { color } => {
                if let Self::Open { editor, .. } = self {
                    editor.set_tool_color(Tool::Rectangle, *color);
                }
            }
            Action::ScreenshotTool { tool } => self.set_tool(*tool),
            Action::ScreenshotTogglePalette => self.toggle_palette(),
            Action::ScreenshotSetPalette { open } => {
                if let Self::Open { editor, .. } = self {
                    if (!*open || editor.color_tool().is_some()) && editor.palette_open != *open {
                        self.toggle_palette();
                    }
                }
            }
            Action::ScreenshotPaletteColor { index } => {
                if let Self::Open { editor, .. } = self {
                    if editor.palette_open {
                        let index = usize::from(*index).saturating_sub(1);
                        self.activate_control(Control::Color(index));
                    }
                }
            }
            Action::ScreenshotBrushColor { color } => self.set_color(*color),
            Action::ScreenshotUndo => {
                if !active
                    || matches!(self, Self::Open { editor, interaction, .. } if matches!(interaction, Interaction::Canvas { .. }))
                {
                    self.undo();
                }
            }
            Action::ScreenshotRedo => self.redo(),
            Action::ScreenshotTogglePointer => self.toggle_pointer(),
            Action::ScreenshotSetPointer {
                show_pointer: value,
            } => {
                if let Self::Open { show_pointer, .. } = self {
                    *show_pointer = *value;
                }
            }
            Action::ScreenshotMagnifierZoom { zoom } => {
                if let Self::Open { editor, .. } = self {
                    editor.magnifier_zoom.change(*zoom);
                }
            }
            Action::ScreenshotMoveSelection { x, y } => self.adjust_selection(*x, *y, false),
            Action::ScreenshotResizeSelection { width, height } => {
                self.adjust_selection(*width, *height, true)
            }
            Action::ScreenshotFocusNext => self.focus_control(false),
            Action::ScreenshotFocusPrevious => self.focus_control(true),
            Action::ScreenshotConfirm => {
                let focus = if let Self::Open { editor, .. } = self {
                    editor.focus
                } else {
                    None
                };
                result = Some(match focus.map(|c| self.activate_control(c)) {
                    Some(EditorOutcome::Changed) => return None,
                    Some(EditorOutcome::Copy) => {
                        EditorCommand::Export(ExportRequest::ClipboardOnly)
                    }
                    _ => EditorCommand::Export(ExportRequest::Save {
                        path: None,
                        format: None,
                        quality: None,
                    }),
                });
            }
            Action::ConfirmScreenshot {
                write_to_disk,
                format,
                quality,
            } => {
                result = Some(EditorCommand::Export(if *write_to_disk {
                    ExportRequest::Save {
                        path: None,
                        format: *format,
                        quality: *quality,
                    }
                } else {
                    ExportRequest::ClipboardOnly
                }))
            }
            Action::CancelScreenshot => result = Some(EditorCommand::Close),
            Action::ScreenshotCancel => {
                if let Self::Open { editor, .. } = self {
                    if editor.palette_open {
                        editor.palette_open = false;
                        editor.focus = editor.focus.map(|_| Control::Tool(editor.tool));
                        return None;
                    }
                }
                if !self.cancel_edit() {
                    result = Some(EditorCommand::Close);
                }
            }
            _ => {}
        }
        self.apply_pending_config();
        result
    }

    fn adjust_selection(&mut self, x: i32, y: i32, resize: bool) {
        let Self::Open {
            selection: (output, a, b),
            output_data,
            ..
        } = self
        else {
            return;
        };
        let size = output_data[output].size;
        let rect = rect_from_corner_points(*a, *b);
        if resize {
            let width = rect.size.w.saturating_add(x).clamp(1, size.w - rect.loc.x);
            let height = rect.size.h.saturating_add(y).clamp(1, size.h - rect.loc.y);
            *a = rect.loc;
            *b = rect.loc + Point::from((width - 1, height - 1));
        } else {
            let dx = x.clamp(-rect.loc.x, size.w - rect.loc.x - rect.size.w);
            let dy = y.clamp(-rect.loc.y, size.h - rect.loc.y - rect.size.h);
            *a += Point::from((dx, dy));
            *b += Point::from((dx, dy));
        }
        self.update_buffers();
    }

    fn focus_control(&mut self, backwards: bool) {
        let Self::Open {
            editor,
            interaction,
            ..
        } = self
        else {
            return;
        };
        let controls = screenshot_editor::CONTROLS
            .iter()
            .copied()
            .chain((0..editor.palette().len()).map(Control::Color));
        let enabled = |control: &Control| {
            screenshot_editor::control_enabled(*control, editor, interaction)
                && (!matches!(control, Control::Color(_)) || editor.palette_open)
        };
        let count = controls.clone().filter(enabled).count();
        if count == 0 {
            return;
        }
        let current = controls
            .clone()
            .filter(enabled)
            .position(|c| Some(c) == editor.focus);
        let next = match (current, backwards) {
            (Some(i), true) => (i + count - 1) % count,
            (Some(i), false) => (i + 1) % count,
            (None, true) => count - 1,
            (None, false) => 0,
        };
        editor.focus = controls.filter(enabled).nth(next);
    }
    fn toggle_palette(&mut self) {
        if let Self::Open { editor, .. } = self {
            if editor.color_tool().is_some() {
                editor.palette_open = !editor.palette_open;
                if editor.focus.is_some() {
                    editor.focus = Some(if editor.palette_open {
                        editor
                            .palette()
                            .iter()
                            .position(|c| rgb_color(*c) == editor.tool_color(editor.tool))
                            .map(Control::Color)
                            .unwrap_or(Control::Tool(editor.tool))
                    } else {
                        Control::Tool(editor.tool)
                    });
                }
            }
        }
    }

    fn activate_control(&mut self, control: Control) -> EditorOutcome {
        let Self::Open {
            editor,
            interaction,
            ..
        } = self
        else {
            return EditorOutcome::Changed;
        };
        if !screenshot_editor::control_enabled(control, editor, interaction) {
            return EditorOutcome::Changed;
        }
        match control {
            Control::Tool(tool) => {
                let focused = editor.focus.is_some();
                let palette_open = editor.palette_open;
                let open_palette = matches!(tool, Tool::Draw | Tool::Arrow | Tool::Rectangle)
                    && editor.tool_settings(tool).open_palette_on_click;
                let repeated = editor.tool == tool;
                self.set_tool(tool);
                if focused {
                    if let Self::Open { editor, .. } = self {
                        editor.focus = Some(control);
                    }
                }
                if repeated && open_palette {
                    if let Self::Open { editor, .. } = self {
                        editor.palette_open = palette_open;
                    }
                    self.toggle_palette();
                }
            }
            Control::Color(index) => {
                let color = rgb_color(editor.palette()[index]);
                if editor.tool_settings(editor.tool).close_palette_on_select {
                    editor.palette_open = false;
                    editor.focus = editor.focus.map(|_| Control::Tool(editor.tool));
                } else {
                    editor.focus = editor.focus.map(|_| control);
                }
                if let Some(tool) = editor.color_tool() {
                    editor.set_tool_color(tool, color);
                }
            }
            Control::Undo => self.undo(),
            Control::Redo => self.redo(),
            Control::Axes => {
                self.execute_action(&Action::ScreenshotToggleAxes(false));
            }
            Control::Pointer => self.toggle_pointer(),
            Control::Save => return EditorOutcome::Save,
            Control::Copy => return EditorOutcome::Copy,
        }
        EditorOutcome::Changed
    }

    pub fn selection_output(&self) -> Option<&Output> {
        if let Self::Open {
            selection: (output, _, _),
            ..
        } = self
        {
            Some(output)
        } else {
            None
        }
    }

    pub fn motion_output(&self) -> Option<&Output> {
        if let Self::Open { interaction, .. } = self {
            match interaction {
                Interaction::Canvas { output, .. }
                | Interaction::Toolbar { output, .. }
                | Interaction::Overlay { output, .. } => return Some(output),
                _ => (),
            }
        }
        self.selection_output()
    }

    pub fn output_size(&self, output: &Output) -> Option<(Size<i32, Physical>, f64, Transform)> {
        if let Self::Open { output_data, .. } = self {
            let data = output_data.get(output)?;
            Some((data.size, data.scale, data.transform))
        } else {
            None
        }
    }

    /// The current selection corner points, for tests.
    #[cfg(test)]
    pub(crate) fn selection_points(&self) -> Option<(Point<i32, Physical>, Point<i32, Physical>)> {
        if let Self::Open {
            selection: (_, p1, p2),
            ..
        } = self
        {
            Some((*p1, *p2))
        } else {
            None
        }
    }

    /// The pointer has moved to `point` relative to the current selection output.
    ///
    /// The point may be outside output bounds.
    pub fn pointer_motion(&mut self, point: Point<i32, Physical>, slot: Option<TouchSlot>) {
        let Self::Open {
            selection,
            output_data,
            editor,
            interaction,
            axes,
            ..
        } = self
        else {
            return;
        };
        let owner = match &*interaction {
            Interaction::Canvas {
                output,
                slot: owner,
                ..
            }
            | Interaction::Toolbar {
                output,
                slot: owner,
                ..
            }
            | Interaction::Overlay {
                output,
                slot: owner,
            } => {
                if *owner != slot {
                    return;
                }
                output
            }
            _ => &selection.0,
        };
        let data = &output_data[owner];
        editor.pointer = Rectangle::from_size(data.size)
            .contains(point)
            .then(|| (owner.clone(), point));
        editor.focus = None;
        if matches!(
            interaction,
            Interaction::Canvas { .. } | Interaction::Toolbar { .. } | Interaction::Overlay { .. }
        ) {
            let point = Point::from((
                point.x.clamp(0, data.size.w - 1),
                point.y.clamp(0, data.size.h - 1),
            ));
            if matches!(interaction, Interaction::Canvas { .. }) {
                let point = if matches!(interaction, Interaction::Canvas { slot: owner, .. } if *owner == slot)
                {
                    let projected = axes.project(
                        point,
                        editor.settings.axes.switch_threshold * data.scale,
                        data.size,
                    );
                    if let Some(corner) = axes.take_corner() {
                        data.canvas
                            .damage(Editor::motion(interaction, corner, slot));
                    }
                    projected
                } else {
                    point
                };
                data.canvas.damage(Editor::motion(interaction, point, slot));
                editor.revision = editor.revision.wrapping_add(1);
            }
            return;
        }
        let Interaction::Selection(SelectionDrag {
            touch_slot,
            last_pos,
            mode,
            ..
        }) = interaction
        else {
            return;
        };
        if *touch_slot != slot {
            return;
        }
        last_pos.1 = point;
        if let SelectionMode::Move(move_state) = mode {
            let delta = point - (selection.1 + move_state.pointer_offset);
            let desired = rect_from_corner_points(selection.1 + delta, selection.2 + delta);
            let bounds = Rectangle::from_size(data.size - desired.size);
            let clamped_loc = desired.loc.constrain(bounds);
            let delta = clamped_loc - rect_from_corner_points(selection.1, selection.2).loc;
            selection.1 += delta;
            selection.2 += delta;
        } else {
            selection.2 = Point::from((
                point.x.clamp(0, data.size.w - 1),
                point.y.clamp(0, data.size.h - 1),
            ));
        }
        self.update_buffers();
    }

    pub fn pointer_down(
        &mut self,
        output: Output,
        point: Point<i32, Physical>,
        slot: Option<TouchSlot>,
        move_existing: bool,
    ) -> bool {
        let Self::Open {
            selection,
            output_data,
            editor,
            interaction,
            axes,
            ..
        } = self
        else {
            return false;
        };
        let Some(data) = output_data.get(&output) else {
            return false;
        };
        if matches!(
            interaction,
            Interaction::Canvas { .. } | Interaction::Toolbar { .. } | Interaction::Overlay { .. }
        ) {
            return false;
        }
        if let Interaction::Selection(drag) = interaction {
            if let Some(new_slot) = slot {
                if drag.touch_slot != slot && matches!(drag.mode, SelectionMode::Select) {
                    drag.mode = SelectionMode::Move(MoveState {
                        pointer_offset: drag.last_pos.1 - selection.1,
                        touch_slot: Some(new_slot),
                    });
                }
            }
            return false;
        }
        editor.pointer = Some((output.clone(), point));
        editor.focus = None;
        if let Some(control) = screenshot_editor::hit_control(
            point,
            data.size,
            data.scale,
            editor.palette_open,
            &editor.settings,
            editor.tool,
        ) {
            if !screenshot_editor::control_enabled(control, editor, interaction) {
                *interaction = Interaction::Overlay { output, slot };
                return true;
            }
            *interaction = Interaction::Toolbar {
                output,
                slot,
                control,
            };
            return true;
        }
        if editor.palette_open
            || screenshot_editor::overlay_contains(
                point,
                data.size,
                data.scale,
                false,
                &editor.settings,
                editor.tool,
            )
        {
            editor.palette_open = false;
            *interaction = Interaction::Overlay { output, slot };
            return true;
        }
        if move_existing || editor.space_down {
            if output != selection.0 {
                return false;
            }
            *interaction = Interaction::Selection(SelectionDrag {
                previous: selection.clone(),
                touch_slot: slot,
                last_pos: (output, point),
                mode: SelectionMode::Move(MoveState {
                    pointer_offset: point - selection.1,
                    touch_slot: slot,
                }),
            });
            return true;
        }
        let point = Point::from((
            point.x.clamp(0, data.size.w - 1),
            point.y.clamp(0, data.size.h - 1),
        ));
        if editor.tool != Tool::Select {
            axes.begin(
                point,
                matches!(editor.tool, Tool::Draw | Tool::Arrow | Tool::Rectangle),
            );
            *interaction = editor.begin(output.clone(), point, slot);
            if let Interaction::Canvas { edit, .. } = &*interaction {
                data.canvas.damage(edit.stroke_bounds());
            }
            editor.revision = editor.revision.wrapping_add(1);
            if output != selection.0 {
                self.move_to_output(output);
            }
            return true;
        }
        *interaction = Interaction::Selection(SelectionDrag {
            previous: selection.clone(),
            touch_slot: slot,
            last_pos: (output.clone(), point),
            mode: SelectionMode::Select,
        });
        *selection = (output, point, point);
        self.update_buffers();
        true
    }

    pub fn cancel_touch(&mut self) -> bool {
        let Self::Open { interaction, .. } = self else {
            return false;
        };
        match interaction {
            Interaction::Canvas { slot: Some(_), .. }
            | Interaction::Toolbar { slot: Some(_), .. }
            | Interaction::Overlay { slot: Some(_), .. } => (),
            Interaction::Selection(drag) if drag.touch_slot.is_some() => (),
            Interaction::Selection(drag) => {
                // A touch can temporarily move a selection dragged by the mouse.
                if matches!(&drag.mode, SelectionMode::Move(state) if state.touch_slot.is_some()) {
                    drag.mode = SelectionMode::Select;
                    return true;
                }
                return false;
            }
            _ => return false,
        }
        self.cancel_edit();
        self.apply_pending_config();
        true
    }

    pub fn pointer_up(&mut self, slot: Option<TouchSlot>) -> Option<EditorOutcome> {
        let outcome = self.pointer_up_inner(slot);
        self.apply_pending_config();
        outcome
    }

    fn pointer_up_inner(&mut self, slot: Option<TouchSlot>) -> Option<EditorOutcome> {
        let Self::Open {
            selection,
            output_data,
            editor,
            interaction,
            axes,
            ..
        } = self
        else {
            return None;
        };
        match &*interaction {
            Interaction::Idle => return None,
            Interaction::Canvas { slot: owner, .. }
            | Interaction::Toolbar { slot: owner, .. }
            | Interaction::Overlay { slot: owner, .. }
                if *owner != slot =>
            {
                return None
            }
            Interaction::Selection(drag) if drag.touch_slot != slot => {
                if matches!(&drag.mode, SelectionMode::Move(state) if state.touch_slot == slot) {
                    if let Interaction::Selection(drag) = interaction {
                        drag.mode = SelectionMode::Select;
                    }
                }
                return None;
            }
            _ => (),
        }
        match std::mem::replace(interaction, Interaction::Idle) {
            Interaction::Canvas { output, edit, .. } => {
                axes.end();
                editor.history.push(Command { output, edit });
                editor.redo.clear();
                editor.revision = editor.revision.wrapping_add(1);
                return Some(EditorOutcome::Changed);
            }
            Interaction::Toolbar {
                output, control, ..
            } => {
                let data = &output_data[&output];
                let hit = editor
                    .pointer
                    .as_ref()
                    .filter(|(owner, _)| owner == &output)
                    .and_then(|(_, point)| {
                        screenshot_editor::hit_control(
                            *point,
                            data.size,
                            data.scale,
                            editor.palette_open,
                            &editor.settings,
                            editor.tool,
                        )
                    });
                if hit != Some(control) {
                    return Some(EditorOutcome::Changed);
                }
                return Some(self.activate_control(control));
            }
            Interaction::Overlay { .. } => return Some(EditorOutcome::Changed),
            Interaction::Selection(_) => (),
            Interaction::Idle => unreachable!(),
        }
        let (output, a, b) = selection;
        let rect = rect_from_corner_points(*a, *b);
        if rect.size == Size::from((1, 1)) {
            let rect = Rectangle::new((rect.loc.x - 16, rect.loc.y - 16).into(), (32, 32).into())
                .intersection(Rectangle::from_size(output_data[output].size))
                .unwrap_or_default();
            *a = rect.loc;
            *b = rect.loc + rect.size - Size::from((1, 1));
        }
        self.update_buffers();
        Some(EditorOutcome::Changed)
    }

    fn cancel_edit(&mut self) -> bool {
        let Self::Open {
            editor,
            interaction,
            output_data,
            axes,
            ..
        } = self
        else {
            return false;
        };
        let state = std::mem::replace(interaction, Interaction::Idle);
        match state {
            Interaction::Idle => return false,
            Interaction::Canvas { output, edit, .. } => {
                axes.end();
                output_data[&output].canvas.damage(edit.stroke_bounds());
                editor.revision = editor.revision.wrapping_add(1);
            }
            Interaction::Selection(drag) => {
                if let Self::Open { selection, .. } = self {
                    *selection = drag.previous;
                }
                self.update_buffers();
            }
            _ => (),
        }
        true
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.cancel_edit();
        if let Self::Open {
            editor,
            config,
            mod_key,
            ..
        } = self
        {
            editor.tool = tool;
            editor.palette_open = false;
            editor.focus = None;
            Self::update_shortcuts(editor, &config.borrow(), *mod_key);
        }
    }

    pub fn set_color(&mut self, color: Color) {
        if let Self::Open { editor, .. } = self {
            editor.color = color;
        }
    }

    pub fn undo(&mut self) {
        if self.cancel_edit() {
            return;
        }
        let Self::Open {
            editor,

            output_data,
            ..
        } = self
        else {
            return;
        };
        if let Some(command) = editor.history.pop() {
            output_data[&command.output]
                .canvas
                .damage(command.edit.stroke_bounds());
            editor.redo.push(command);
            editor.revision = editor.revision.wrapping_add(1);
        }
    }

    pub fn redo(&mut self) {
        if self.cancel_edit() {
            return;
        }
        let Self::Open {
            editor,

            output_data,
            ..
        } = self
        else {
            return;
        };
        if let Some(command) = editor.redo.pop() {
            output_data[&command.output]
                .canvas
                .damage(command.edit.stroke_bounds());
            editor.history.push(command);
            editor.revision = editor.revision.wrapping_add(1);
        }
    }
}

impl OutputScreenshot {
    pub fn from_textures(
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
        texture: GlesTexture,
        pointer: Option<(GlesTexture, Rectangle<i32, Physical>)>,
    ) -> Self {
        let buffer = PrimaryGpuTextureRenderElement(TextureRenderElement::from_texture_buffer(
            TextureBuffer::from_texture(
                renderer,
                texture.clone(),
                scale,
                Transform::Normal,
                Vec::new(),
            ),
            (0., 0.),
            1.,
            None,
            None,
            Kind::Unspecified,
        ));

        let pointer = pointer.map(|(texture, geo)| {
            PrimaryGpuTextureRenderElement(TextureRenderElement::from_texture_buffer(
                TextureBuffer::from_texture(
                    renderer,
                    texture,
                    scale,
                    Transform::Normal,
                    Vec::new(),
                ),
                geo.to_f64().to_logical(scale).loc,
                1.,
                None,
                None,
                Kind::Unspecified,
            ))
        });

        Self {
            texture,
            buffer,
            pointer,
        }
    }
}

fn rgb_color(color: niri_config::Color) -> Color {
    Color {
        r: (color.r * 255.).round() as u8,
        g: (color.g * 255.).round() as u8,
        b: (color.b * 255.).round() as u8,
    }
}

pub fn rect_from_corner_points(
    a: Point<i32, Physical>,
    b: Point<i32, Physical>,
) -> Rectangle<i32, Physical> {
    let x1 = min(a.x, b.x);
    let y1 = min(a.y, b.y);
    let x2 = max(a.x, b.x);
    let y2 = max(a.y, b.y);
    // We're adding + 1 because the pointer is clamped to output size - 1, so to get the full
    // screen worth of selection we must add back that + 1.
    Rectangle::from_extremities((x1, y1), (x2 + 1, y2 + 1))
}
