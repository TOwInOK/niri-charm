use std::cell::RefCell;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::iter::zip;
use std::rc::Rc;

use anyhow::Context;
use niri_config::{Action, Config};
use niri_ipc::SizeChange;
use smithay::backend::allocator::Fourcc;
use smithay::backend::input::TouchSlot;
use smithay::backend::renderer::element::utils::{Relocate, RelocateRenderElement};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture, Uniform};
use smithay::backend::renderer::{ExportMem, Texture as _};
use smithay::input::keyboard::{Keysym, ModifiersState};
use smithay::output::{Output, WeakOutput};
use smithay::utils::{Physical, Point, Rectangle, Scale, Size, Transform};

use super::screenshot_editor::{
    self, Canvas, Color, Command, Control, Editor, EditorOutcome, Gesture, Tool, Toolbar,
    ToolbarState,
};
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
use crate::utils::to_physical_precise_round;

const SELECTION_BORDER: i32 = 2;

type Selection = (Output, Point<i32, Physical>, Point<i32, Physical>);

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
        selection_before: Option<Selection>,
        output_data: HashMap<Output, OutputData>,
        button: Button,
        editor: Editor,
        show_pointer: bool,
        open_anim: Animation,
        clock: Clock,
        config: Rc<RefCell<Config>>,
        path: Option<String>,
    },
}

/// State for moving the selection (as opposed to just drawing).
pub struct MoveState {
    // Cursor offset from selection.1 when starting the move.
    pointer_offset: Point<i32, Physical>,
    // If the move is initiated by a touch, this is the slot. If `None`, the move was initiated by
    // holding Space.
    touch_slot: Option<TouchSlot>,
}

pub enum Button {
    Up,
    Down {
        touch_slot: Option<TouchSlot>,
        last_pos: (Output, Point<i32, Physical>),
        move_state: Option<MoveState>,
    },
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

impl Button {
    fn is_down(&self) -> bool {
        matches!(self, Self::Down { .. })
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
        _renderer: &mut GlesRenderer,
        // Output, screencast, screen capture.
        screenshots: HashMap<Output, [OutputScreenshot; 3]>,
        default_output: Output,
        show_pointer: bool,
        path: Option<String>,
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

        let last_selection = last_selection
            .take()
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
            selection_before: None,
            output_data,
            button: Button::Up,
            editor: Editor::default(),
            show_pointer,
            open_anim,
            clock: clock.clone(),
            config: config.clone(),
            path,
        };

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

    pub fn toggle_pointer(&mut self) {
        if let Self::Open { show_pointer, .. } = self {
            *show_pointer = !*show_pointer;
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, ScreenshotUi::Open { .. })
    }

    pub fn set_space_down(&mut self, down: bool) {
        if let Self::Open { editor, .. } = self {
            editor.space_down = down;
        }
        if let Self::Open {
            selection,
            button:
                Button::Down {
                    move_state,
                    last_pos,
                    ..
                },
            ..
        } = self
        {
            if down {
                if move_state.is_none() {
                    *move_state = Some(MoveState {
                        pointer_offset: last_pos.1 - selection.1,
                        touch_slot: None,
                    });
                }
            } else {
                // Only clear if moving with Space.
                if let Some(MoveState {
                    touch_slot: None, ..
                }) = move_state
                {
                    *move_state = None;
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

                let border = to_physical_precise_round(scale, SELECTION_BORDER);

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
            show_pointer,
            open_anim,
            button,
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
        let dragging = button.is_down() || matches!(editor.gesture, Some(Gesture::Canvas { .. }));
        let hover = editor
            .pointer
            .as_ref()
            .filter(|(owner, _)| owner == output && !dragging)
            .and_then(|(_, point)| {
                screenshot_editor::hit_control(*point, data.size, data.scale, editor.palette_open)
            });
        match Toolbar::render(
            &data.toolbar,
            renderer,
            ToolbarState::new(editor, *show_pointer, result.is_err(), hover),
            data.size,
            data.scale,
            if dragging { 0.5 } else { 1.0 },
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
                    )
                {
                    if let Err(err) =
                        self.render_magnifier(renderer, output, *point, index, &image, push)
                    {
                        warn!("error rendering screenshot magnifier: {err:?}");
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
            .prepare(renderer, editor, output, data.size, data.scale)?;
        let screenshot = &data.screenshot[index];
        data.canvas.elements(
            renderer,
            editor.edits(output),
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
        if zoom == 1 {
            return Ok(());
        }
        let diameter = ((192. * data.scale).round() as i32)
            .min(data.size.w)
            .min(data.size.h);
        let extent = ((f64::from(diameter) / (f64::from(zoom) * data.scale)).ceil() as i32) | 1;
        // Do not clamp the source origin: the cursor pixel stays in the center at edges.
        let source = Rectangle::new(
            point - Point::from((extent / 2, extent / 2)),
            Size::from((extent, extent)),
        );
        let gap = (16. * data.scale).round() as i32;
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

    pub fn scroll_magnifier(&mut self, amount_v120: f64) -> bool {
        match self {
            Self::Open { editor, .. } => editor.magnifier_zoom.scroll(amount_v120),
            Self::Closed { .. } => false,
        }
    }

    pub fn capture(
        &self,
        renderer: &mut GlesRenderer,
    ) -> anyhow::Result<(Size<i32, Physical>, Vec<u8>)> {
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

    pub fn action(&self, raw: Keysym, mods: ModifiersState) -> Option<Action> {
        let Self::Open { button, editor, .. } = self else {
            return None;
        };

        // Do not export an unfinished gesture.
        if button.is_down() || editor.gesture.is_some() {
            return (raw == Keysym::Escape).then_some(Action::CancelScreenshot);
        }

        action(raw, mods)
    }
    fn toggle_palette(&mut self) {
        if let Self::Open { editor, .. } = self {
            if editor.tool == Tool::Draw {
                editor.palette_open = !editor.palette_open;
                if editor.focus.is_some() {
                    editor.focus = Some(if editor.palette_open {
                        Control::Color(editor.color)
                    } else {
                        Control::Tool(Tool::Draw)
                    });
                }
            }
        }
    }

    fn activate_control(&mut self, control: Control) -> EditorOutcome {
        let Self::Open { editor, .. } = self else {
            return EditorOutcome::Changed;
        };
        if !screenshot_editor::control_enabled(control, editor) {
            return EditorOutcome::Changed;
        }
        match control {
            Control::Tool(tool) => {
                let focused = editor.focus.is_some();
                let palette_open = editor.palette_open;
                self.set_tool(tool);
                if focused {
                    if let Self::Open { editor, .. } = self {
                        editor.focus = Some(control);
                    }
                }
                if tool == Tool::Draw {
                    if let Self::Open { editor, .. } = self {
                        editor.palette_open = palette_open;
                    }
                    self.toggle_palette();
                }
            }
            Control::Color(color) => {
                editor.palette_open = false;
                editor.focus = editor.focus.map(|_| Control::Tool(Tool::Draw));
                self.set_color(color);
            }
            Control::Undo => self.undo(),
            Control::Redo => self.redo(),
            Control::Pointer => self.toggle_pointer(),
            Control::Save => return EditorOutcome::Save,
            Control::Copy => return EditorOutcome::Copy,
        }
        EditorOutcome::Changed
    }

    /// Handle hardcoded editor controls before configurable compositor bindings.
    /// The outer option denotes ownership; the inner option requests a compositor action.
    pub fn handle_editor_key(
        &mut self,
        raw: Keysym,
        mods: ModifiersState,
    ) -> Option<Option<Action>> {
        let Self::Open { editor, button, .. } = self else {
            return None;
        };
        if raw == Keysym::Escape {
            if editor.palette_open {
                editor.palette_open = false;
                editor.focus = editor.focus.map(|_| Control::Tool(Tool::Draw));
                return Some(None);
            }
            if self.cancel_edit() {
                return Some(None);
            }
            if let Self::Open {
                button,
                selection,
                selection_before,
                ..
            } = self
            {
                if button.is_down() {
                    *button = Button::Up;
                    if let Some(previous) = selection_before.take() {
                        *selection = previous;
                    }
                    self.update_buffers();
                    return Some(None);
                }
            }
            return None;
        }
        if mods.ctrl || mods.alt || mods.logo {
            return None;
        }
        if raw == Keysym::space {
            return Some(None);
        }
        if raw == Keysym::Tab || raw == Keysym::ISO_Left_Tab {
            if button.is_down() || editor.gesture.is_some() {
                return Some(None);
            }
            let controls = screenshot_editor::CONTROLS.iter().copied().chain(
                screenshot_editor::COLORS
                    .iter()
                    .copied()
                    .map(Control::Color),
            );
            let enabled = |control: &Control| {
                screenshot_editor::control_enabled(*control, editor)
                    && (!matches!(control, Control::Color(_)) || editor.palette_open)
            };
            let count = controls.clone().filter(enabled).count();
            let current = controls
                .clone()
                .filter(enabled)
                .position(|c| Some(c) == editor.focus);
            let backwards = mods.shift || raw == Keysym::ISO_Left_Tab;
            let next = match (current, backwards) {
                (Some(i), true) => (i + count - 1) % count,
                (Some(i), false) => (i + 1) % count,
                (None, true) => count - 1,
                (None, false) => 0,
            };
            editor.focus = controls.filter(enabled).nth(next);
            return Some(None);
        }
        if mods.shift {
            return None;
        }
        if button.is_down() || editor.gesture.is_some() {
            return matches!(
                raw,
                Keysym::v | Keysym::b | Keysym::r | Keysym::c | Keysym::Return
            )
            .then_some(None);
        }
        let tool = match raw {
            Keysym::v => Some(Tool::Select),
            Keysym::b => Some(Tool::Draw),
            Keysym::r => Some(Tool::Blur),
            _ => None,
        };
        if let Some(tool) = tool {
            self.set_tool(tool);
            return Some(None);
        }
        if raw == Keysym::c {
            self.toggle_palette();
            return Some(None);
        }
        let control = match raw {
            Keysym::Return if !button.is_down() && editor.gesture.is_none() => editor.focus?,
            Keysym::_1
            | Keysym::_2
            | Keysym::_3
            | Keysym::_4
            | Keysym::_5
            | Keysym::_6
            | Keysym::_7
                if editor.palette_open =>
            {
                let index = (raw.raw() - Keysym::_1.raw()) as usize;
                Control::Color(screenshot_editor::COLORS[index])
            }
            _ => return None,
        };
        let outcome = self.activate_control(control);
        Some(match outcome {
            EditorOutcome::Changed => None,
            EditorOutcome::Save => Some(Action::ConfirmScreenshot {
                write_to_disk: true,
            }),
            EditorOutcome::Copy => Some(Action::ConfirmScreenshot {
                write_to_disk: false,
            }),
        })
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
        if let Self::Open { editor, .. } = self {
            if let Some(
                Gesture::Canvas { output, .. }
                | Gesture::Toolbar { output, .. }
                | Gesture::Overlay { output, .. },
            ) = &editor.gesture
            {
                return Some(output);
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
            button,
            ..
        } = self
        else {
            return;
        };
        let owner = match &editor.gesture {
            Some(
                Gesture::Canvas {
                    output,
                    slot: owner,
                    ..
                }
                | Gesture::Toolbar {
                    output,
                    slot: owner,
                    ..
                }
                | Gesture::Overlay {
                    output,
                    slot: owner,
                },
            ) => {
                if *owner != slot {
                    return;
                }
                output
            }
            None => &selection.0,
        };
        let data = &output_data[owner];
        editor.pointer = Rectangle::from_size(data.size)
            .contains(point)
            .then(|| (owner.clone(), point));
        editor.focus = None;
        if editor.gesture.is_some() {
            let point = Point::from((
                point.x.clamp(0, data.size.w - 1),
                point.y.clamp(0, data.size.h - 1),
            ));
            if matches!(editor.gesture, Some(Gesture::Canvas { .. })) {
                data.canvas.damage(editor.motion(point, slot));
                editor.revision = editor.revision.wrapping_add(1);
            }
            return;
        }
        let Button::Down {
            touch_slot,
            last_pos,
            move_state,
        } = button
        else {
            return;
        };
        if *touch_slot != slot {
            return;
        }
        last_pos.1 = point;
        if let Some(move_state) = move_state {
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
            selection_before,
            output_data,
            editor,
            button,
            ..
        } = self
        else {
            return false;
        };
        let Some(data) = output_data.get(&output) else {
            return false;
        };
        if editor.gesture.is_some() {
            return false;
        }
        if let Some(new_slot) = slot {
            if let Button::Down {
                touch_slot,
                move_state,
                last_pos,
            } = button
            {
                if *touch_slot != slot && move_state.is_none() {
                    *move_state = Some(MoveState {
                        pointer_offset: last_pos.1 - selection.1,
                        touch_slot: Some(new_slot),
                    });
                }
            }
        }
        if button.is_down() {
            return false;
        }
        editor.pointer = Some((output.clone(), point));
        editor.focus = None;
        if let Some(control) =
            screenshot_editor::hit_control(point, data.size, data.scale, editor.palette_open)
        {
            if !screenshot_editor::control_enabled(control, editor) {
                editor.gesture = Some(Gesture::Overlay { output, slot });
                return true;
            }
            editor.gesture = Some(Gesture::Toolbar {
                output,
                slot,
                control,
            });
            return true;
        }
        if editor.palette_open
            || screenshot_editor::overlay_contains(point, data.size, data.scale, false)
        {
            editor.palette_open = false;
            editor.gesture = Some(Gesture::Overlay { output, slot });
            return true;
        }
        if move_existing || editor.space_down {
            if output != selection.0 {
                return false;
            }
            *selection_before = Some(selection.clone());
            *button = Button::Down {
                touch_slot: slot,
                last_pos: (output, point),
                move_state: Some(MoveState {
                    pointer_offset: point - selection.1,
                    touch_slot: slot,
                }),
            };
            return true;
        }
        let point = Point::from((
            point.x.clamp(0, data.size.w - 1),
            point.y.clamp(0, data.size.h - 1),
        ));
        if editor.tool != Tool::Select {
            editor.begin(output.clone(), point, slot);
            if let Some(Gesture::Canvas { edit, .. }) = &editor.gesture {
                data.canvas.damage(edit.stroke_bounds());
            }
            editor.revision = editor.revision.wrapping_add(1);
            if output != selection.0 {
                self.move_to_output(output);
            }
            return true;
        }
        *selection_before = Some(selection.clone());
        *button = Button::Down {
            touch_slot: slot,
            last_pos: (output.clone(), point),
            move_state: None,
        };
        *selection = (output, point, point);
        self.update_buffers();
        true
    }

    pub fn pointer_up(&mut self, slot: Option<TouchSlot>) -> Option<EditorOutcome> {
        let Self::Open {
            selection,
            selection_before,
            output_data,
            editor,
            button,
            ..
        } = self
        else {
            return None;
        };
        if let Some(gesture) = &editor.gesture {
            let (Gesture::Canvas { slot: owner, .. }
            | Gesture::Toolbar { slot: owner, .. }
            | Gesture::Overlay { slot: owner, .. }) = gesture;
            if *owner != slot {
                return None;
            }
            match editor.gesture.take().unwrap() {
                Gesture::Overlay { .. } => return Some(EditorOutcome::Changed),
                Gesture::Canvas { output, edit, .. } => {
                    editor.history.push(Command { output, edit });
                    editor.redo.clear();
                    editor.revision = editor.revision.wrapping_add(1);
                    return Some(EditorOutcome::Changed);
                }
                Gesture::Toolbar {
                    output, control, ..
                } => {
                    let data = &output_data[&output];
                    let hit = editor
                        .pointer
                        .as_ref()
                        .filter(|(owner, _)| *owner == output)
                        .and_then(|(_, point)| {
                            screenshot_editor::hit_control(
                                *point,
                                data.size,
                                data.scale,
                                editor.palette_open,
                            )
                        });
                    if hit != Some(control) {
                        return Some(EditorOutcome::Changed);
                    }
                    return Some(self.activate_control(control));
                }
            }
        }
        let Button::Down {
            touch_slot,
            move_state,
            ..
        } = button
        else {
            return None;
        };
        if *touch_slot != slot {
            if move_state
                .as_ref()
                .is_some_and(|state| state.touch_slot.is_some() && state.touch_slot == slot)
            {
                *move_state = None;
            }
            return None;
        }
        *button = Button::Up;
        *selection_before = None;
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
            output_data,
            ..
        } = self
        else {
            return false;
        };
        let Some(gesture) = editor.gesture.take() else {
            return false;
        };
        if let Gesture::Canvas { output, edit, .. } = gesture {
            output_data[&output].canvas.damage(edit.stroke_bounds());
            editor.revision = editor.revision.wrapping_add(1);
        }
        true
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.cancel_edit();
        if let Self::Open { editor, .. } = self {
            editor.tool = tool;
            editor.palette_open = false;
            editor.focus = None;
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

fn action(raw: Keysym, mods: ModifiersState) -> Option<Action> {
    if raw == Keysym::Escape {
        return Some(Action::CancelScreenshot);
    }

    if (mods.ctrl ^ mods.alt) && !mods.logo {
        if raw == Keysym::z {
            return Some(if mods.shift {
                Action::ScreenshotRedo
            } else {
                Action::ScreenshotUndo
            });
        }
        if mods.ctrl && !mods.shift && raw == Keysym::y {
            return Some(Action::ScreenshotRedo);
        }
    }

    if mods.alt || mods.shift || mods.logo {
        return None;
    }

    if (!mods.ctrl && raw == Keysym::Return) || (mods.ctrl && raw == Keysym::s) {
        return Some(Action::ConfirmScreenshot {
            write_to_disk: true,
        });
    }
    if mods.ctrl && raw == Keysym::c {
        return Some(Action::ConfirmScreenshot {
            write_to_disk: false,
        });
    }

    if !mods.ctrl && raw == Keysym::p {
        return Some(Action::ScreenshotTogglePointer);
    }

    None
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
