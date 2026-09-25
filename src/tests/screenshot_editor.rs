use niri_config::Action;
use smithay::input::keyboard::{Keysym, ModifiersState};
use smithay::output::Output;
use smithay::utils::{Physical, Point, Rectangle, Size};

use super::fixture::Fixture;
use super::knit::{open_window, render_output_rgba};
use crate::ui::screenshot_editor::{self, Color, Control, Editor, EditorOutcome, Tool};
use crate::ui::screenshot_ui::ScreenshotUi;

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
    f.niri_state().open_screenshot_ui(false, None);
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

/// Sends a key press through the editor's local key handler. The outer `None`
/// means unhandled: input falls through to the common screenshot `action()`.
fn key(f: &mut Fixture, raw: Keysym, mods: ModifiersState) -> Option<Option<Action>> {
    f.niri().screenshot_ui.handle_editor_key(raw, mods)
}

fn no_mods() -> ModifiersState {
    ModifiersState::default()
}

fn shift() -> ModifiersState {
    ModifiersState {
        shift: true,
        ..Default::default()
    }
}

/// The compositor action a key produces through the common fallback path.
fn fallback_action(f: &mut Fixture, raw: Keysym, mods: ModifiersState) -> Option<Action> {
    f.niri().screenshot_ui.action(raw, mods)
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
    let rect = screenshot_editor::control_rect(index, size, scale);
    Point::from((rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2))
}

/// The output-local center of palette swatch `index`. Swatch geometry uses
/// `control_rect` indices starting right after `CONTROLS`.
fn color_center(f: &mut Fixture, output: &Output, index: usize) -> Point<i32, Physical> {
    let (size, scale, _) = f.niri().screenshot_ui.output_size(output).unwrap();
    let rect =
        screenshot_editor::control_rect(screenshot_editor::CONTROLS.len() + index, size, scale);
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
    f.niri().screenshot_ui.set_color(Color::Red);
    // A single-point gesture draws a dot.
    gesture(&mut f, &output, &[(480, 300)]);
    // A multi-point gesture draws a stroke.
    gesture(&mut f, &output, &[(300, 400), (460, 400), (620, 400)]);
    f.niri().screenshot_ui.set_color(Color::Blue);
    gesture(&mut f, &output, &[(600, 300)]);

    f.niri().screenshot_ui.set_tool(Tool::Select);
    select(&mut f, &output, (100, 100), (740, 620));

    let (size, pixels) = capture(&mut f);
    assert_eq!(size, Size::from((641, 521)));

    let red = rgba(Color::Red);
    let blue = rgba(Color::Blue);
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
    f.niri().screenshot_ui.set_color(Color::Red);
    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();
    let draw = screenshot_editor::CONTROLS
        .iter()
        .position(|control| *control == Control::Tool(Tool::Draw))
        .unwrap();
    let rect = screenshot_editor::control_rect(draw, size, scale);
    let center = Point::from((rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2));

    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), center, None, false));
    assert_eq!(ui.pointer_up(None), Some(EditorOutcome::Changed));
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
            rgba(Color::Blue),
            2
        ));
        assert!(!near(
            px(
                size,
                &rendered,
                rect.loc.x + x,
                rect.loc.y + rect.size.h - 5
            ),
            rgba(Color::Blue),
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
        rgba(Color::Blue),
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
    let red = rgba(Color::Red);
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
    f.niri().screenshot_ui.set_color(Color::Black);
    gesture(&mut f, &output, &[(340, 360), (620, 360)]);
    let (_, stroked) = capture(&mut f);
    assert!(has_color_near(
        size2,
        &stroked,
        400,
        280,
        2,
        rgba(Color::Black),
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
    f.niri().screenshot_ui.set_color(Color::Blue);
    gesture(&mut f, &output, &[(200, 360), (760, 360)]);
    let (size, image) = capture(&mut f);

    f.niri().screenshot_ui.set_tool(Tool::Select);
    select(&mut f, &output, (400, 200), (700, 500));
    // Hover the toolbar so the magnifier does not cover the sampled pixels.
    let toolbar = screenshot_editor::control_rect(0, size, 1.);
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
    let toolbar = screenshot_editor::control_rect(0, size, 1.);
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
    assert!(f.niri().screenshot_ui.scroll_magnifier(-240.));
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
    assert!(!f.niri().screenshot_ui.scroll_magnifier(-60.));
    assert!(f.niri().screenshot_ui.scroll_magnifier(-60.));
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
fn screenshot_magnifier_scroll_limits_reverse_and_reopen() {
    let mut f = set_up(CONFIG);
    open_ui(&mut f);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.scroll_magnifier(-12000.));
    assert!(!ui.scroll_magnifier(-120.));
    assert!(ui.scroll_magnifier(120.));
    assert!(ui.scroll_magnifier(12000.));
    assert!(!ui.scroll_magnifier(120.));
    assert!(!ui.scroll_magnifier(-60.));
    assert!(!ui.scroll_magnifier(60.));
    assert!(!ui.scroll_magnifier(-60.));
    assert!(ui.scroll_magnifier(-60.));
    ui.close();
    assert!(!ui.scroll_magnifier(-120.));
    open_ui(&mut f);
    // Reopening resets to the hidden 1x state rather than retaining the old zoom.
    assert!(!f.niri().screenshot_ui.scroll_magnifier(120.));
    let output = f.niri_output(1);
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    let (size, hidden) = render_output_rgba(f.niri_state(), &output);
    assert!(f.niri().screenshot_ui.scroll_magnifier(-120.));
    let (_, visible) = render_output_rgba(f.niri_state(), &output);
    // Moving the pointer cannot change the displayed image while the lens is off.
    assert!(f.niri().screenshot_ui.scroll_magnifier(120.));
    f.niri()
        .screenshot_ui
        .pointer_motion((200, 200).into(), None);
    let (_, hidden_again) = render_output_rgba(f.niri_state(), &output);
    assert_eq!(hidden, hidden_again);
    // The center outline proves the lens reappears at 2x, not just its model state.
    assert!(!near(
        px(size, &visible, 512, 412),
        px(size, &hidden, 512, 412),
        8
    ));
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
        rgba(Color::Red),
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
        rgba(Color::Red),
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
        screenshot_editor::COLORS[4]
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
    assert!(with_editor(&mut f, |e| e.gesture.is_none()));
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
        rgba(screenshot_editor::COLORS[4]),
        2
    ));

    // Toolbar padding (the gap between control groups) is overlay, not
    // canvas: with the popup open it dismisses without an edit.
    let blur_rect = screenshot_editor::control_rect(2, size, scale);
    let undo_rect = screenshot_editor::control_rect(3, size, scale);
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

/// Keyboard tool switching, Tab focus cycling over enabled controls, digit
/// swatch selection and Enter activation through `handle_editor_key`.
#[test]
fn screenshot_editor_keyboard_tools_focus_digits_and_enter_outcomes() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);

    // The palette key is gated on the Draw tool: dead in Select.
    assert_eq!(key(&mut f, Keysym::c, no_mods()), Some(None));
    assert!(!palette_open(&mut f));

    // Tool keys work regardless of focus.
    assert_eq!(key(&mut f, Keysym::b, no_mods()), Some(None));
    assert_eq!(with_editor(&mut f, |e| e.tool), Tool::Draw);
    assert_eq!(key(&mut f, Keysym::r, no_mods()), Some(None));
    assert_eq!(with_editor(&mut f, |e| e.tool), Tool::Blur);
    assert_eq!(key(&mut f, Keysym::v, no_mods()), Some(None));
    assert_eq!(with_editor(&mut f, |e| e.tool), Tool::Select);
    // Modifier chords stay compositor actions.
    let ctrl = ModifiersState {
        ctrl: true,
        ..Default::default()
    };
    assert!(key(&mut f, Keysym::b, ctrl).is_none());

    // Draw re-enables the palette key.
    assert_eq!(key(&mut f, Keysym::b, no_mods()), Some(None));

    // Digits without the popup are unhandled and reach no fallback action.
    // Digits without the popup are unhandled.
    assert!(key(&mut f, Keysym::_2, no_mods()).is_none());

    // Tab walks only enabled controls: with Draw active and empty history the
    // ring is Select, Draw, Blur, Pointer, Copy, Save.
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Select)));
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));
    // Enter on the brush opens its palette; Escape returns focus to the brush.
    assert_eq!(key(&mut f, Keysym::Return, no_mods()), Some(None));
    assert!(palette_open(&mut f));
    assert_eq!(key(&mut f, Keysym::Escape, no_mods()), Some(None));
    assert_eq!(with_editor(&mut f, |e| e.tool), Tool::Draw);
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));

    // Shift+Tab walks backwards and wraps to the last enabled control.
    assert_eq!(key(&mut f, Keysym::Tab, shift()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Select)));
    assert_eq!(key(&mut f, Keysym::Tab, shift()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Save));

    // Enter on focused Save/Copy requests compositor outcomes.
    assert!(matches!(
        key(&mut f, Keysym::Return, no_mods()),
        Some(Some(Action::ConfirmScreenshot {
            write_to_disk: true
        }))
    ));
    // Returning an outcome does not close the UI by itself.
    assert!(f.niri().screenshot_ui.is_open());
    assert_eq!(focus(&mut f), Some(Control::Save));
    // Forward wrap lands back on the first control.
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Select)));
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));

    // Enter on Draw opens the popup and focuses the current swatch;
    // swatches are appended to the focus ring while it is open.
    assert_eq!(key(&mut f, Keysym::Return, no_mods()), Some(None));
    assert!(palette_open(&mut f));
    assert_eq!(
        focus(&mut f),
        Some(Control::Color(screenshot_editor::COLORS[0]))
    );
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(
        focus(&mut f),
        Some(Control::Color(screenshot_editor::COLORS[1]))
    );
    // Enter picks the swatch, closes the popup and returns focus to Draw.
    assert_eq!(key(&mut f, Keysym::Return, no_mods()), Some(None));
    assert!(!palette_open(&mut f));
    assert_eq!(
        with_editor(&mut f, |e| e.color),
        screenshot_editor::COLORS[1]
    );
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));

    // Reopening through the key focuses the *current* color, not the first.
    assert_eq!(key(&mut f, Keysym::c, no_mods()), Some(None));
    assert!(palette_open(&mut f));
    assert_eq!(
        focus(&mut f),
        Some(Control::Color(screenshot_editor::COLORS[1]))
    );
    // Digits pick a swatch, close the popup and park focus on Draw.
    assert_eq!(key(&mut f, Keysym::_5, no_mods()), Some(None));
    assert!(!palette_open(&mut f));
    assert_eq!(
        with_editor(&mut f, |e| e.color),
        screenshot_editor::COLORS[4]
    );
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));

    // Enter on focused Copy reports the copy outcome.
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Blur)));
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Copy));
    assert!(matches!(
        key(&mut f, Keysym::Return, no_mods()),
        Some(Some(Action::ConfirmScreenshot {
            write_to_disk: false
        }))
    ));

    // Pointer motion drops keyboard focus; Enter with no focus falls through
    // instead of activating a control.
    f.niri()
        .screenshot_ui
        .pointer_motion((480, 400).into(), None);
    assert_eq!(focus(&mut f), None);
    assert!(key(&mut f, Keysym::Return, no_mods()).is_none());

    // With a committed edit Undo joins the focus ring and can be activated.
    gesture(&mut f, &output, &[(480, 360)]);
    let (_, baseline) = capture(&mut f);
    f.niri().screenshot_ui.set_color(Color::Blue);
    gesture(&mut f, &output, &[(600, 400)]);
    // Enabled ring: Select, Draw, Blur, Undo, Pointer, Copy, Save.
    for _ in 0..4 {
        assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    }
    assert_eq!(focus(&mut f), Some(Control::Undo));
    // Undoing the newer dot leaves one edit in history, so Undo stays enabled
    // and Redo takes the ring slot right behind it.
    assert_eq!(key(&mut f, Keysym::Return, no_mods()), Some(None));
    let (_, pixels) = capture(&mut f);
    assert_eq!(pixels, baseline);
    assert_eq!(focus(&mut f), Some(Control::Undo));
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(focus(&mut f), Some(Control::Redo));
    assert_eq!(key(&mut f, Keysym::Return, no_mods()), Some(None));
    let (size2, pixels) = capture(&mut f);
    // Relative to the default quarter-selection: (600,400) -> (360,220).
    assert!(has_color_near(
        size2,
        &pixels,
        360,
        220,
        2,
        rgba(Color::Blue),
        2
    ));
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
    assert_eq!(key(&mut f, Keysym::c, no_mods()), Some(None));
    assert!(palette_open(&mut f));
    assert_eq!(key(&mut f, Keysym::Escape, no_mods()), Some(None));
    assert!(!palette_open(&mut f));
    // Focus was not on a swatch, so it stays unset.
    assert_eq!(focus(&mut f), None);

    // Layer 2: a Draw gesture is cancelled, not committed.
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (400, 300).into(), None, false));
    ui.pointer_motion((460, 300).into(), None);
    // Tab is a no-op mid-gesture instead of moving focus under the stroke.
    assert_eq!(key(&mut f, Keysym::Tab, no_mods()), Some(None));
    assert_eq!(key(&mut f, Keysym::Escape, no_mods()), Some(None));
    assert!(with_editor(&mut f, |e| e.gesture.is_none()));
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 0);
    // The matching release reaches nobody.
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);

    // Layer 3: a Select drag restores the crop it started from.
    f.niri().screenshot_ui.set_tool(Tool::Select);
    let before = f.niri().screenshot_ui.selection_points().unwrap();
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), (300, 400).into(), None, false));
    ui.pointer_motion((500, 600).into(), None);
    assert_eq!(key(&mut f, Keysym::Escape, no_mods()), Some(None));
    assert_eq!(f.niri().screenshot_ui.selection_points().unwrap(), before);
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);

    // Idle: Escape is unhandled locally and falls through to cancel.
    assert!(key(&mut f, Keysym::Escape, no_mods()).is_none());
    assert!(matches!(
        fallback_action(&mut f, Keysym::Escape, no_mods()),
        Some(Action::CancelScreenshot)
    ));
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
    assert!(with_editor(&mut f, |e| e.gesture.is_none()));
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
    assert_eq!(key(&mut f, Keysym::Escape, no_mods()), Some(None));
    assert_eq!(f.niri().screenshot_ui.selection_points().unwrap(), moved);
    assert_eq!(f.niri().screenshot_ui.pointer_up(None), None);
    f.niri().screenshot_ui.set_space_down(false);

    // Space itself is consumed locally: it reaches no save fallback.
    assert_eq!(key(&mut f, Keysym::space, no_mods()), Some(None));
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
        .map(|i| screenshot_editor::control_rect(i, size, scale))
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
    assert_eq!(key(&mut f, Keysym::Escape, no_mods()), Some(None));
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
    let blur_rect = screenshot_editor::control_rect(2, size, scale);
    let undo_rect = screenshot_editor::control_rect(3, size, scale);
    let padding = Point::from((
        (blur_rect.loc.x + blur_rect.size.w + undo_rect.loc.x) / 2,
        blur_rect.loc.y + blur_rect.size.h / 2,
    ));
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output.clone(), padding, None, false));
    assert!(with_editor(&mut f, |e| {
        matches!(e.gesture, Some(screenshot_editor::Gesture::Overlay { .. }))
    }));
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
        .map(|i| screenshot_editor::control_rect(i, size, scale))
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

    // The 'p' shortcut stays a compositor action, not an editor-local key.
    assert!(key(&mut f, Keysym::p, no_mods()).is_none());
}
