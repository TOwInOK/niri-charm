#[path = "screenshot_shapes.rs"]
mod shapes;

use niri_config::Action;
use smithay::output::Output;
use smithay::utils::{Physical, Point, Rectangle, Size};

use super::fixture::Fixture;
use super::knit::{open_window, render_output_rgba};
use crate::ui::screenshot_editor::{self, Control, Editor, EditorOutcome};
use crate::ui::screenshot_ui::ScreenshotUi;
use niri_ipc::{ScreenshotColor as Color, ScreenshotTool as Tool, ScreenshotValueChange};

/// Minimal config for deterministic rendering: no animations, startup overlay,
/// gaps, or focus ring.
const CONFIG: &str = r#"
animations {
    off
}

hotkey-overlay {
    skip-at-startup
}

layout {
    gaps 0
    focus-ring {
        off
    }
    default-column-width {
        proportion 0.5
    }
}
"#;

const TWO_OUTPUTS_CONFIG: &str = r#"
animations {
    off
}

hotkey-overlay {
    skip-at-startup
}

output "headless-1" {
    position x=0 y=0
}

output "headless-2" {
    position x=960 y=0
}
"#;

const SCALED_TRANSFORMED_CONFIG: &str = r#"
animations {
    off
}

hotkey-overlay {
    skip-at-startup
}

output "headless-1" {
    scale 1.5
    position x=0 y=0
}

output "headless-2" {
    transform "90"
    position x=960 y=0
}
"#;

fn set_up(config_text: &str) -> Fixture {
    let config = niri_config::Config::parse_mem(config_text).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, (960, 720));
    f
}

fn open_ui(f: &mut Fixture) {
    f.niri_state()
        .open_screenshot_ui(false, None, niri_ipc::ScreenshotTool::Select);
    assert!(f.niri().screenshot_ui.is_open());
}

/// Captures the current selection through the real renderer.
fn capture(f: &mut Fixture) -> (Size<i32, Physical>, Vec<u8>) {
    let state = f.niri_state();
    state
        .backend
        .headless()
        .with_primary_renderer(|renderer| state.niri.screenshot_ui.capture(renderer).unwrap())
        .unwrap()
}

/// Drags out a fresh selection. Requires the Select tool to be active.
fn select(f: &mut Fixture, output: &Output, a: (i32, i32), b: (i32, i32)) {
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), a.into(), None, false));
    ui.pointer_motion(b.into(), None);
    assert_eq!(ui.pointer_up(None), Some(EditorOutcome::Changed));
}

/// Draws a gesture (stroke or blur region) with the active tool, following
/// `points` from pointer down to pointer up.
fn gesture(f: &mut Fixture, output: &Output, points: &[(i32, i32)]) {
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), points[0].into(), None, false));
    for point in &points[1..] {
        ui.pointer_motion((*point).into(), None);
    }
    assert_eq!(ui.pointer_up(None), Some(EditorOutcome::Changed));
}

fn px(size: Size<i32, Physical>, pixels: &[u8], x: i32, y: i32) -> [u8; 4] {
    let i = (y * size.w + x) as usize * 4;
    pixels[i..i + 4].try_into().unwrap()
}

fn near(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(a, b)| a.abs_diff(*b) <= tolerance)
}

fn has_color_near(
    size: Size<i32, Physical>,
    pixels: &[u8],
    cx: i32,
    cy: i32,
    radius: i32,
    color: [u8; 4],
    tolerance: u8,
) -> bool {
    (cy - radius..=cy + radius).any(|y| {
        (cx - radius..=cx + radius).any(|x| {
            (0..size.w).contains(&x)
                && (0..size.h).contains(&y)
                && near(px(size, pixels, x, y), color, tolerance)
        })
    })
}

/// Bounding box of pixels that differ between two equally sized captures.
fn changed_rect(
    size: Size<i32, Physical>,
    before: &[u8],
    after: &[u8],
) -> Option<Rectangle<i32, Physical>> {
    assert_eq!(before.len(), after.len());
    let mut changed: Option<Rectangle<i32, Physical>> = None;
    for (i, (a, b)) in before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .enumerate()
    {
        if a != b {
            let point = Point::from((i as i32 % size.w, i as i32 / size.w));
            let rect = Rectangle::new(point, Size::from((1, 1)));
            changed = Some(changed.map_or(rect, |old| old.merge(rect)));
        }
    }
    changed
}

/// The expected opaque RGBA pixel for an editor color.
fn rgba(color: Color) -> [u8; 4] {
    let (r, g, b) = color.rgb();
    [
        (r * 255.).round() as u8,
        (g * 255.).round() as u8,
        (b * 255.).round() as u8,
        255,
    ]
}

fn command(f: &mut Fixture, action: Action) -> Option<crate::ui::screenshot_ui::EditorCommand> {
    f.niri().screenshot_ui.execute_action(&action)
}

fn set_zoom(f: &mut Fixture, zoom: u32) {
    command(
        f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Set(zoom),
        },
    );
}

fn with_ui<R>(f: &mut Fixture, inspect: impl FnOnce(&ScreenshotUi) -> R) -> R {
    inspect(&f.niri().screenshot_ui)
}

fn with_editor<R>(f: &mut Fixture, inspect: impl FnOnce(&Editor) -> R) -> R {
    with_ui(f, |ui| match ui {
        ScreenshotUi::Open { editor, .. } => inspect(editor),
        _ => panic!("screenshot UI must be open"),
    })
}

fn palette_open(f: &mut Fixture) -> bool {
    with_editor(f, |editor| editor.palette_open)
}

fn focus(f: &mut Fixture) -> Option<Control> {
    with_editor(f, |editor| editor.focus)
}

fn show_pointer(f: &mut Fixture) -> bool {
    with_ui(f, |ui| match ui {
        ScreenshotUi::Open { show_pointer, .. } => *show_pointer,
        _ => false,
    })
}

/// The output-local center of a toolbar control.
fn control_center(f: &mut Fixture, output: &Output, control: Control) -> Point<i32, Physical> {
    let (size, scale, _) = f.niri().screenshot_ui.output_size(output).unwrap();
    let index = screenshot_editor::CONTROLS
        .iter()
        .position(|c| *c == control)
        .unwrap();
    let rect = screenshot_editor::control_rect(
        index,
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    Point::from((rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2))
}

fn hover_control(f: &mut Fixture, output: &Output, control: Control) {
    let point = control_center(f, output, control);
    f.niri().screenshot_ui.pointer_motion(point, None);
}

/// The output-local center of palette swatch `index`. Swatch geometry uses
/// `control_rect` indices starting right after `CONTROLS`.
fn color_center(f: &mut Fixture, output: &Output, index: usize) -> Point<i32, Physical> {
    let (size, scale, _) = f.niri().screenshot_ui.output_size(output).unwrap();
    let rect = screenshot_editor::control_rect(
        screenshot_editor::CONTROLS.len() + index,
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    Point::from((rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2))
}

/// Presses and releases the pointer at `point`.
fn click(f: &mut Fixture, output: &Output, point: Point<i32, Physical>) -> Option<EditorOutcome> {
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), point, None, false));
    ui.pointer_up(None)
}

fn pixels_equal_in(
    size: Size<i32, Physical>,
    a: &[u8],
    b: &[u8],
    rect: Rectangle<i32, Physical>,
) -> bool {
    (rect.loc.y..rect.loc.y + rect.size.h).all(|y| {
        (rect.loc.x..rect.loc.x + rect.size.w).all(|x| px(size, a, x, y) == px(size, b, x, y))
    })
}

#[test]
fn screenshot_editor_draw_dot_and_stroke_capture_their_colors() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);

    f.niri().screenshot_ui.set_tool(Tool::Draw);
    f.niri()
        .screenshot_ui
        .set_color(Color { r: 255, g: 0, b: 0 });
    // A single-point gesture draws a dot.
    gesture(&mut f, &output, &[(480, 300)]);
    // A multi-point gesture draws a stroke.
    gesture(&mut f, &output, &[(300, 400), (460, 400), (620, 400)]);
    f.niri().screenshot_ui.set_color(Color {
        r: 0,
        g: 128,
        b: 255,
    });
    gesture(&mut f, &output, &[(600, 300)]);

    f.niri().screenshot_ui.set_tool(Tool::Select);
    select(&mut f, &output, (100, 100), (740, 620));

    let (size, pixels) = capture(&mut f);
    assert_eq!(size, Size::from((641, 521)));

    let red = rgba(Color { r: 255, g: 0, b: 0 });
    let blue = rgba(Color {
        r: 0,
        g: 128,
        b: 255,
    });
    // Captured coordinates are relative to the selection origin (100, 100).
    assert!(has_color_near(size, &pixels, 380, 200, 2, red, 2));
    assert!(has_color_near(size, &pixels, 360, 300, 2, red, 2));
    assert!(has_color_near(size, &pixels, 500, 200, 2, blue, 2));
}

#[test]
fn screenshot_editor_brush_palette_updates_indicator_and_stroke() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (740, 620));
    f.niri()
        .screenshot_ui
        .set_color(Color { r: 255, g: 0, b: 0 });
    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();
    let draw = screenshot_editor::CONTROLS
        .iter()
        .position(|control| *control == Control::Tool(Tool::Draw))
        .unwrap();
    let rect = screenshot_editor::control_rect(
        draw,
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    let center = Point::from((rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2));

    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), center, None, false));
    assert_eq!(ui.pointer_up(None), Some(EditorOutcome::Changed));
    assert!(!palette_open(&mut f));
    // Selecting a tool does not open its palette; repeat activation does.
    click(&mut f, &output, center);
    assert!(palette_open(&mut f));
    let blue = color_center(&mut f, &output, 4);
    click(&mut f, &output, blue);
    assert!(!palette_open(&mut f));
    let (_, rendered) = render_output_rgba(f.niri_state(), &output);
    for x in [5, 20, 35] {
        assert!(near(
            px(
                size,
                &rendered,
                rect.loc.x + x,
                rect.loc.y + rect.size.h - 2
            ),
            rgba(Color {
                r: 0,
                g: 128,
                b: 255
            }),
            2
        ));
        assert!(!near(
            px(
                size,
                &rendered,
                rect.loc.x + x,
                rect.loc.y + rect.size.h - 5
            ),
            rgba(Color {
                r: 0,
                g: 128,
                b: 255
            }),
            2
        ));
    }

    gesture(&mut f, &output, &[(480, 360)]);
    let (size, pixels) = capture(&mut f);
    assert!(has_color_near(
        size,
        &pixels,
        380,
        260,
        2,
        rgba(Color {
            r: 0,
            g: 128,
            b: 255
        }),
        2
    ));
}

#[test]
fn screenshot_editor_undo_redo_are_global_and_new_edit_clears_redo() {
    let mut f = set_up(TWO_OUTPUTS_CONFIG);
    f.add_output(2, (960, 720));
    let out1 = f.niri_output(1);
    let out2 = f.niri_output(2);
    f.niri_focus_output(1);
    open_ui(&mut f);

    f.niri().screenshot_ui.set_tool(Tool::Select);
    select(&mut f, &out1, (200, 200), (700, 500));
    let (_, baseline1) = capture(&mut f);
    select(&mut f, &out2, (200, 200), (700, 500));
    let (size, baseline2) = capture(&mut f);

    // Both outputs share the same capture rectangle, so dots land at the same
    // relative position.
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    gesture(&mut f, &out1, &[(480, 360)]);
    gesture(&mut f, &out2, &[(480, 360)]);
    let red = rgba(Color { r: 255, g: 0, b: 0 });
    let (size2, pixels) = capture(&mut f);
    assert_eq!(size2, size);
    assert!(has_color_near(size2, &pixels, 280, 160, 2, red, 2));

    // Undo is global: it reverts the most recent edit, on whichever output.
    f.niri().screenshot_ui.undo();
    let (_, pixels) = capture(&mut f);
    assert_eq!(pixels, baseline2);
    f.niri().screenshot_ui.move_to_output(out1.clone());
    let (_, pixels) = capture(&mut f);
    assert!(has_color_near(size2, &pixels, 280, 160, 2, red, 2));

    f.niri().screenshot_ui.undo();
    let (_, pixels) = capture(&mut f);
    assert_eq!(pixels, baseline1);

    // Redo restores edits in the order they were undone.
    f.niri().screenshot_ui.redo();
    let (_, pixels) = capture(&mut f);
    assert!(has_color_near(size2, &pixels, 280, 160, 2, red, 2));

    // A new edit on the other output clears the redo stack holding the second
    // dot, so a later redo is a no-op.
    gesture(&mut f, &out2, &[(600, 400)]);
    let (_, pixels) = capture(&mut f);
    assert!(has_color_near(size2, &pixels, 400, 200, 2, red, 2));
    f.niri().screenshot_ui.redo();
    let (_, after_redo) = capture(&mut f);
    assert_eq!(after_redo, pixels);

    // The first dot is untouched by all of that.
    f.niri().screenshot_ui.move_to_output(out1.clone());
    let (_, pixels) = capture(&mut f);
    assert!(has_color_near(size2, &pixels, 280, 160, 2, red, 2));
}

#[test]
fn screenshot_editor_stronger_blur_covers_weaker_in_both_orders() {
    use niri_config::screenshot::BlurStrength::{self, High, Low, Medium};

    fn blur(f: &mut Fixture, output: &Output, strength: BlurStrength) {
        f.niri().screenshot_ui.set_tool(Tool::Blur);
        let ScreenshotUi::Open { editor, .. } = &mut f.niri().screenshot_ui else {
            unreachable!()
        };
        editor.blur_strength = strength;
        gesture(f, output, &[(320, 240), (640, 480)]);
    }

    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(&mut f, id, "left", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_window(&mut f, id, "right", 480, 720, [0, u32::MAX, 0, u32::MAX]);
    open_ui(&mut f);
    select(&mut f, &output, (80, 80), (879, 639));

    for (weak, strong) in [(Low, Medium), (Low, High), (Medium, High)] {
        blur(&mut f, &output, strong);
        let (_, expected) = capture(&mut f);
        f.niri().screenshot_ui.undo();
        blur(&mut f, &output, weak);
        assert_ne!(capture(&mut f).1, expected);
        f.niri().screenshot_ui.undo();
        for order in [[weak, strong], [strong, weak]] {
            for strength in order {
                blur(&mut f, &output, strength);
            }
            assert_eq!(capture(&mut f).1, expected);
            f.niri().screenshot_ui.undo();
            f.niri().screenshot_ui.undo();
        }
    }
}

#[test]
fn screenshot_editor_blur_softens_inside_rect_and_restores_on_undo() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    // Two side-by-side windows give the blur a sharp edge to soften at x = 480.
    let id = f.add_client();
    open_window(&mut f, id, "left", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_window(&mut f, id, "right", 480, 720, [0, u32::MAX, 0, u32::MAX]);
    open_ui(&mut f);

    select(&mut f, &output, (80, 80), (879, 639));
    let (size, baseline) = capture(&mut f);
    let red = [255, 0, 0, 255];
    let green = [0, 255, 0, 255];

    // The blur rectangle straddles the window edge.
    f.niri().screenshot_ui.set_tool(Tool::Blur);
    gesture(&mut f, &output, &[(320, 240), (640, 480)]);
    let (size2, blurred) = capture(&mut f);
    assert_eq!(size2, size);

    // Coordinates below are relative to the selection origin (80, 80); the
    // blur covers relative x 240..=560, y 160..=400.
    // Inside, on the edge: softened into a mix of both windows.
    let edge = px(size2, &blurred, 400, 280);
    assert!(!near(edge, red, 8));
    assert!(!near(edge, green, 8));
    // Inside, far from the edge: unchanged by the blur.
    assert!(near(px(size2, &blurred, 280, 220), red, 8));
    assert!(near(px(size2, &blurred, 520, 220), green, 8));
    // Outside the blur rectangle: untouched, including on the window edge.
    assert_eq!(px(size2, &blurred, 220, 220), px(size, &baseline, 220, 220));
    assert_eq!(px(size2, &blurred, 580, 220), px(size, &baseline, 580, 220));
    assert_eq!(px(size2, &blurred, 400, 120), px(size, &baseline, 400, 120));

    // Blur applies to the frozen background only: a stroke over it stays crisp.
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    f.niri().screenshot_ui.set_color(Color { r: 0, g: 0, b: 0 });
    gesture(&mut f, &output, &[(340, 360), (620, 360)]);
    let (_, stroked) = capture(&mut f);
    assert!(has_color_near(
        size2,
        &stroked,
        400,
        280,
        2,
        rgba(Color { r: 0, g: 0, b: 0 }),
        2
    ));

    f.niri().screenshot_ui.undo();
    let (_, pixels) = capture(&mut f);
    assert_eq!(pixels, blurred);
    // Undoing the blur restores the untouched frozen background.
    f.niri().screenshot_ui.undo();
    let (_, pixels) = capture(&mut f);
    assert_eq!(pixels, baseline);
}

#[test]
fn screenshot_editor_dims_only_frozen_background() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(&mut f, id, "left", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_window(&mut f, id, "right", 480, 720, [0, u32::MAX, 0, u32::MAX]);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    f.niri().screenshot_ui.set_tool(Tool::Blur);
    gesture(&mut f, &output, &[(320, 240), (640, 480)]);
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    f.niri().screenshot_ui.set_color(Color {
        r: 0,
        g: 128,
        b: 255,
    });
    gesture(&mut f, &output, &[(200, 360), (760, 360)]);
    let (size, image) = capture(&mut f);

    f.niri().screenshot_ui.set_tool(Tool::Select);
    select(&mut f, &output, (400, 200), (700, 500));
    // Hover the toolbar so the magnifier does not cover the sampled pixels.
    let toolbar = screenshot_editor::control_rect(
        0,
        size,
        1.,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    f.niri()
        .screenshot_ui
        .pointer_motion(toolbar.loc + toolbar.size.downscale(2), None);
    let (render_size, rendered) = render_output_rgba(f.niri_state(), &output);
    assert_eq!(render_size, size);
    // Draw and blur retain their exported colors on both sides of the crop.
    for (x, y) in [(300, 360), (600, 360), (360, 300), (480, 300)] {
        assert!(
            near(px(size, &rendered, x, y), px(size, &image, x, y), 2),
            "edit dimmed at {x}, {y}"
        );
    }
    assert!(px(size, &rendered, 200, 300)[0] < px(size, &image, 200, 300)[0]);
    assert_eq!(px(size, &rendered, 680, 300), px(size, &image, 680, 300));
    // The selection border remains above the stroke crossing it.
    assert!(near(px(size, &rendered, 399, 360), [255; 4], 2));
}

#[test]
fn screenshot_editor_capture_excludes_overlay() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(&mut f, id, "window", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_ui(&mut f);

    select(&mut f, &output, (0, 0), (959, 719));
    let (size, pixels) = capture(&mut f);
    // This crop includes the toolbar and magnifier, not only an unobscured
    // interior rectangle. The toolbar overlaps the solid red window.
    let red = [255, 0, 0, 255];
    let toolbar = screenshot_editor::control_rect(
        0,
        size,
        1.,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    let point = toolbar.loc + toolbar.size.downscale(2);
    assert!(near(px(size, &pixels, point.x, point.y), red, 2));
    let (render_size, rendered) = render_output_rgba(f.niri_state(), &output);
    assert!(!near(px(render_size, &rendered, point.x, point.y), red, 8));
    f.niri()
        .screenshot_ui
        .pointer_motion((480, 360).into(), None);
    let (_, captured_with_magnifier) = capture(&mut f);
    assert_eq!(captured_with_magnifier, pixels);
}

#[test]
fn screenshot_magnifier_border_and_grid_reload() {
    let mut f = set_up(&format!(
        r##"{CONFIG}
        screenshot {{ magnifier {{
            border-width 6; border-color "#0000ff";
            grid-color "#ffffff80"; cross-highlight false;
        }}; }}"##
    ));
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(&mut f, id, "left", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_window(&mut f, id, "right", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    f.niri()
        .screenshot_ui
        .pointer_motion((480, 300).into(), None);
    set_zoom(&mut f, 4);
    let (_, styled) = render_output_rgba(f.niri_state(), &output);
    assert!(near(px(size, &styled, 684, 412), [0, 0, 255, 255], 2));
    assert!(near(px(size, &styled, 550, 422), [255, 128, 128, 255], 2));
    let replacement = niri_config::Config::parse_mem(
        r##"
        screenshot { magnifier {
            border-width 0; border-color "#0000ff";
            grid-color "#ffffff00"; cross-highlight false;
        }; }
    "##,
    )
    .unwrap();
    f.niri().config.borrow_mut().screenshot = replacement.screenshot;
    f.niri()
        .screenshot_ui
        .refresh_config(niri_config::ModKey::Super);
    let (_, plain) = render_output_rgba(f.niri_state(), &output);
    assert!(near(px(size, &plain, 684, 412), [255, 0, 0, 255], 2));
    assert!(near(px(size, &plain, 550, 422), [255, 0, 0, 255], 2));
    assert_eq!(capture(&mut f).1, original);
}

#[test]
fn screenshot_magnifier_pixels_grid_circle_and_zoom() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(&mut f, id, "left", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_window(&mut f, id, "right", 480, 720, [0, u32::MAX, 0, u32::MAX]);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    f.niri()
        .screenshot_ui
        .pointer_motion((480, 300).into(), None);
    set_zoom(&mut f, 4);
    let (_, four) = render_output_rgba(f.niri_state(), &output);
    // The lens starts at (496,316), its center is (592,412).
    // Circular corners are transparent; the left half magnifies the red window.
    assert_eq!(px(size, &four, 497, 317), px(size, &original, 497, 317));
    assert!(near(px(size, &four, 540, 420), [255, 0, 0, 255], 2));
    // Both axes are highlighted, but not neighboring rows or the center pixel.
    assert!(px(size, &four, 540, 412)[2] > 60);
    assert!(px(size, &four, 592, 429)[2] > 60);
    assert!(near(px(size, &four, 595, 429), [0, 255, 0, 255], 2));
    assert!(near(px(size, &four, 592, 412), [0, 255, 0, 255], 2));
    set_zoom(&mut f, 8);
    let (_, eight) = render_output_rgba(f.niri_state(), &output);
    // Nearest keeps both sides of a pixel boundary pure, not interpolated.
    assert!(
        near(px(size, &eight, 587, 430), [255, 0, 0, 255], 2),
        "{:?}",
        px(size, &eight, 587, 430)
    );
    assert!(near(px(size, &eight, 598, 430), [0, 255, 0, 255], 2));
    // Axis width tracks a single source pixel as magnification changes.
    assert!(px(size, &eight, 595, 429)[2] > 60);
    assert!(near(px(size, &eight, 592, 412), [0, 255, 0, 255], 2));
    // Grid line density changes with zoom while the footprint stays circular.
    assert_ne!(px(size, &four, 550, 422), px(size, &eight, 550, 422));
    assert_eq!(capture(&mut f).1, original);
    // At an output edge the cursor pixel remains centered, with neutral padding.
    f.niri().screenshot_ui.pointer_motion((0, 300).into(), None);
    let (_, edge) = render_output_rgba(f.niri_state(), &output);
    assert!(near(px(size, &edge, 118, 430), [255, 0, 0, 255], 2));
    assert!(!near(px(size, &edge, 100, 430), [255, 0, 0, 255], 2));
}

#[test]
fn screenshot_magnifier_thresholds_reset_and_limits() {
    let mut f = set_up(&format!(
        "{CONFIG}\nscreenshot {{ magnifier {{ initial-zoom 2; show-from 4; max-zoom 8; }}; }}"
    ));
    open_ui(&mut f);
    let output = f.niri_output(1);
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    let (_, hidden) = render_output_rgba(f.niri_state(), &output);
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Adjust(1),
        },
    );
    let (_, visible) = render_output_rgba(f.niri_state(), &output);
    assert_ne!(visible, hidden);
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Adjust(i32::MAX),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 8);
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Adjust(i32::MIN),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 1);
    f.niri().screenshot_ui.close();
    open_ui(&mut f);
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    assert_eq!(render_output_rgba(f.niri_state(), &output).1, hidden);
}

#[test]
fn screenshot_editor_fractional_scale_and_transformed_output() {
    let mut f = set_up(SCALED_TRANSFORMED_CONFIG);
    f.add_output(2, (960, 720));
    let out1 = f.niri_output(1);
    let out2 = f.niri_output(2);
    f.niri_focus_output(1);
    open_ui(&mut f);

    // Fractional scale (1.5): output-local physical coordinates.
    select(&mut f, &out1, (100, 100), (700, 500));
    let (size, baseline) = capture(&mut f);
    assert_eq!(size, Size::from((601, 401)));

    f.niri().screenshot_ui.set_tool(Tool::Draw);
    gesture(&mut f, &out1, &[(400, 300)]);
    let (size2, pixels) = capture(&mut f);
    assert_eq!(size2, size);
    // The dot lands at relative (300, 200) with no resampling error.
    let changed = changed_rect(size2, &baseline, &pixels).expect("dot must change captured pixels");
    assert!(changed.contains(Point::from((300, 200))));
    assert!(changed.size.w <= 12 && changed.size.h <= 12);
    assert!(has_color_near(
        size2,
        &pixels,
        300,
        200,
        4,
        rgba(Color { r: 255, g: 0, b: 0 }),
        4
    ));

    // Rotated output: local coordinates are in the displayed orientation
    // (720x960), not the mode's buffer orientation.
    f.niri().screenshot_ui.set_tool(Tool::Select);
    select(&mut f, &out2, (100, 100), (600, 800));
    let (size, baseline) = capture(&mut f);
    assert_eq!(size, Size::from((501, 701)));

    f.niri().screenshot_ui.set_tool(Tool::Draw);
    gesture(&mut f, &out2, &[(360, 300)]);
    let (size2, pixels) = capture(&mut f);
    assert_eq!(size2, size);
    let changed = changed_rect(size2, &baseline, &pixels).expect("dot must change captured pixels");
    assert!(changed.contains(Point::from((260, 200))));
    assert!(changed.size.w <= 12 && changed.size.h <= 12);
    assert!(has_color_near(
        size2,
        &pixels,
        260,
        200,
        4,
        rgba(Color { r: 255, g: 0, b: 0 }),
        4
    ));
}

/// The palette popup must not swallow or emit canvas edits. A click away from
/// it dismisses it through `Gesture::Overlay`; the same pixel behaves as a
/// normal canvas click once the popup is gone. Swatches only hit when open.
#[test]
fn screenshot_editor_palette_swatches_require_open_popup_and_dismiss_cleanly() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (740, 620));
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();

    // While the palette is closed the swatch area is ordinary canvas: a click
    // there commits a Draw dot, it does not get eaten by the hidden popup.
    let swatch = color_center(&mut f, &output, 4);
    assert_eq!(click(&mut f, &output, swatch), Some(EditorOutcome::Changed));
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 1);
    let (_, with_dot) = capture(&mut f);

    // Clicking the brush opens its palette without painting.
    let palette = control_center(&mut f, &output, Control::Tool(Tool::Draw));
    assert_eq!(
        click(&mut f, &output, palette),
        Some(EditorOutcome::Changed)
    );
    assert!(palette_open(&mut f));

    // Clicking a swatch picks that color, closes the popup and adds no edit.
    assert_eq!(click(&mut f, &output, swatch), Some(EditorOutcome::Changed));
    assert!(!palette_open(&mut f));
    assert_eq!(
        with_editor(&mut f, |e| e.color),
        Color {
            r: 0,
            g: 128,
            b: 255
        }
    );
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 1);
    assert_eq!(capture(&mut f).1, with_dot);

    // Clicking canvas away from the popup dismisses it without starting a
    // stroke or mutating the exported image.
    assert_eq!(
        click(&mut f, &output, palette),
        Some(EditorOutcome::Changed)
    );
    assert!(palette_open(&mut f));
    let canvas_point = Point::from((480, 300));
    assert_eq!(
        click(&mut f, &output, canvas_point),
        Some(EditorOutcome::Changed)
    );
    assert!(!palette_open(&mut f));

    assert_eq!(with_editor(&mut f, |e| e.history.len()), 1);
    assert_eq!(capture(&mut f).1, with_dot);

    // The very next click on the same pixel reaches the canvas and paints
    // the color chosen through the palette.
    assert_eq!(
        click(&mut f, &output, canvas_point),
        Some(EditorOutcome::Changed)
    );
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 2);
    let (cap_size, pixels) = capture(&mut f);
    assert!(has_color_near(
        cap_size,
        &pixels,
        380,
        200,
        2,
        rgba(Color {
            r: 0,
            g: 128,
            b: 255
        }),
        2
    ));

    // Toolbar padding (the gap between control groups) is overlay, not
    // canvas: with the popup open it dismisses without an edit.
    let blur_rect = screenshot_editor::control_rect(
        screenshot_editor::CONTROLS
            .iter()
            .position(|control| *control == Control::Tool(Tool::Blur))
            .unwrap(),
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    let undo_rect = screenshot_editor::control_rect(
        screenshot_editor::CONTROLS
            .iter()
            .position(|control| *control == Control::Undo)
            .unwrap(),
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    let padding = Point::from((
        (blur_rect.loc.x + blur_rect.size.w + undo_rect.loc.x) / 2,
        blur_rect.loc.y + blur_rect.size.h / 2,
    ));
    assert_eq!(
        click(&mut f, &output, palette),
        Some(EditorOutcome::Changed)
    );
    assert!(palette_open(&mut f));
    assert_eq!(
        click(&mut f, &output, padding),
        Some(EditorOutcome::Changed)
    );
    assert!(!palette_open(&mut f));
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 2);

    // With the palette closed, the same padding is still not canvas: a Select
    // press there is absorbed instead of restarting the crop.
    f.niri().screenshot_ui.set_tool(Tool::Select);
    let before = f.niri().screenshot_ui.selection_points().unwrap();
    assert_eq!(
        click(&mut f, &output, padding),
        Some(EditorOutcome::Changed)
    );
    assert_eq!(f.niri().screenshot_ui.selection_points().unwrap(), before);
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 2);
}

#[test]
fn screenshot_editor_scroll_uses_source_units_and_accumulates() {
    use super::test_input_backend::{TestInputBackend, TestPointerAxisEvent};
    use smithay::backend::input::{Axis, AxisSource, InputEvent};

    let mut f = set_up(&format!(
        r#"{CONFIG}
        binds {{
            TouchpadScrollDown {{ screenshot-context "+1"; }}
            TouchpadScrollUp {{ screenshot-context "-1"; }}
            TouchpadScrollRight {{ screenshot-context "+2"; }}
            WheelScrollRight {{ screenshot-context "+1"; }}
        }}"#
    ));
    open_ui(&mut f);
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    command(&mut f, Action::ScreenshotFocusNext);
    command(&mut f, Action::ScreenshotFocusNext);
    let initial = with_editor(&mut f, |e| e.width);
    for (source, axis, amount, v120, delta) in [
        (AxisSource::Finger, Axis::Vertical, Some(6.), None, 0),
        (AxisSource::Finger, Axis::Vertical, Some(4.), None, 1),
        (AxisSource::Finger, Axis::Vertical, Some(-10.), None, 0),
        (AxisSource::Wheel, Axis::Horizontal, Some(15.), Some(60.), 0),
        (AxisSource::Wheel, Axis::Horizontal, Some(15.), Some(60.), 1),
        (AxisSource::Finger, Axis::Horizontal, Some(10.), None, 3),
        (AxisSource::Finger, Axis::Vertical, Some(30.), None, 6),
    ] {
        f.niri_state()
            .process_input_event::<TestInputBackend>(InputEvent::PointerAxis {
                event: TestPointerAxisEvent {
                    source,
                    axis,
                    amount,
                    v120,
                },
            });
        assert_eq!(with_editor(&mut f, |e| e.width), initial + delta);
    }
}

#[test]
fn screenshot_editor_focus_activation_and_palette_selection() {
    let mut f = set_up(CONFIG);
    open_ui(&mut f);
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    command(&mut f, Action::ScreenshotFocusNext);
    command(&mut f, Action::ScreenshotFocusNext);
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));
    command(&mut f, Action::ScreenshotConfirm);
    assert!(palette_open(&mut f));
    command(&mut f, Action::ScreenshotPaletteColor { index: 5 });
    assert!(!palette_open(&mut f));
    assert_eq!(
        with_editor(&mut f, |e| e.color),
        Color {
            r: 0,
            g: 128,
            b: 255
        }
    );
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));
    command(&mut f, Action::ScreenshotFocusPrevious);
    command(&mut f, Action::ScreenshotFocusPrevious);
    assert_eq!(focus(&mut f), Some(Control::Save));
    assert_eq!(
        command(&mut f, Action::ScreenshotConfirm),
        Some(crate::ui::screenshot_ui::EditorCommand::Export(
            crate::utils::screenshot::ExportRequest::Save {
                path: None,
                format: None,
                quality: None,
            }
        ))
    );
    command(&mut f, Action::ScreenshotFocusPrevious);
    assert_eq!(focus(&mut f), Some(Control::Copy));
    assert_eq!(
        command(&mut f, Action::ScreenshotConfirm),
        Some(crate::ui::screenshot_ui::EditorCommand::Export(
            crate::utils::screenshot::ExportRequest::ClipboardOnly
        ))
    );
    assert_eq!(
        command(
            &mut f,
            Action::ConfirmScreenshot {
                write_to_disk: true,
                format: None,
                quality: None,
            }
        ),
        Some(crate::ui::screenshot_ui::EditorCommand::Export(
            crate::utils::screenshot::ExportRequest::Save {
                path: None,
                format: None,
                quality: None,
            }
        ))
    );
}

/// Escape dismisses the popup, cancels an in-progress gesture or select drag
/// (restoring the previous crop), and only falls through to the compositor
/// cancel action when the editor has nothing left to undo.

#[test]
fn screenshot_editor_escape_layers_and_crop_restore() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (740, 620));

    // Layer 1: open palette swallows Escape and returns focus to Draw.
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    assert_eq!(command(&mut f, Action::ScreenshotTogglePalette), None);
    assert!(palette_open(&mut f));
    assert_eq!(command(&mut f, Action::ScreenshotCancel), None);
    assert!(!palette_open(&mut f));
    // Focus was not on a swatch, so it stays unset.
    assert_eq!(focus(&mut f), None);

    // Layer 2: a Draw gesture is cancelled, not committed.
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (400, 300).into(), None, false));
    ui.pointer_motion((460, 300).into(), None);
    // Tab is a no-op mid-gesture instead of moving focus under the stroke.
    assert_eq!(command(&mut f, Action::ScreenshotFocusNext), None);
    assert_eq!(command(&mut f, Action::ScreenshotCancel), None);

    assert_eq!(with_editor(&mut f, |e| e.history.len()), 0);
    // The matching release reaches nobody.
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);

    // Layer 3: a Select drag restores the crop it started from.
    f.niri().screenshot_ui.set_tool(Tool::Select);
    let before = f.niri().screenshot_ui.selection_points().unwrap();
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (300, 400).into(), None, false));
    ui.pointer_motion((500, 600).into(), None);
    assert_eq!(command(&mut f, Action::ScreenshotCancel), None);
    assert_eq!(f.niri().screenshot_ui.selection_points().unwrap(), before);
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);

    assert_eq!(
        command(&mut f, Action::ScreenshotCancel),
        Some(crate::ui::screenshot_ui::EditorCommand::Close)
    );
    assert!(f.niri().screenshot_ui.is_open());
}

/// Holding Space before pointer-down moves the crop in any tool instead of
/// drawing or reselecting; Escape during the move restores the crop.
#[test]
fn screenshot_editor_space_drag_moves_crop_in_any_tool() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (740, 620));
    let before = f.niri().screenshot_ui.selection_points().unwrap();

    // Draw tool + Space drag: the crop slides, no stroke is committed.
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    f.niri().screenshot_ui.set_space_down(true);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (300, 300).into(), None, false));
    ui.pointer_motion((340, 330).into(), None);
    let moved = f.niri().screenshot_ui.selection_points().unwrap();
    assert_eq!(moved.0.x - before.0.x, 40);
    assert_eq!(moved.0.y - before.0.y, 30);
    assert_eq!(moved.1.x - before.1.x, 40);
    assert_eq!(moved.1.y - before.1.y, 30);

    assert_eq!(
        f.niri().screenshot_ui.pointer_up(None),
        Some(EditorOutcome::Changed)
    );
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 0);

    // Select tool + Space drag also moves rather than reselecting.
    f.niri().screenshot_ui.set_tool(Tool::Select);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (400, 350).into(), None, false));
    ui.pointer_motion((450, 350).into(), None);
    let moved2 = f.niri().screenshot_ui.selection_points().unwrap();
    assert_eq!(moved2.0.x - moved.0.x, 50);
    assert_eq!(moved2.0.y - moved.0.y, 0);
    // Escape mid-move restores the crop to where the drag began.
    assert_eq!(command(&mut f, Action::ScreenshotCancel), None);
    assert_eq!(f.niri().screenshot_ui.selection_points().unwrap(), moved);
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);
    f.niri().screenshot_ui.set_space_down(false);
}

/// Toolbar opacity drops while a canvas gesture or select drag is held, but
/// stays full while a toolbar button or overlay padding is pressed, and is
/// restored after pointer-up or Escape.
#[test]
fn screenshot_editor_toolbar_opacity_only_drops_for_canvas_drags() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(
        &mut f,
        id,
        "window",
        960,
        720,
        [u32::MAX, u32::MAX, u32::MAX, u32::MAX],
    );
    open_ui(&mut f);
    select(&mut f, &output, (0, 80), (959, 719));

    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();
    // The Save cell is the alpha probe: its contents depend only on opacity.
    let save_rect = screenshot_editor::CONTROLS
        .iter()
        .position(|c| *c == Control::Save)
        .map(|i| {
            screenshot_editor::control_rect(
                i,
                size,
                scale,
                &f.niri().config.borrow().screenshot,
                Tool::Draw,
            )
        })
        .unwrap();
    f.niri()
        .screenshot_ui
        .pointer_motion((10, 700).into(), None);
    let (_, idle) = render_output_rgba(f.niri_state(), &output);

    // Held select drag: toolbar dims.
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (300, 400).into(), None, false));
    ui.pointer_motion((340, 420).into(), None);
    let (_, during_select) = render_output_rgba(f.niri_state(), &output);
    assert!(!pixels_equal_in(size, &idle, &during_select, save_rect));
    // Escape cancels the drag and restores full opacity and the old crop.
    assert_eq!(command(&mut f, Action::ScreenshotCancel), None);
    let (_, after_escape) = render_output_rgba(f.niri_state(), &output);
    assert!(pixels_equal_in(size, &idle, &after_escape, save_rect));
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);

    // Held Draw stroke: toolbar dims the same way.
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (480, 400).into(), None, false));
    let (_, during_draw) = render_output_rgba(f.niri_state(), &output);
    assert!(!pixels_equal_in(size, &idle, &during_draw, save_rect));
    assert_eq!(
        f.niri().screenshot_ui.pointer_up(None),
        Some(EditorOutcome::Changed)
    );
    f.niri()
        .screenshot_ui
        .pointer_motion((10, 700).into(), None);
    let (_, after_draw) = render_output_rgba(f.niri_state(), &output);
    assert!(pixels_equal_in(size, &idle, &after_draw, save_rect));

    // Held Blur rectangle: same dimming.
    f.niri().screenshot_ui.set_tool(Tool::Blur);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (300, 400).into(), None, false));
    ui.pointer_motion((500, 500).into(), None);
    let (_, during_blur) = render_output_rgba(f.niri_state(), &output);
    assert!(!pixels_equal_in(size, &idle, &during_blur, save_rect));
    assert_eq!(
        f.niri().screenshot_ui.pointer_up(None),
        Some(EditorOutcome::Changed)
    );

    // Held toolbar press: full opacity. Compare against a fresh baseline so
    // the committed stroke/blur pixels don't pollute the diff.
    f.niri()
        .screenshot_ui
        .pointer_motion((10, 700).into(), None);
    let (_, pre_press) = render_output_rgba(f.niri_state(), &output);
    let copy = control_center(&mut f, &output, Control::Copy);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), copy, None, false));
    let (_, during_press) = render_output_rgba(f.niri_state(), &output);
    // The press itself repaints (hover/tooltip) but must not dim the bar like
    // a canvas drag does: the Save cell matches the undimmed baseline and
    // differs from the dragged frame.
    assert!(pixels_equal_in(size, &pre_press, &during_press, save_rect));
    assert!(!pixels_equal_in(
        size,
        &during_draw,
        &during_press,
        save_rect
    ));
    // Releasing still reports the pressed control's outcome.
    assert_eq!(
        f.niri().screenshot_ui.pointer_up(None),
        Some(EditorOutcome::Copy)
    );
    f.niri()
        .screenshot_ui
        .pointer_motion((10, 700).into(), None);
    let (_, after_press) = render_output_rgba(f.niri_state(), &output);
    assert_eq!(after_press, pre_press);
    // Held overlay press (bar padding) keeps full opacity too.
    let blur_rect = screenshot_editor::control_rect(
        screenshot_editor::CONTROLS
            .iter()
            .position(|control| *control == Control::Tool(Tool::Blur))
            .unwrap(),
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    let undo_rect = screenshot_editor::control_rect(
        screenshot_editor::CONTROLS
            .iter()
            .position(|control| *control == Control::Undo)
            .unwrap(),
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    let padding = Point::from((
        (blur_rect.loc.x + blur_rect.size.w + undo_rect.loc.x) / 2,
        blur_rect.loc.y + blur_rect.size.h / 2,
    ));
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), padding, None, false));

    let (_, during_overlay) = render_output_rgba(f.niri_state(), &output);
    assert!(pixels_equal_in(
        size,
        &pre_press,
        &during_overlay,
        save_rect
    ));
    assert_eq!(
        f.niri().screenshot_ui.pointer_up(None),
        Some(EditorOutcome::Changed)
    );
}

/// Click outcomes for the action row: Copy and Save report EditorOutcome,
/// disabled controls absorb the click as an Overlay gesture, and the Cursor
/// toggle visibly repaints its cell without touching the exported pixels.
#[test]
fn screenshot_editor_action_controls_outcomes_and_pointer_toggle() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (740, 620));
    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();
    let pointer_rect = screenshot_editor::CONTROLS
        .iter()
        .position(|c| *c == Control::Pointer)
        .map(|i| {
            screenshot_editor::control_rect(
                i,
                size,
                scale,
                &f.niri().config.borrow().screenshot,
                Tool::Draw,
            )
        })
        .unwrap();

    // Disabled Undo is absorbed without an edit.
    let undo = control_center(&mut f, &output, Control::Undo);
    assert_eq!(click(&mut f, &output, undo), Some(EditorOutcome::Changed));
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 0);

    // Copy and Save hand their outcomes to the compositor without closing.
    let copy = control_center(&mut f, &output, Control::Copy);
    assert_eq!(click(&mut f, &output, copy), Some(EditorOutcome::Copy));
    let save = control_center(&mut f, &output, Control::Save);
    assert_eq!(click(&mut f, &output, save), Some(EditorOutcome::Save));
    assert!(f.niri().screenshot_ui.is_open());

    // Cursor toggle repaints its cell; the captured cursor may legitimately
    // appear elsewhere in the frame, so only pin the cell itself. Hover the
    // cell first so both frames share the same tooltip/hover state.
    let pointer = control_center(&mut f, &output, Control::Pointer);
    f.niri().screenshot_ui.pointer_motion(pointer, None);
    assert!(!show_pointer(&mut f));
    let (_, before) = render_output_rgba(f.niri_state(), &output);
    assert_eq!(
        click(&mut f, &output, pointer),
        Some(EditorOutcome::Changed)
    );
    assert!(show_pointer(&mut f));
    let (_, toggled) = render_output_rgba(f.niri_state(), &output);
    let changed = changed_rect(size, &before, &toggled).unwrap();
    assert!(changed.intersection(pointer_rect).is_some());
    // Toggling back restores the off state exactly.
    assert_eq!(
        click(&mut f, &output, pointer),
        Some(EditorOutcome::Changed)
    );
    assert!(!show_pointer(&mut f));
    let (_, restored) = render_output_rgba(f.niri_state(), &output);
    assert_eq!(restored, before);
}

#[test]
fn screenshot_reload_defers_gesture_and_preserves_session_and_strokes() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (740, 620));
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    hover_control(&mut f, &output, Control::Tool(Tool::Draw));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(12),
        },
    );
    command(
        &mut f,
        Action::ScreenshotBrushColor {
            color: Color {
                r: 18,
                g: 52,
                b: 86,
            },
        },
    );
    let crop = f.niri().screenshot_ui.selection_points().unwrap();
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (300, 300).into(), None, false));
    ui.pointer_motion((500, 300).into(), None);
    let replacement = niri_config::Config::parse_mem(
        r##"
        screenshot {
            toolbar { button-size 52; }
            brush { width 2; color "#00FF00"; palette "#00FF00"; }
            magnifier { initial-zoom 2; max-zoom 4; }
        }
    "##,
    )
    .unwrap();
    let old_button_size = with_editor(&mut f, |e| e.settings.toolbar.button_size);
    f.niri().config.borrow_mut().screenshot = replacement.screenshot;
    f.niri()
        .screenshot_ui
        .refresh_config(niri_config::ModKey::Super);
    assert_eq!(
        with_editor(&mut f, |e| e.settings.toolbar.button_size),
        old_button_size
    );
    assert_eq!(
        command(
            &mut f,
            Action::ConfirmScreenshot {
                write_to_disk: false,
                format: None,
                quality: None,
            }
        ),
        None
    );
    assert_eq!(with_editor(&mut f, |e| e.width), 12);
    f.niri().screenshot_ui.pointer_up(None);
    assert_eq!(with_editor(&mut f, |e| e.settings.toolbar.button_size), 52.);
    assert_eq!(
        with_editor(&mut f, |e| (e.tool, e.width, e.color)),
        (
            Tool::Draw,
            12,
            Color {
                r: 18,
                g: 52,
                b: 86
            }
        )
    );
    assert_eq!(f.niri().screenshot_ui.selection_points().unwrap(), crop);
    let (size, before) = capture(&mut f);
    assert!(near(px(size, &before, 300, 204), [18, 52, 86, 255], 2));
    hover_control(&mut f, &output, Control::Tool(Tool::Draw));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(2),
        },
    );
    command(
        &mut f,
        Action::ScreenshotBrushColor {
            color: Color { r: 0, g: 255, b: 0 },
        },
    );
    gesture(&mut f, &output, &[(300, 400), (500, 400)]);
    let (_, with_second) = capture(&mut f);
    assert!(near(px(size, &with_second, 300, 300), [0, 255, 0, 255], 2));
    command(&mut f, Action::ScreenshotUndo);
    assert_eq!(capture(&mut f).1, before);
    command(&mut f, Action::ScreenshotRedo);
    assert_eq!(capture(&mut f).1, with_second);
    f.niri().screenshot_ui.close();
    open_ui(&mut f);
    assert_eq!(
        with_editor(&mut f, |e| (e.width, e.color, e.magnifier_zoom.level())),
        (2, Color { r: 0, g: 255, b: 0 }, 2)
    );
}

#[test]
fn screenshot_selection_commands_clamp_without_overflow() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (300, 250));
    command(
        &mut f,
        Action::ScreenshotMoveSelection {
            x: i32::MAX,
            y: i32::MIN,
        },
    );
    assert_eq!(
        f.niri().screenshot_ui.selection_points(),
        Some(((759, 0).into(), (959, 150).into()))
    );
    command(
        &mut f,
        Action::ScreenshotResizeSelection {
            width: i32::MIN,
            height: i32::MAX,
        },
    );
    assert_eq!(
        f.niri().screenshot_ui.selection_points(),
        Some(((759, 0).into(), (759, 719).into()))
    );
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    hover_control(&mut f, &output, Control::Tool(Tool::Draw));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(i32::MAX),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.width), 128);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(i32::MIN),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.width), 1);
}

#[test]
fn screenshot_theme_dims_inactive_outputs_too() {
    let mut f = set_up(&format!(
        r##"{TWO_OUTPUTS_CONFIG}
        screenshot {{ selection {{ dim-color "#ff0000"; dim-opacity 0.5; }}; }}
    "##
    ));
    f.add_output(2, (960, 720));
    open_ui(&mut f);
    let out1 = f.niri_output(1);
    let out2 = f.niri_output(2);
    let (size, first) = render_output_rgba(f.niri_state(), &out1);
    let (_, second) = render_output_rgba(f.niri_state(), &out2);
    let first = px(size, &first, 10, 300);
    let second = px(size, &second, 10, 300);
    assert!(first[0] > first[1].saturating_add(80));
    assert_eq!(first, second);
}

#[test]
fn screenshot_selection_touch_release_order_preserves_crop() {
    use smithay::backend::input::TouchSlot;
    for secondary_first in [false, true] {
        let mut f = set_up(CONFIG);
        let output = f.niri_output(1);
        open_ui(&mut f);
        let ui = &mut f.niri().screenshot_ui;
        let primary = Some(TouchSlot::from(Some(0)));
        let secondary = Some(TouchSlot::from(Some(1)));
        assert!(ui.pointer_down(output.clone(), (100, 100).into(), primary, false));
        ui.pointer_motion((300, 300).into(), primary);
        assert!(!ui.pointer_down(output, (400, 400).into(), secondary, false));
        ui.pointer_motion((320, 330).into(), primary);
        assert_eq!(
            ui.selection_points(),
            Some(((120, 130).into(), (320, 330).into()))
        );
        if secondary_first {
            assert_eq!(ui.pointer_up(secondary), None);
            assert!(ui.has_active_gesture());
            ui.pointer_motion((350, 360).into(), primary);
            assert_eq!(
                ui.selection_points(),
                Some(((120, 130).into(), (350, 360).into()))
            );
        }
        assert_eq!(ui.pointer_up(primary), Some(EditorOutcome::Changed));
        let committed = ui.selection_points();
        assert_eq!(ui.pointer_up(secondary), None);
        ui.pointer_motion((600, 600).into(), primary);
        assert_eq!(ui.selection_points(), committed);
        assert!(!ui.has_active_gesture());
    }
}

#[test]
fn screenshot_editor_touch_cancel_discards_stroke_and_allows_reused_slot() {
    use super::test_input_backend::{TestInputBackend, TestTouchEvent};
    use smithay::backend::input::InputEvent;

    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    f.niri().seat.add_touch();
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    let before = capture(&mut f);
    let touch = |x, y| TestTouchEvent {
        slot: Some(0).into(),
        position: (x / 960., y / 720.),
    };
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchDown {
            event: touch(300., 250.),
        });
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchMotion {
            event: touch(400., 300.),
        });
    assert!(f.niri().screenshot_ui.has_active_gesture());

    let tooltips = !with_editor(&mut f, |e| e.settings.tooltips.enabled);
    f.niri().config.borrow_mut().screenshot.tooltips.enabled = tooltips;
    f.niri()
        .screenshot_ui
        .refresh_config(niri_config::ModKey::Super);
    assert_ne!(
        with_editor(&mut f, |e| e.settings.tooltips.enabled),
        tooltips
    );
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchCancel {
            event: touch(400., 300.),
        });
    assert!(!f.niri().screenshot_ui.has_active_gesture());
    assert_eq!(
        with_editor(&mut f, |e| e.settings.tooltips.enabled),
        tooltips
    );
    assert_eq!(capture(&mut f), before);

    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchDown {
            event: touch(500., 450.),
        });
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchMotion {
            event: touch(600., 450.),
        });
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchUp {
            event: touch(600., 450.),
        });
    let (size, pixels) = capture(&mut f);
    assert!(has_color_near(
        size,
        &pixels,
        550,
        450,
        1,
        [255, 0, 0, 255],
        2
    ));
    assert_eq!(px(size, &pixels, 350, 275), px(size, &before.1, 350, 275));
    command(&mut f, Action::ScreenshotUndo);
    assert_eq!(capture(&mut f), before);
}

#[test]
fn screenshot_editor_touch_cancel_restores_crop_and_preserves_mouse_gesture() {
    use super::test_input_backend::{TestInputBackend, TestTouchEvent};
    use smithay::backend::input::InputEvent;

    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    f.niri().seat.add_touch();
    open_ui(&mut f);
    select(&mut f, &output, (100, 100), (800, 600));
    let before = capture(&mut f);
    let touch = |x, y| TestTouchEvent {
        slot: Some(0).into(),
        position: (x / 960., y / 720.),
    };
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchDown {
            event: touch(300., 250.),
        });
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchMotion {
            event: touch(400., 350.),
        });
    assert!(f.niri().screenshot_ui.has_active_gesture());
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchCancel {
            event: touch(400., 350.),
        });
    assert_eq!(capture(&mut f), before);

    f.niri().screenshot_ui.set_tool(Tool::Draw);
    f.niri()
        .screenshot_ui
        .pointer_down(output, (300, 300).into(), None, false);
    f.niri()
        .screenshot_ui
        .pointer_motion((500, 400).into(), None);
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchCancel {
            event: touch(400., 350.),
        });
    assert!(f.niri().screenshot_ui.has_active_gesture());
    assert_eq!(
        f.niri().screenshot_ui.pointer_up(None),
        Some(EditorOutcome::Changed)
    );
    let (size, pixels) = capture(&mut f);
    assert!(has_color_near(
        size,
        &pixels,
        300,
        250,
        1,
        [255, 0, 0, 255],
        2
    ));
}

#[test]
fn screenshot_editor_closed_command_preserves_overview_touch_grab() {
    use super::test_input_backend::{TestInputBackend, TestTouchEvent};
    use smithay::backend::input::InputEvent;

    let mut f = set_up(CONFIG);
    let touch = f.niri().seat.add_touch();
    f.niri_state().do_action(Action::ToggleOverview, false);
    f.niri_state().refresh_and_flush_clients();
    f.niri_state()
        .process_input_event::<TestInputBackend>(InputEvent::TouchDown {
            event: TestTouchEvent {
                slot: Some(0).into(),
                position: (0.5, 0.5),
            },
        });
    assert!(touch.is_grabbed());
    assert!(!f.niri().screenshot_ui.is_open());
    f.niri_state().do_action(Action::ScreenshotUndo, false);
    assert!(touch.is_grabbed());

    // Ordinary actions still end the grab before changing compositor state.
    f.niri_state().do_action(Action::ToggleOverview, false);
    assert!(!touch.is_grabbed());
}

#[test]
fn screenshot_editor_tooltip_is_visible_on_translucent_toolbar() {
    let mut f = set_up(&format!(
        r#"{CONFIG}
        screenshot {{ toolbar {{ opacity 0.95; }}; }}"#
    ));
    let output = f.niri_output(1);
    open_ui(&mut f);
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    hover_control(&mut f, &output, Control::Tool(Tool::Draw));
    let (size, with_tip) = render_output_rgba(f.niri_state(), &output);

    f.niri().config.borrow_mut().screenshot.tooltips.enabled = false;
    f.niri()
        .screenshot_ui
        .refresh_config(niri_config::ModKey::Super);
    let (_, without_tip) = render_output_rgba(f.niri_state(), &output);
    let changed = changed_rect(size, &without_tip, &with_tip).expect("visible tooltip");
    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();
    let index = screenshot_editor::CONTROLS
        .iter()
        .position(|c| *c == Control::Tool(Tool::Draw))
        .unwrap();
    let button = screenshot_editor::control_rect(
        index,
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    assert!(changed.loc.y >= button.loc.y + button.size.h);
    assert!(changed.loc.y + changed.size.h <= button.loc.y + button.size.h + 64);
    assert!(pixels_equal_in(size, &without_tip, &with_tip, button));
}

#[test]
fn screenshot_editor_duplicate_palette_colors_remain_keyboard_accessible() {
    use super::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};
    use smithay::backend::input::{InputEvent, InputTime, KeyState};

    let mut f = set_up(&format!(
        r##"{CONFIG}
        screenshot {{ brush {{ color "#ff0000"; palette "#ff0000" "#ff0000" "#0080ff"; }}; }}
        binds {{
            Tab {{ screenshot-focus-next; }}
            BackSpace {{ screenshot-focus-previous; }}
            Return {{ screenshot-confirm; }}
        }}"##
    ));
    open_ui(&mut f);
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    command(&mut f, Action::ScreenshotFocusNext);
    command(&mut f, Action::ScreenshotFocusNext);
    command(&mut f, Action::ScreenshotTogglePalette);
    assert_eq!(focus(&mut f), Some(Control::Color(0)));

    let key = |f: &mut Fixture, code: u32| {
        for state in [KeyState::Pressed, KeyState::Released] {
            f.niri_state()
                .process_input_event::<TestInputBackend>(InputEvent::Keyboard {
                    event: TestKeyboardKeyEvent {
                        time: InputTime::from_micros(0),
                        code: code.into(),
                        state,
                        count: 1,
                    },
                });
        }
    };
    key(&mut f, 23); // Tab reaches the second red swatch.
    assert_eq!(focus(&mut f), Some(Control::Color(1)));
    f.niri().config.borrow_mut().screenshot.tooltips.enabled = false;
    f.niri()
        .screenshot_ui
        .refresh_config(niri_config::ModKey::Super);
    key(&mut f, 23);
    key(&mut f, 36); // Return selects blue, including across an unrelated reload.
    assert_eq!(
        with_editor(&mut f, |e| e.color),
        Color {
            r: 0,
            g: 128,
            b: 255
        }
    );

    command(&mut f, Action::ScreenshotSetPalette { open: true });
    key(&mut f, 22); // BackSpace traverses both red swatches in reverse.
    assert_eq!(focus(&mut f), Some(Control::Color(1)));
    key(&mut f, 22);
    assert_eq!(focus(&mut f), Some(Control::Color(0)));
}
