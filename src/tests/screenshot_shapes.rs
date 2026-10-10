use super::*;

#[test]
fn context_honors_keyboard_focus_until_pointer_moves() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Set(1),
        },
    );
    hover_control(&mut f, &output, Control::Tool(Tool::Draw));
    let selected = with_editor(&mut f, |e| e.tool);
    let brush_width = with_editor(&mut f, |e| e.width);
    for _ in 0..3 {
        command(&mut f, Action::ScreenshotFocusNext);
    }
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Arrow)));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(12),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| (
            e.tool,
            e.width,
            e.arrow_width,
            e.magnifier_zoom.level()
        )),
        (selected, brush_width, 12, 1)
    );
    command(&mut f, Action::ScreenshotFocusPrevious);
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(7),
        },
    );
    assert_eq!(with_editor(&mut f, |e| (e.width, e.arrow_width)), (7, 12));
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    assert_eq!(focus(&mut f), None);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| (e.width, e.magnifier_zoom.level())),
        (7, 2)
    );
    command(&mut f, Action::ScreenshotFocusPrevious);
    assert_eq!(focus(&mut f), Some(Control::Save));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 4);
    f.niri().screenshot_ui.pointer_motion((-1, -1).into(), None);
    for _ in 0..2 {
        command(&mut f, Action::ScreenshotFocusNext);
    }
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Draw)));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.width), 8);
    for _ in 0..3 {
        command(&mut f, Action::ScreenshotFocusNext);
    }
    assert_eq!(focus(&mut f), Some(Control::Tool(Tool::Blur)));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(3),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| (e.tool, e.blur_strength)),
        (selected, niri_config::BlurStrength::High)
    );
}

#[test]
fn context_targets_hovered_tools_without_changing_selection() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    let zoom = with_editor(&mut f, |e| e.magnifier_zoom.level());
    for (tool, value) in [
        (Tool::Draw, 9),
        (Tool::Arrow, 15),
        (Tool::Rectangle, 23),
        (Tool::Blur, 128),
    ] {
        hover_control(&mut f, &output, Control::Tool(tool));
        command(
            &mut f,
            Action::ScreenshotContext {
                change: ScreenshotValueChange::Set(value),
            },
        );
        assert_eq!(with_editor(&mut f, |e| e.tool), Tool::Draw);
        assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), zoom);
    }
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(-1),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| (
            e.width,
            e.arrow_width,
            e.rectangle_width,
            e.blur_strength
        )),
        (9, 15, 23, niri_config::BlurStrength::Medium)
    );
    command(&mut f, Action::ScreenshotTool { tool: Tool::Arrow });
    hover_control(&mut f, &output, Control::Tool(Tool::Arrow));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    hover_control(&mut f, &output, Control::Tool(Tool::Select));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(1),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| (
            e.width,
            e.arrow_width,
            e.rectangle_width,
            e.blur_strength
        )),
        (9, 16, 23, niri_config::BlurStrength::Medium)
    );
    assert_eq!(with_editor(&mut f, |e| e.tool), Tool::Arrow);
}

#[test]
fn context_blocks_overlay_but_zooms_canvas_and_explicit_zoom_bypasses_hover() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Set(2),
        },
    );
    let before = with_editor(&mut f, |e| {
        (e.width, e.arrow_width, e.rectangle_width, e.blur_strength)
    });
    f.niri().screenshot_ui.pointer_motion((-1, -1).into(), None);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    for control in [
        Control::Copy,
        Control::Axes,
        Control::Undo,
        Control::Tool(Tool::Select),
    ] {
        hover_control(&mut f, &output, control);
        command(
            &mut f,
            Action::ScreenshotContext {
                change: ScreenshotValueChange::Adjust(1),
            },
        );
        assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
        assert_eq!(
            with_editor(&mut f, |e| (
                e.width,
                e.arrow_width,
                e.rectangle_width,
                e.blur_strength
            )),
            before
        );
    }
    let (size, scale, _) = f.niri().screenshot_ui.output_size(&output).unwrap();
    let rect = screenshot_editor::control_rect(
        0,
        size,
        scale,
        &f.niri().config.borrow().screenshot,
        Tool::Draw,
    );
    f.niri()
        .screenshot_ui
        .pointer_motion((rect.loc.x - 1, rect.loc.y + rect.size.h / 2).into(), None);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    command(&mut f, Action::ScreenshotSetPalette { open: true });
    let color = color_center(&mut f, &output, 0);
    f.niri().screenshot_ui.pointer_motion(color, None);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    assert!(palette_open(&mut f));
    hover_control(&mut f, &output, Control::Copy);
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 4);
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 8);
    assert_eq!(
        with_editor(&mut f, |e| (
            e.width,
            e.arrow_width,
            e.rectangle_width,
            e.blur_strength
        )),
        before
    );
}

#[test]
fn context_during_gesture_zooms_canvas_without_changing_tool_parameters() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    command(&mut f, Action::ScreenshotTool { tool: Tool::Draw });
    command(
        &mut f,
        Action::ScreenshotMagnifierZoom {
            zoom: ScreenshotValueChange::Set(1),
        },
    );
    let before = with_editor(&mut f, |e| {
        (e.width, e.arrow_width, e.rectangle_width, e.blur_strength)
    });
    assert!(f
        .niri()
        .screenshot_ui
        .pointer_down(output.clone(), (300, 300).into(), None, false));
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    hover_control(&mut f, &output, Control::Tool(Tool::Arrow));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(1),
        },
    );
    assert_eq!(with_editor(&mut f, |e| e.magnifier_zoom.level()), 2);
    assert_eq!(
        with_editor(&mut f, |e| (
            e.width,
            e.arrow_width,
            e.rectangle_width,
            e.blur_strength
        )),
        before
    );
    assert!(f.niri().screenshot_ui.has_active_gesture());
    command(&mut f, Action::ScreenshotCancel);
}

#[test]
fn independent_shapes_capture_and_undo() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    command(
        &mut f,
        Action::ScreenshotArrowColor {
            color: Color::new(0, 255, 0),
        },
    );
    f.niri().screenshot_ui.set_tool(Tool::Arrow);
    hover_control(&mut f, &output, Control::Tool(Tool::Arrow));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(8),
        },
    );
    command(
        &mut f,
        Action::ScreenshotRectangleColor {
            color: Color::new(0, 0, 255),
        },
    );
    f.niri().screenshot_ui.set_tool(Tool::Rectangle);
    hover_control(&mut f, &output, Control::Tool(Tool::Rectangle));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Set(6),
        },
    );
    f.niri().screenshot_ui.set_tool(Tool::Arrow);
    gesture(&mut f, &output, &[(200, 250), (400, 250)]);
    let (_, arrow) = capture(&mut f);
    assert!(near(px(size, &arrow, 300, 250), [0, 255, 0, 255], 2));
    assert!(near(px(size, &arrow, 300, 253), [0, 255, 0, 255], 2));
    assert_eq!(px(size, &arrow, 300, 256), px(size, &original, 300, 256));
    assert!(has_color_near(
        size,
        &arrow,
        385,
        240,
        8,
        [0, 255, 0, 255],
        2
    ));
    f.niri().screenshot_ui.set_tool(Tool::Rectangle);
    gesture(&mut f, &output, &[(500, 350), (700, 500)]);
    let (_, rectangle) = capture(&mut f);
    assert!(near(px(size, &rectangle, 600, 350), [0, 0, 255, 255], 2));
    assert_eq!(
        px(size, &rectangle, 600, 400),
        px(size, &original, 600, 400)
    );
    f.niri().screenshot_ui.undo();
    assert_eq!(capture(&mut f).1, arrow);
    f.niri().screenshot_ui.redo();
    assert_eq!(capture(&mut f).1, rectangle);
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    gesture(&mut f, &output, &[(200, 550), (400, 550)]);
    let (_, brush) = capture(&mut f);
    assert!(near(px(size, &brush, 300, 550), [255, 0, 0, 255], 2));
    assert_eq!(
        with_editor(&mut f, |e| e.tool_color(Tool::Arrow)),
        Color::new(0, 255, 0)
    );
    assert_eq!(with_editor(&mut f, |e| e.tool_width(Tool::Rectangle)), 6);
}

#[test]
fn rectangle_axis_hold_reanchors_without_finishing_figure() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    f.niri().screenshot_ui.set_tool(Tool::Rectangle);
    let key = 42u32.into();
    let ui = &mut f.niri().screenshot_ui;
    ui.begin_axes_hold(key);
    assert!(ui.pointer_down(output.clone(), (200, 200).into(), None, false));
    ui.pointer_motion((450, 220).into(), None);
    ui.end_axes_hold(key);
    ui.begin_axes_hold(key);
    ui.pointer_motion((454, 420).into(), None);
    assert!(ui.has_active_gesture());
    assert_eq!(ui.pointer_up(None), Some(EditorOutcome::Changed));
    ui.end_axes_hold(key);
    let (_, pixels) = capture(&mut f);
    assert!(near(px(size, &pixels, 300, 200), [255, 0, 0, 255], 2));
    assert!(near(px(size, &pixels, 450, 300), [255, 0, 0, 255], 2));
    assert!(near(px(size, &pixels, 300, 400), [255, 0, 0, 255], 2));
    assert_eq!(px(size, &pixels, 300, 300), px(size, &original, 300, 300));
    assert_eq!(with_editor(&mut f, |e| e.history.len()), 1);
    f.niri().screenshot_ui.undo();
    assert_eq!(capture(&mut f).1, original);
}

#[test]
fn axes_corner_does_not_cut_diagonal_and_cancel_discards_shape() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    command(&mut f, Action::ScreenshotToggleAxes(false));
    gesture(&mut f, &output, &[(300, 400), (450, 402), (297, 300)]);
    let (_, pixels) = capture(&mut f);
    assert!(near(px(size, &pixels, 380, 400), [255, 0, 0, 255], 2));
    assert!(near(px(size, &pixels, 300, 350), [255, 0, 0, 255], 2));
    assert_eq!(px(size, &pixels, 375, 350), px(size, &original, 375, 350));
    assert_eq!(px(size, &pixels, 290, 400), px(size, &original, 290, 400));
    f.niri().screenshot_ui.set_tool(Tool::Arrow);
    let ui = &mut f.niri().screenshot_ui;
    assert!(ui.pointer_down(output, (500, 400).into(), None, false));
    ui.pointer_motion((700, 400).into(), None);
    command(&mut f, Action::ScreenshotCancel);
    assert_eq!(capture(&mut f).1, pixels);
}

#[test]
fn axis_guides_and_brush_cursor_never_enter_export() {
    let mut f = set_up(&format!(
        r##"{CONFIG}
screenshot {{ axes {{ color "#00ffff"; opacity 0.5; width 2; dash-length 8; dash-gap 4; }}; }}"##
    ));
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    f.niri()
        .screenshot_ui
        .pointer_motion((400, 300).into(), None);
    command(&mut f, Action::ScreenshotSetAxes { enabled: true });
    let (_, on) = render_output_rgba(f.niri_state(), &output);
    command(&mut f, Action::ScreenshotSetAxes { enabled: false });
    let (_, off) = render_output_rgba(f.niri_state(), &output);
    assert_ne!(px(size, &on, 242, 300), px(size, &off, 242, 300));
    assert_ne!(px(size, &on, 400, 242), px(size, &off, 400, 242));
    assert_eq!(capture(&mut f).1, original);
}

#[test]
fn blur_strength_clamps_and_preserves_completed_edits() {
    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    let id = f.add_client();
    open_window(&mut f, id, "left", 480, 720, [u32::MAX, 0, 0, u32::MAX]);
    open_window(&mut f, id, "right", 480, 720, [0, u32::MAX, 0, u32::MAX]);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    let (size, original) = capture(&mut f);
    f.niri().screenshot_ui.set_tool(Tool::Blur);
    hover_control(&mut f, &output, Control::Tool(Tool::Blur));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(i32::MAX),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| e.blur_strength),
        niri_config::BlurStrength::High
    );
    gesture(&mut f, &output, &[(300, 300), (600, 450)]);
    let before = capture(&mut f).1;
    assert_ne!(px(size, &before, 479, 350), px(size, &original, 479, 350));
    hover_control(&mut f, &output, Control::Tool(Tool::Blur));
    command(
        &mut f,
        Action::ScreenshotContext {
            change: ScreenshotValueChange::Adjust(i32::MIN),
        },
    );
    assert_eq!(
        with_editor(&mut f, |e| e.blur_strength),
        niri_config::BlurStrength::Low
    );
    assert_eq!(capture(&mut f).1, before);
}

#[test]
fn shapes_and_guides_preserve_frozen_coordinates_across_scale_and_rotation() {
    for (scale, transform) in [(1.5, "normal"), (2., "90")] {
        let mut f = set_up(&format!(
            r#"{CONFIG} output "headless-1" {{ scale {scale}; transform "{transform}"; }}"#
        ));
        let output = f.niri_output(1);
        open_ui(&mut f);
        select(&mut f, &output, (100, 200), (600, 600));
        let (size, original) = capture(&mut f);
        f.niri().screenshot_ui.set_tool(Tool::Arrow);
        command(&mut f, Action::ScreenshotToggleAxes(false));
        gesture(&mut f, &output, &[(200, 300), (500, 330)]);
        let (_, arrow) = capture(&mut f);
        assert!(near(px(size, &arrow, 250, 100), [255, 0, 0, 255], 2));
        assert_eq!(px(size, &arrow, 250, 130), px(size, &original, 250, 130));
        f.niri().screenshot_ui.set_tool(Tool::Rectangle);
        command(&mut f, Action::ScreenshotToggleAxes(false));
        gesture(&mut f, &output, &[(200, 400), (500, 550)]);
        let (_, rectangle) = capture(&mut f);
        assert!(near(px(size, &rectangle, 250, 200), [255, 0, 0, 255], 2));
        assert_eq!(
            px(size, &rectangle, 250, 250),
            px(size, &original, 250, 250)
        );
        command(&mut f, Action::ScreenshotToggleAxes(false));
        f.niri()
            .screenshot_ui
            .pointer_motion((400, 350).into(), None);
        render_output_rgba(f.niri_state(), &output);
        assert_eq!(capture(&mut f).1, rectangle);
    }
}

#[test]
fn axes_holds_end_on_device_removal_without_changing_toggle() {
    use smithay::backend::input::InputEvent;

    use crate::tests::test_input_backend::{TestInputBackend, TestInputDevice};
    use crate::ui::screenshot_axes::HoldButton;

    let mut f = set_up(CONFIG);
    let output = f.niri_output(1);
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    for toggled in [false, true] {
        command(&mut f, Action::ScreenshotSetAxes { enabled: toggled });
        f.niri().screenshot_ui.begin_axes_hold(42u32.into());
        f.niri()
            .screenshot_ui
            .set_axes_hold(HoldButton::Pointer(273), true);
        assert!(with_editor(&mut f, |e| e.axes_enabled));
        f.niri_state()
            .process_input_event::<TestInputBackend>(InputEvent::DeviceRemoved {
                device: TestInputDevice,
            });
        assert_eq!(with_editor(&mut f, |e| e.axes_enabled), toggled);
        command(&mut f, Action::ScreenshotSetAxes { enabled: false });
        assert!(!with_editor(&mut f, |e| e.axes_enabled));
    }
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    gesture(&mut f, &output, &[(300, 400), (600, 550)]);
    let (size, pixels) = capture(&mut f);
    assert!(has_color_near(
        size,
        &pixels,
        450,
        475,
        1,
        [255, 0, 0, 255],
        2
    ));
    assert!(!near(px(size, &pixels, 450, 400), [255, 0, 0, 255], 2));
}

#[test]
fn axes_hold_ends_when_virtual_pointer_is_destroyed_without_release() {
    use wayland_client::protocol::wl_pointer;

    let mut f = set_up(&format!(
        r#"{CONFIG}
        binds {{ MouseRight {{ screenshot-toggle-axes hold=true; }}; }}"#
    ));
    let output = f.niri_output(1);
    let id = f.add_client();
    let client = f.client(id);
    let pointer = client
        .state
        .virtual_pointer_manager
        .as_ref()
        .unwrap()
        .create_virtual_pointer(None, &client.qh, ());
    open_ui(&mut f);
    select(&mut f, &output, (0, 0), (959, 719));
    f.niri().screenshot_ui.set_tool(Tool::Draw);
    pointer.button(0, 273, wl_pointer::ButtonState::Pressed);
    pointer.frame();
    f.roundtrip(id);
    assert!(with_editor(&mut f, |e| e.axes_enabled));
    pointer.destroy();
    f.roundtrip(id);
    assert!(!with_editor(&mut f, |e| e.axes_enabled));
    gesture(&mut f, &output, &[(300, 400), (600, 550)]);
    let (size, pixels) = capture(&mut f);
    assert!(has_color_near(
        size,
        &pixels,
        450,
        475,
        1,
        [255, 0, 0, 255],
        2
    ));
}

#[test]
fn axes_reanchor_uses_the_visible_arrow_endpoint() {
    for (before_toggle, after_toggle, endpoint) in [(250, 350, 150), (50, 150, 100)] {
        let mut f = set_up(CONFIG);
        let output = f.niri_output(1);
        open_ui(&mut f);
        select(&mut f, &output, (0, 0), (959, 719));
        f.niri().screenshot_ui.set_tool(Tool::Arrow);
        command(&mut f, Action::ScreenshotSetAxes { enabled: true });
        f.niri()
            .screenshot_ui
            .pointer_down(output.clone(), (100, 100).into(), None, false);
        f.niri()
            .screenshot_ui
            .pointer_motion((550, 300).into(), None);
        command(&mut f, Action::ScreenshotSetAxes { enabled: false });
        f.niri()
            .screenshot_ui
            .pointer_motion((550, before_toggle).into(), None);
        command(&mut f, Action::ScreenshotSetAxes { enabled: true });
        f.niri()
            .screenshot_ui
            .pointer_motion((550, after_toggle).into(), None);
        f.niri().screenshot_ui.pointer_up(None);
        let actual = capture(&mut f);

        command(&mut f, Action::ScreenshotUndo);
        command(&mut f, Action::ScreenshotSetAxes { enabled: false });
        gesture(&mut f, &output, &[(100, 100), (550, endpoint)]);
        assert_eq!(actual, capture(&mut f));
    }
}
