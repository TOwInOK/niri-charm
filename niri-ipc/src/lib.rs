//! Types for communicating with niri via IPC.
//!
//! After connecting to the niri socket, you can send [`Request`]s. Niri will process them one by
//! one, in order, and to each request it will respond with a single [`Reply`], which is a `Result`
//! wrapping a [`Response`].
//!
//! If you send a [`Request::EventStream`], niri will *stop* reading subsequent [`Request`]s, and
//! will start continuously writing compositor [`Event`]s to the socket. If you'd like to read an
//! event stream and write more requests at the same time, you need to use two IPC sockets.
//!
//! <div class="warning">
//!
//! Requests are *always* processed separately. Time passes between requests, even when sending
//! multiple requests to the socket at once. For example, sending [`Request::Workspaces`] and
//! [`Request::Windows`] together may not return consistent results (e.g. a window may open on a
//! new workspace in-between the two responses). This goes for actions too: sending
//! [`Action::FocusWindow`] and <code>[Action::CloseWindow] { id: None }</code> together may close
//! the wrong window because a different window got focused in-between these requests.
//!
//! </div>
//!
//! You can use the [`socket::Socket`] helper if you're fine with blocking communication. However,
//! it is a fairly simple helper, so if you need async, or if you're using a different language,
//! you are encouraged to communicate with the socket manually.
//!
//! 1. Read the socket filesystem path from [`socket::SOCKET_PATH_ENV`] (`$NIRI_SOCKET`).
//! 2. Connect to the socket and write a JSON-formatted [`Request`] on a single line. You can follow
//!    up with a line break and a flush, or just flush and shutdown the write end of the socket.
//! 3. Niri will respond with a single line JSON-formatted [`Reply`].
//! 4. You can keep writing [`Request`]s, each on a single line, and read [`Reply`]s, also each on a
//!    separate line.
//! 5. After you request an event stream, niri will keep responding with JSON-formatted [`Event`]s,
//!    on a single line each.
//!
//! ## Backwards compatibility
//!
//! This crate follows the niri version. It is **not** API-stable in terms of the Rust semver. In
//! particular, expect new struct fields and enum variants to be added in patch version bumps.
//!
//! Use an exact version requirement to avoid breaking changes:
//!
//! ```toml
//! [dependencies]
//! niri-ipc = "=26.4.0"
//! ```
//!
//! ## Features
//!
//! This crate defines the following features:
//! - `json-schema`: derives the [schemars](https://lib.rs/crates/schemars) `JsonSchema` trait for
//!   the types.
//! - `clap`: derives the clap CLI parsing traits for some types. Used internally by niri itself.
#![warn(missing_docs)]

use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub mod socket;
pub mod state;

/// Request from client to niri.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Request {
    /// Request the version string for the running niri instance.
    Version,
    /// Request information about connected outputs.
    Outputs,
    /// Request information about workspaces.
    Workspaces,
    /// Request information about open windows.
    Windows,
    /// Request information about layer-shell surfaces.
    Layers,
    /// Request information about the configured keyboard layouts.
    KeyboardLayouts,
    /// Request information about the focused output.
    FocusedOutput,
    /// Request information about the focused window.
    FocusedWindow,
    /// Request picking a window and get its information.
    PickWindow,
    /// Request picking a color from the screen.
    PickColor,
    /// Request selecting a region on an output.
    ///
    /// The user drags a rectangle on one output with the mouse. The reply is
    /// [`Response::SelectedRegion`] with `Some` on success, or `None` if the selection was
    /// cancelled.
    SelectRegion,
    /// Query or manipulate the fixed region frame shown on an output.
    RegionFrame(RegionFrameCommand),
    /// Perform an action.
    Action(Action),
    /// Change output configuration temporarily.
    ///
    /// The configuration is changed temporarily and not saved into the config file. If the output
    /// configuration subsequently changes in the config file, these temporary changes will be
    /// forgotten.
    Output {
        /// Output name.
        output: String,
        /// Configuration to apply.
        action: OutputAction,
    },
    /// Start continuously receiving events from the compositor.
    ///
    /// The compositor should reply with `Reply::Ok(Response::Handled)`, then continuously send
    /// [`Event`]s, one per line.
    ///
    /// The event stream will always give you the full current state up-front. For example, the
    /// first workspace-related event you will receive will be [`Event::WorkspacesChanged`]
    /// containing the full current workspaces state. You *do not* need to separately send
    /// [`Request::Workspaces`] when using the event stream.
    ///
    /// Where reasonable, event stream state updates are atomic, though this is not always the
    /// case. For example, a window may end up with a workspace id for a workspace that had already
    /// been removed. This can happen if the corresponding [`Event::WorkspacesChanged`] arrives
    /// before the corresponding [`Event::WindowOpenedOrChanged`].
    EventStream,
    /// Respond with an error (for testing error handling).
    ReturnError,
    /// Request information about the overview.
    OverviewState,
    /// Request information about screencasts.
    Casts,
    /// Request the desktop zoom state of each output.
    Zoom,
}

/// Reply from niri to client.
///
/// Every request gets one reply.
///
/// * If an error had occurred, it will be an `Reply::Err`.
/// * If the request does not need any particular response, it will be
///   `Reply::Ok(Response::Handled)`. Kind of like an `Ok(())`.
/// * Otherwise, it will be `Reply::Ok(response)` with one of the other [`Response`] variants.
pub type Reply = Result<Response, String>;

/// Successful response from niri to client.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Response {
    /// A request that does not need a response was handled successfully.
    Handled,
    /// The version string for the running niri instance.
    Version(String),
    /// Information about connected outputs.
    ///
    /// Map from output name to output info.
    Outputs(HashMap<String, Output>),
    /// Information about workspaces.
    Workspaces(Vec<Workspace>),
    /// Information about open windows.
    Windows(Vec<Window>),
    /// Information about layer-shell surfaces.
    Layers(Vec<LayerSurface>),
    /// Information about the keyboard layout.
    KeyboardLayouts(KeyboardLayouts),
    /// Information about the focused output.
    FocusedOutput(Option<Output>),
    /// Information about the focused window.
    FocusedWindow(Option<Window>),
    /// Information about the picked window.
    PickedWindow(Option<Window>),
    /// Information about the picked color.
    PickedColor(Option<PickedColor>),
    /// The region selected by the user, or `None` if the selection was cancelled.
    SelectedRegion(Option<SelectedRegion>),
    /// The currently active region frame, or `None` if no frame is set.
    RegionFrame(Option<RegionFrameSpec>),
    /// Output configuration change result.
    OutputConfigChanged(OutputConfigChanged),
    /// Information about the overview.
    OverviewState(Overview),
    /// Information about screencasts.
    Casts(Vec<Cast>),
    /// Desktop zoom state of each output.
    Zoom(Vec<ZoomState>),
}

/// Overview information.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Overview {
    /// Whether the overview is currently open.
    pub is_open: bool,
}

/// Color picked from the screen.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct PickedColor {
    /// Color values as red, green, blue, each ranging from 0.0 to 1.0.
    pub rgb: [f64; 3],
}

/// A rectangle in logical coordinates.
///
/// Used both for output-local geometry (relative to an output's top-left corner) and for
/// geometry in the global compositor coordinate space.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct RegionGeometry {
    /// X coordinate of the region's top-left corner.
    pub x: i32,
    /// Y coordinate of the region's top-left corner.
    pub y: i32,
    /// Region width.
    pub width: u32,
    /// Region height.
    pub height: u32,
}

/// A rectangle on a specific output.
///
/// The [`geometry`](Self::geometry) is in output-local logical coordinates, relative to the
/// output's top-left corner.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct OutputRegion {
    /// Output name.
    pub output: String,
    /// Region geometry in output-local logical coordinates.
    pub geometry: RegionGeometry,
}

/// A region selected by the user.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct SelectedRegion {
    /// The selected output and the region in output-local logical coordinates.
    pub region: OutputRegion,
    /// The same region in the global compositor coordinate space, captured atomically with the
    /// selection.
    pub global_geometry: RegionGeometry,
}

/// Specification of a fixed region frame shown on an output.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct RegionFrameSpec {
    /// The output and output-local geometry of the frame.
    pub region: OutputRegion,
    /// Frame color as a CSS color string, e.g. `"red"` or `"#ff000080"`.
    pub color: String,
}

/// Command for querying or manipulating the fixed region frame.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum RegionFrameCommand {
    /// Set the region frame, replacing any existing frame.
    Set(RegionFrameSpec),
    /// Change the color of the existing region frame.
    SetColor(String),
    /// Get the current region frame.
    Get,
    /// Remove the region frame.
    Clear,
}

/// Desktop zoom state of a single output.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ZoomState {
    /// Output name.
    pub output: String,
    /// Currently displayed zoom level.
    ///
    /// While a zoom transition is in progress this is the current animation
    /// sample; during a pinch gesture it is the level set by the latest
    /// gesture update. Otherwise it is the committed zoom level.
    pub level: f64,
    /// Zoom level the output is transitioning towards.
    ///
    /// Equals [`level`](Self::level) at rest and during a pinch gesture, and
    /// differs while a zoom animation or restore transition is in progress.
    pub target_level: f64,
    /// Desktop scene zoom after Overview suppression.
    ///
    /// While the overview is open this is 1; during the overview transition
    /// it moves between [`level`](Self::level) and 1. This is the zoom of the
    /// desktop scene itself, not necessarily the transform currently visible
    /// on screen (for example under the session lock).
    pub effective_level: f64,
    /// Fixed point of the current zoom transform in output-local logical
    /// coordinates.
    pub focal: (f64, f64),
    /// Whether the zoom focal point is locked.
    ///
    /// This is the desktop zoom focal lock, not the session lock.
    pub locked: bool,
}

/// Actions that niri can perform.
// Variants in this enum should match the spelling of the ones in niri-config. Most, but not all,
// variants from niri-config should be present here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[cfg_attr(feature = "clap", command(subcommand_value_name = "ACTION"))]
#[cfg_attr(feature = "clap", command(subcommand_help_heading = "Actions"))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Action {
    /// Exit niri.
    Quit {
        /// Skip the "Press Enter to confirm" prompt.
        #[cfg_attr(feature = "clap", arg(short, long))]
        skip_confirmation: bool,
    },
    /// Power off all monitors via DPMS.
    PowerOffMonitors {},
    /// Power on all monitors via DPMS.
    PowerOnMonitors {},
    /// Spawn a command.
    Spawn {
        /// Command to spawn.
        #[cfg_attr(feature = "clap", arg(last = true, required = true))]
        command: Vec<String>,
    },
    /// Spawn a command through the shell.
    SpawnSh {
        /// Command to run.
        #[cfg_attr(feature = "clap", arg(last = true, required = true))]
        command: String,
    },
    /// Do a screen transition.
    DoScreenTransition {
        /// Delay in milliseconds for the screen to freeze before starting the transition.
        #[cfg_attr(feature = "clap", arg(short, long))]
        delay_ms: Option<u16>,
    },
    /// Open the screenshot UI.
    Screenshot {
        /// Whether to show the mouse pointer by default in the screenshot UI.
        #[cfg_attr(feature = "clap", arg(short = 'p', long, action = clap::ArgAction::Set, default_value_t = true))]
        show_pointer: bool,

        /// Path to save the screenshot to.
        ///
        /// The path must be absolute, otherwise an error is returned.
        ///
        /// If `None`, the screenshot is saved according to the `screenshot-path` config setting.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set))]
        path: Option<String>,

        /// Tool to select when opening the screenshot UI.
        #[cfg_attr(feature = "clap", arg(long, value_enum, default_value = "select"))]
        #[serde(default)]
        tool: ScreenshotTool,
    },
    /// Screenshot the focused screen.
    ScreenshotScreen {
        /// Write the screenshot to disk in addition to putting it in your clipboard.
        ///
        /// The screenshot is saved according to the `screenshot-path` config setting.
        #[cfg_attr(feature = "clap", arg(short = 'd', long, action = clap::ArgAction::Set, default_value_t = true))]
        write_to_disk: bool,

        /// Whether to include the mouse pointer in the screenshot.
        #[cfg_attr(feature = "clap", arg(short = 'p', long, action = clap::ArgAction::Set, default_value_t = true))]
        show_pointer: bool,

        /// Path to save the screenshot to.
        ///
        /// The path must be absolute, otherwise an error is returned.
        ///
        /// If `None`, the screenshot is saved according to the `screenshot-path` config setting.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set))]
        path: Option<String>,
    },
    /// Screenshot a window.
    #[cfg_attr(feature = "clap", clap(about = "Screenshot the focused window"))]
    ScreenshotWindow {
        /// Id of the window to screenshot.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
        /// Write the screenshot to disk in addition to putting it in your clipboard.
        ///
        /// The screenshot is saved according to the `screenshot-path` config setting.
        #[cfg_attr(feature = "clap", arg(short = 'd', long, action = clap::ArgAction::Set, default_value_t = true))]
        write_to_disk: bool,

        /// Whether to include the mouse pointer in the screenshot.
        ///
        /// The pointer will be included only if the window is currently receiving pointer input
        /// (usually this means the pointer is on top of the window).
        #[cfg_attr(feature = "clap", arg(short = 'p', long, action = clap::ArgAction::Set, default_value_t = false))]
        show_pointer: bool,

        /// Path to save the screenshot to.
        ///
        /// The path must be absolute, otherwise an error is returned.
        ///
        /// If `None`, the screenshot is saved according to the `screenshot-path` config setting.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set))]
        path: Option<String>,
    },
    /// Select the active tool in the screenshot editor without opening the palette.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotTool {
        /// Tool to select.
        #[cfg_attr(feature = "clap", arg(value_enum))]
        tool: ScreenshotTool,
    },
    /// Toggle the color palette of the brush tool.
    ///
    /// Has no effect if the screenshot UI is not open, or if the brush tool is not active.
    ScreenshotTogglePalette {},
    /// Open or close the color palette of the brush tool.
    ///
    /// Opening the palette while another tool is active is a no-op; the tool is not switched
    /// implicitly. Has no effect if the screenshot UI is not open.
    ScreenshotSetPalette {
        /// Whether the palette should be open.
        #[cfg_attr(feature = "clap", arg(action = clap::ArgAction::Set))]
        open: bool,
    },
    /// Pick a color from the palette by its 1-based index.
    ///
    /// Has no effect if the screenshot UI is not open or the palette is closed. An index that is
    /// within the allowed range but has no corresponding palette color is also a no-op.
    ScreenshotPaletteColor {
        /// 1-based index of the palette color, from 1 to 7.
        #[cfg_attr(feature = "clap", arg())]
        index: u8,
    },
    /// Set the brush color directly, independently of the palette and the active tool.
    ///
    /// Does not switch the tool or toggle the palette. Has no effect if the screenshot UI is
    /// not open.
    ScreenshotBrushColor {
        /// Color in `#RRGGBB` form.
        #[cfg_attr(feature = "clap", arg())]
        color: ScreenshotColor,
    },
    /// Set or adjust the keyboard-focused tool, falling back to pointer context:
    /// Draw, Arrow or Rectangle stroke width, Blur strength, or magnifier zoom
    /// over the canvas. Mouse movement clears keyboard focus. Tools are not selected.
    ///
    /// A bare number sets an absolute value from 1 to 128; a signed string such as `"+1"`
    /// or `"-1"` adjusts the current value. Each target clamps to its own range at runtime:
    /// blur strength to 1–3, magnifier zoom floored to a power of two within the configured
    /// `max-zoom`. Select and other overlay controls ignore the action. With neither
    /// keyboard focus nor a pointer, this is a no-op. Each tool remembers its own value
    /// independently. Has no effect if the screenshot UI is not open.
    ScreenshotContext {
        /// Absolute value, or a signed adjustment like `"+1"` or `"-1"`.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        change: ScreenshotValueChange,
    },
    /// Set the arrow color directly, independently of the palette.
    ///
    /// Does not switch the tool. Has no effect if the screenshot UI is not open.
    ScreenshotArrowColor {
        /// Color in `#RRGGBB` form.
        #[cfg_attr(feature = "clap", arg())]
        color: ScreenshotColor,
    },
    /// Set the rectangle outline color directly, independently of the palette.
    ///
    /// Does not switch the tool. Has no effect if the screenshot UI is not open.
    ScreenshotRectangleColor {
        /// Color in `#RRGGBB` form.
        #[cfg_attr(feature = "clap", arg())]
        color: ScreenshotColor,
    },
    /// Undo the last edit in the screenshot editor.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotUndo {},
    /// Redo the last undone edit in the screenshot editor.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotRedo {},
    /// Toggle whether the captured mouse pointer is shown in the screenshot image.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotTogglePointer {},
    /// Set whether the captured mouse pointer is shown in the screenshot image.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotSetPointer {
        /// Whether the captured pointer should be visible.
        #[cfg_attr(feature = "clap", arg(action = clap::ArgAction::Set))]
        show_pointer: bool,
    },
    /// Toggle the axis guides in the screenshot editor.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotToggleAxes {},
    /// Set whether the axis guides are shown in the screenshot editor.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotSetAxes {
        /// Whether the axis guides should be enabled.
        #[cfg_attr(feature = "clap", arg(action = clap::ArgAction::Set))]
        enabled: bool,
    },
    /// Move the selection in captured-frame pixels.
    ///
    /// The selection keeps its size and is clamped to the frame. Has no effect if the
    /// screenshot UI is not open.
    ScreenshotMoveSelection {
        /// Horizontal offset in captured-frame pixels.
        #[cfg_attr(
            feature = "clap",
            arg(long, default_value_t = 0, allow_hyphen_values = true)
        )]
        x: i32,
        /// Vertical offset in captured-frame pixels.
        #[cfg_attr(
            feature = "clap",
            arg(long, default_value_t = 0, allow_hyphen_values = true)
        )]
        y: i32,
    },
    /// Grow or shrink the selection in captured-frame pixels.
    ///
    /// The top-left corner stays in place; the size is clamped to a minimum of 1x1 and to the
    /// frame bounds. Has no effect if the screenshot UI is not open.
    ScreenshotResizeSelection {
        /// Width change in captured-frame pixels.
        #[cfg_attr(
            feature = "clap",
            arg(long, default_value_t = 0, allow_hyphen_values = true)
        )]
        width: i32,
        /// Height change in captured-frame pixels.
        #[cfg_attr(
            feature = "clap",
            arg(long, default_value_t = 0, allow_hyphen_values = true)
        )]
        height: i32,
    },
    /// Set or adjust the magnifier zoom in the screenshot editor.
    ///
    /// A bare number sets an absolute zoom level; allowed absolute values are 1, 2, 4, 8, 16
    /// and 32. A signed string such as `"+1"` or `"-1"` moves that many steps along the zoom
    /// ladder, clamped by the configured `max-zoom`. Has no effect if the screenshot UI is not
    /// open.
    ScreenshotMagnifierZoom {
        /// Absolute zoom level, or a signed step adjustment like `"+1"` or `"-1"`.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        zoom: ScreenshotValueChange,
    },
    /// Move keyboard focus to the next available screenshot UI control.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotFocusNext {},
    /// Move keyboard focus to the previous available screenshot UI control.
    ///
    /// Has no effect if the screenshot UI is not open.
    ScreenshotFocusPrevious {},
    /// Activate the focused screenshot UI control, or save and copy when nothing is focused.
    ///
    /// Equivalent to pressing Enter in the screenshot UI. Has no effect if the screenshot UI is
    /// not open.
    ScreenshotConfirm {},
    /// One step of the screenshot UI's Escape handling.
    ///
    /// Closes the palette, then cancels the in-progress gesture restoring the selection, then
    /// closes the UI. Has no effect if the screenshot UI is not open.
    ScreenshotCancel {},
    /// Export the screenshot editor's selection, copy it to the clipboard, and close the UI.
    ///
    /// Has no effect if the screenshot UI is not open.
    ConfirmScreenshot {
        /// Write the screenshot to disk in addition to putting it in your clipboard.
        ///
        /// The screenshot is saved to the path from the session that opened the editor.
        #[cfg_attr(feature = "clap", arg(short = 'd', long, action = clap::ArgAction::Set, default_value_t = true))]
        write_to_disk: bool,

        /// Image format for the screenshot written to disk.
        ///
        /// The clipboard image is always PNG.
        ///
        /// If `None`, the screenshot is saved according to the `screenshot-format` config
        /// setting.
        #[cfg_attr(feature = "clap", arg(long, value_enum))]
        #[serde(default)]
        format: Option<ScreenshotFormat>,
        /// Quality for this export: WebP 0–100 (lossy), AVIF 1–100.
        /// Omitted: WebP lossless, AVIF 90. Ignored for PNG and clipboard-only.
        #[cfg_attr(feature = "clap", arg(long, allow_hyphen_values = true))]
        #[serde(default)]
        quality: Option<f32>,
    },
    /// Close the screenshot editor unconditionally, discarding any edits.
    ///
    /// Has no effect if the screenshot UI is not open.
    CancelScreenshot {},
    /// Enable or disable the keyboard shortcuts inhibitor (if any) for the focused surface.
    ToggleKeyboardShortcutsInhibit {},
    /// Close a window.
    #[cfg_attr(feature = "clap", clap(about = "Close the focused window"))]
    CloseWindow {
        /// Id of the window to close.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Toggle fullscreen on a window.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Toggle fullscreen on the focused window")
    )]
    FullscreenWindow {
        /// Id of the window to toggle fullscreen of.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Toggle windowed (fake) fullscreen on a window.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Toggle windowed (fake) fullscreen on the focused window")
    )]
    ToggleWindowedFullscreen {
        /// Id of the window to toggle windowed fullscreen of.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Focus a window by id.
    FocusWindow {
        /// Id of the window to focus.
        #[cfg_attr(feature = "clap", arg(long))]
        id: u64,
    },
    /// Focus a window in the focused column by index.
    FocusWindowInColumn {
        /// Index of the window in the column.
        ///
        /// The index starts from 1 for the topmost window.
        #[cfg_attr(feature = "clap", arg())]
        index: u8,
    },
    /// Focus the previously focused window.
    FocusWindowPrevious {},
    /// Focus the column to the left.
    FocusColumnLeft {},
    /// Focus the column to the right.
    FocusColumnRight {},
    /// Focus the first column.
    FocusColumnFirst {},
    /// Focus the last column.
    FocusColumnLast {},
    /// Focus the next column to the right, looping if at end.
    FocusColumnRightOrFirst {},
    /// Focus the next column to the left, looping if at start.
    FocusColumnLeftOrLast {},
    /// Focus a column by index.
    FocusColumn {
        /// Index of the column to focus.
        ///
        /// The index starts from 1 for the first column.
        #[cfg_attr(feature = "clap", arg())]
        index: usize,
    },
    /// Focus the window or the monitor above.
    FocusWindowOrMonitorUp {},
    /// Focus the window or the monitor below.
    FocusWindowOrMonitorDown {},
    /// Focus the column or the monitor to the left.
    FocusColumnOrMonitorLeft {},
    /// Focus the column or the monitor to the right.
    FocusColumnOrMonitorRight {},
    /// Focus the window below.
    FocusWindowDown {},
    /// Focus the window above.
    FocusWindowUp {},
    /// Focus the window below or the column to the left.
    FocusWindowDownOrColumnLeft {},
    /// Focus the window below or the column to the right.
    FocusWindowDownOrColumnRight {},
    /// Focus the window above or the column to the left.
    FocusWindowUpOrColumnLeft {},
    /// Focus the window above or the column to the right.
    FocusWindowUpOrColumnRight {},
    /// Focus the window or the workspace below.
    FocusWindowOrWorkspaceDown {},
    /// Focus the window or the workspace above.
    FocusWindowOrWorkspaceUp {},
    /// Focus the topmost window.
    FocusWindowTop {},
    /// Focus the bottommost window.
    FocusWindowBottom {},
    /// Focus the window below or the topmost window.
    FocusWindowDownOrTop {},
    /// Focus the window above or the bottommost window.
    FocusWindowUpOrBottom {},
    /// Move the focused column to the left.
    MoveColumnLeft {},
    /// Move the focused column to the right.
    MoveColumnRight {},
    /// Move the focused column to the start of the workspace.
    MoveColumnToFirst {},
    /// Move the focused column to the end of the workspace.
    MoveColumnToLast {},
    /// Move the focused column to the left or to the monitor to the left.
    MoveColumnLeftOrToMonitorLeft {},
    /// Move the focused column to the right or to the monitor to the right.
    MoveColumnRightOrToMonitorRight {},
    /// Move the focused column to a specific index on its workspace.
    MoveColumnToIndex {
        /// New index for the column.
        ///
        /// The index starts from 1 for the first column.
        #[cfg_attr(feature = "clap", arg())]
        index: usize,
    },
    /// Move the focused window down in a column.
    MoveWindowDown {},
    /// Move the focused window up in a column.
    MoveWindowUp {},
    /// Move the focused window down in a column or to the workspace below.
    MoveWindowDownOrToWorkspaceDown {},
    /// Move the focused window up in a column or to the workspace above.
    MoveWindowUpOrToWorkspaceUp {},
    /// Consume or expel a window left.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Consume or expel the focused window left")
    )]
    ConsumeOrExpelWindowLeft {
        /// Id of the window to consume or expel.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Consume or expel a window right.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Consume or expel the focused window right")
    )]
    ConsumeOrExpelWindowRight {
        /// Id of the window to consume or expel.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Consume the window to the right into the focused column.
    ConsumeWindowIntoColumn {},
    /// Expel the bottom window from the focused column.
    ExpelWindowFromColumn {},
    /// Swap focused window with one to the right.
    SwapWindowRight {},
    /// Swap focused window with one to the left.
    SwapWindowLeft {},
    /// Toggle the focused column between normal and tabbed display.
    ToggleColumnTabbedDisplay {},
    /// Set the display mode of the focused column.
    SetColumnDisplay {
        /// Display mode to set.
        #[cfg_attr(feature = "clap", arg())]
        display: ColumnDisplay,
    },
    /// Center the focused column on the screen.
    CenterColumn {},
    /// Center a window on the screen.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Center the focused window on the screen")
    )]
    CenterWindow {
        /// Id of the window to center.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Center all fully visible columns on the screen.
    CenterVisibleColumns {},
    /// Focus the workspace below.
    FocusWorkspaceDown {},
    /// Focus the workspace above.
    FocusWorkspaceUp {},
    /// Focus a workspace by reference (index or name).
    FocusWorkspace {
        /// Reference (index or name) of the workspace to focus.
        #[cfg_attr(feature = "clap", arg())]
        reference: WorkspaceReferenceArg,
    },
    /// Focus the previous workspace.
    FocusWorkspacePrevious {},
    /// Move the focused window to the workspace below.
    MoveWindowToWorkspaceDown {
        /// Whether the focus should follow the target workspace.
        ///
        /// If `true` (the default), the focus will follow the window to the new workspace. If
        /// `false`, the focus will remain on the original workspace.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set, default_value_t = true))]
        focus: bool,
    },
    /// Move the focused window to the workspace above.
    MoveWindowToWorkspaceUp {
        /// Whether the focus should follow the target workspace.
        ///
        /// If `true` (the default), the focus will follow the window to the new workspace. If
        /// `false`, the focus will remain on the original workspace.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set, default_value_t = true))]
        focus: bool,
    },
    /// Move a window to a workspace.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Move the focused window to a workspace by reference (index or name)")
    )]
    MoveWindowToWorkspace {
        /// Id of the window to move.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        window_id: Option<u64>,

        /// Reference (index or name) of the workspace to move the window to.
        #[cfg_attr(feature = "clap", arg())]
        reference: WorkspaceReferenceArg,

        /// Whether the focus should follow the moved window.
        ///
        /// If `true` (the default) and the window to move is focused, the focus will follow the
        /// window to the new workspace. If `false`, the focus will remain on the original
        /// workspace.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set, default_value_t = true))]
        focus: bool,
    },
    /// Move the focused column to the workspace below.
    MoveColumnToWorkspaceDown {
        /// Whether the focus should follow the target workspace.
        ///
        /// If `true` (the default), the focus will follow the column to the new workspace. If
        /// `false`, the focus will remain on the original workspace.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set, default_value_t = true))]
        focus: bool,
    },
    /// Move the focused column to the workspace above.
    MoveColumnToWorkspaceUp {
        /// Whether the focus should follow the target workspace.
        ///
        /// If `true` (the default), the focus will follow the column to the new workspace. If
        /// `false`, the focus will remain on the original workspace.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set, default_value_t = true))]
        focus: bool,
    },
    /// Move the focused column to a workspace by reference (index or name).
    MoveColumnToWorkspace {
        /// Reference (index or name) of the workspace to move the column to.
        #[cfg_attr(feature = "clap", arg())]
        reference: WorkspaceReferenceArg,

        /// Whether the focus should follow the target workspace.
        ///
        /// If `true` (the default), the focus will follow the column to the new workspace. If
        /// `false`, the focus will remain on the original workspace.
        #[cfg_attr(feature = "clap", arg(long, action = clap::ArgAction::Set, default_value_t = true))]
        focus: bool,
    },
    /// Move the focused workspace down.
    MoveWorkspaceDown {},
    /// Move the focused workspace up.
    MoveWorkspaceUp {},
    /// Move a workspace to a specific index on its monitor.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Move the focused workspace to a specific index on its monitor")
    )]
    MoveWorkspaceToIndex {
        /// New index for the workspace.
        #[cfg_attr(feature = "clap", arg())]
        index: usize,

        /// Reference (index or name) of the workspace to move.
        ///
        /// If `None`, uses the focused workspace.
        #[cfg_attr(feature = "clap", arg(long))]
        reference: Option<WorkspaceReferenceArg>,
    },
    /// Set the name of a workspace.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Set the name of the focused workspace")
    )]
    SetWorkspaceName {
        /// New name for the workspace.
        #[cfg_attr(feature = "clap", arg())]
        name: String,

        /// Reference (index or name) of the workspace to name.
        ///
        /// If `None`, uses the focused workspace.
        #[cfg_attr(feature = "clap", arg(long))]
        workspace: Option<WorkspaceReferenceArg>,
    },
    /// Unset the name of a workspace.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Unset the name of the focused workspace")
    )]
    UnsetWorkspaceName {
        /// Reference (index or name) of the workspace to unname.
        ///
        /// If `None`, uses the focused workspace.
        #[cfg_attr(feature = "clap", arg())]
        reference: Option<WorkspaceReferenceArg>,
    },
    /// Focus the monitor to the left.
    FocusMonitorLeft {},
    /// Focus the monitor to the right.
    FocusMonitorRight {},
    /// Focus the monitor below.
    FocusMonitorDown {},
    /// Focus the monitor above.
    FocusMonitorUp {},
    /// Focus the previous monitor.
    FocusMonitorPrevious {},
    /// Focus the next monitor.
    FocusMonitorNext {},
    /// Focus a monitor by name.
    FocusMonitor {
        /// Name of the output to focus.
        #[cfg_attr(feature = "clap", arg())]
        output: String,
    },
    /// Move the focused window to the monitor to the left.
    MoveWindowToMonitorLeft {},
    /// Move the focused window to the monitor to the right.
    MoveWindowToMonitorRight {},
    /// Move the focused window to the monitor below.
    MoveWindowToMonitorDown {},
    /// Move the focused window to the monitor above.
    MoveWindowToMonitorUp {},
    /// Move the focused window to the previous monitor.
    MoveWindowToMonitorPrevious {},
    /// Move the focused window to the next monitor.
    MoveWindowToMonitorNext {},
    /// Move a window to a specific monitor.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Move the focused window to a specific monitor")
    )]
    MoveWindowToMonitor {
        /// Id of the window to move.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,

        /// The target output name.
        #[cfg_attr(feature = "clap", arg())]
        output: String,
    },
    /// Move the focused column to the monitor to the left.
    MoveColumnToMonitorLeft {},
    /// Move the focused column to the monitor to the right.
    MoveColumnToMonitorRight {},
    /// Move the focused column to the monitor below.
    MoveColumnToMonitorDown {},
    /// Move the focused column to the monitor above.
    MoveColumnToMonitorUp {},
    /// Move the focused column to the previous monitor.
    MoveColumnToMonitorPrevious {},
    /// Move the focused column to the next monitor.
    MoveColumnToMonitorNext {},
    /// Move the focused column to a specific monitor.
    MoveColumnToMonitor {
        /// The target output name.
        #[cfg_attr(feature = "clap", arg())]
        output: String,
    },
    /// Change the width of a window.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Change the width of the focused window")
    )]
    SetWindowWidth {
        /// Id of the window whose width to set.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,

        /// How to change the width.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        change: SizeChange,
    },
    /// Change the height of a window.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Change the height of the focused window")
    )]
    SetWindowHeight {
        /// Id of the window whose height to set.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,

        /// How to change the height.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        change: SizeChange,
    },
    /// Reset the height of a window back to automatic.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Reset the height of the focused window back to automatic")
    )]
    ResetWindowHeight {
        /// Id of the window whose height to reset.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Switch between preset column widths.
    SwitchPresetColumnWidth {},
    /// Switch between preset column widths backwards.
    SwitchPresetColumnWidthBack {},
    /// Switch between preset window widths.
    SwitchPresetWindowWidth {
        /// Id of the window whose width to switch.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Switch between preset window widths backwards.
    SwitchPresetWindowWidthBack {
        /// Id of the window whose width to switch.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Switch between preset window heights.
    SwitchPresetWindowHeight {
        /// Id of the window whose height to switch.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Switch between preset window heights backwards.
    SwitchPresetWindowHeightBack {
        /// Id of the window whose height to switch.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Toggle the maximized state of the focused column.
    MaximizeColumn {},
    /// Toggle the maximized-to-edges state of the focused window.
    MaximizeWindowToEdges {
        /// Id of the window to maximize.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Change the width of the focused column.
    SetColumnWidth {
        /// How to change the width.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        change: SizeChange,
    },
    /// Expand the focused column to space not taken up by other fully visible columns.
    ExpandColumnToAvailableWidth {},
    /// Switch between keyboard layouts.
    SwitchLayout {
        /// Layout to switch to.
        #[cfg_attr(feature = "clap", arg())]
        layout: LayoutSwitchTarget,
    },
    /// Show the hotkey overlay.
    ShowHotkeyOverlay {},
    /// Move the focused workspace to the monitor to the left.
    MoveWorkspaceToMonitorLeft {},
    /// Move the focused workspace to the monitor to the right.
    MoveWorkspaceToMonitorRight {},
    /// Move the focused workspace to the monitor below.
    MoveWorkspaceToMonitorDown {},
    /// Move the focused workspace to the monitor above.
    MoveWorkspaceToMonitorUp {},
    /// Move the focused workspace to the previous monitor.
    MoveWorkspaceToMonitorPrevious {},
    /// Move the focused workspace to the next monitor.
    MoveWorkspaceToMonitorNext {},
    /// Move a workspace to a specific monitor.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Move the focused workspace to a specific monitor")
    )]
    MoveWorkspaceToMonitor {
        /// The target output name.
        #[cfg_attr(feature = "clap", arg())]
        output: String,

        // Reference (index or name) of the workspace to move.
        ///
        /// If `None`, uses the focused workspace.
        #[cfg_attr(feature = "clap", arg(long))]
        reference: Option<WorkspaceReferenceArg>,
    },
    /// Toggle a debug tint on windows.
    ToggleDebugTint {},
    /// Toggle visualization of render element opaque regions.
    DebugToggleOpaqueRegions {},
    /// Toggle visualization of output damage.
    DebugToggleDamage {},
    /// Move the focused window between the floating and the tiling layout.
    ToggleWindowFloating {
        /// Id of the window to move.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Move the focused window to the floating layout.
    MoveWindowToFloating {
        /// Id of the window to move.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Move the focused window to the tiling layout.
    MoveWindowToTiling {
        /// Id of the window to move.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Switches focus to the floating layout.
    FocusFloating {},
    /// Switches focus to the tiling layout.
    FocusTiling {},
    /// Toggles the focus between the floating and the tiling layout.
    SwitchFocusBetweenFloatingAndTiling {},
    /// Move a floating window on screen.
    #[cfg_attr(feature = "clap", clap(about = "Move the floating window on screen"))]
    MoveFloatingWindow {
        /// Id of the window to move.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,

        /// How to change the X position.
        #[cfg_attr(
            feature = "clap",
            arg(short, long, default_value = "+0", allow_hyphen_values = true)
        )]
        x: PositionChange,

        /// How to change the Y position.
        #[cfg_attr(
            feature = "clap",
            arg(short, long, default_value = "+0", allow_hyphen_values = true)
        )]
        y: PositionChange,
    },
    /// Toggle the opacity of a window.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Toggle the opacity of the focused window")
    )]
    ToggleWindowRuleOpacity {
        /// Id of the window.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Set the dynamic cast target to a window.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Set the dynamic cast target to the focused window")
    )]
    SetDynamicCastWindow {
        /// Id of the window to target.
        ///
        /// If `None`, uses the focused window.
        #[cfg_attr(feature = "clap", arg(long))]
        id: Option<u64>,
    },
    /// Set the dynamic cast target to a monitor.
    #[cfg_attr(
        feature = "clap",
        clap(about = "Set the dynamic cast target to the focused monitor")
    )]
    SetDynamicCastMonitor {
        /// Name of the output to target.
        ///
        /// If `None`, uses the focused output.
        #[cfg_attr(feature = "clap", arg())]
        output: Option<String>,
    },
    /// Clear the dynamic cast target, making it show nothing.
    ClearDynamicCastTarget {},
    /// Stop a PipeWire screencast.
    ///
    /// wlr-screencopy screencasts cannot currently be stopped via IPC.
    StopCast {
        /// Session ID of the screencast to stop.
        ///
        /// If the session has multiple screencast streams, this will stop all of them.
        #[cfg_attr(feature = "clap", arg(long))]
        session_id: u64,
    },
    /// Toggle (open/close) the Overview.
    ToggleOverview {},
    /// Open the Overview.
    OpenOverview {},
    /// Close the Overview.
    CloseOverview {},
    /// Toggle urgent status of a window.
    ToggleWindowUrgent {
        /// Id of the window to toggle urgent.
        #[cfg_attr(feature = "clap", arg(long))]
        id: u64,
    },
    /// Set urgent status of a window.
    SetWindowUrgent {
        /// Id of the window to set urgent.
        #[cfg_attr(feature = "clap", arg(long))]
        id: u64,
    },
    /// Unset urgent status of a window.
    UnsetWindowUrgent {
        /// Id of the window to unset urgent.
        #[cfg_attr(feature = "clap", arg(long))]
        id: u64,
    },
    /// Reload the config file.
    ///
    /// Can be useful for scripts changing the config file, to avoid waiting the small duration for
    /// niri's config file watcher to notice the changes.
    LoadConfigFile {
        /// Path of a new config file to load.
        ///
        /// If unset, reloads the current config file.
        #[cfg_attr(feature = "clap", arg(long))]
        path: Option<String>,
    },
    /// Zoom in on an output.
    ///
    /// Zooms the output under the pointer, or the focused output if the pointer is not on any
    /// output.
    ZoomIn {},
    /// Zoom out on an output.
    ///
    /// Zooms the output under the pointer, or the focused output if the pointer is not on any
    /// output.
    ZoomOut {},
    /// Set the zoom level of an output.
    ///
    /// Zooms the output under the pointer, or the focused output if the pointer is not on any
    /// output.
    SetZoomLevel {
        /// Zoom level to set, with 1 meaning no zoom.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        level: f64,
    },
    /// Reset the zoom level of an output back to 1.
    ///
    /// Zooms the output under the pointer, or the focused output if the pointer is not on any
    /// output.
    ResetZoom {},
    /// Toggle the zoom lock of an output.
    ///
    /// While locked, the zoomed viewport does not follow the pointer. Applies to the output under
    /// the pointer, or the focused output if the pointer is not on any output.
    ZoomLock {},
    /// Toggle the zoom level of an output between 1 and a preset level.
    ///
    /// Zooms the output under the pointer, or the focused output if the pointer is not on any
    /// output. If the target zoom level is above 1, resets it to 1; otherwise sets it to `level`.
    Zoom {
        /// Zoom level to toggle to, must be greater than 1.
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        level: f64,
        /// Lock the viewport while zoomed in.
        ///
        /// Zooming in is anchored at the pointer and immediately locks the viewport so it
        /// does not follow pointer movement; toggling back to 1 unlocks it.
        #[cfg_attr(feature = "clap", arg(long))]
        #[serde(default)]
        lock: bool,
    },
}
impl Action {
    /// Validates action arguments that do not depend on compositor state.
    ///
    /// The IPC server runs this check before dispatching the action to the event loop, so raw
    /// JSON requests and CLI arguments are validated identically. Config-file actions go through
    /// the same argument types and are checked when the config is parsed.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ConfirmScreenshot {
                quality: Some(quality),
                format,
                write_to_disk,
            } => {
                if !(0.0..=100.0).contains(quality) {
                    return Err("quality must be between 0 and 100".into());
                }
                if *write_to_disk && *format == Some(ScreenshotFormat::Avif) && *quality < 1.0 {
                    return Err("AVIF quality must be between 1 and 100".into());
                }
            }
            Self::ScreenshotContext {
                change: ScreenshotValueChange::Set(change),
            } => {
                // The pointer context is not known here, so absolute values accept the full
                // stroke-width range; each target clamps to its own range at runtime.
                if !(MIN_SCREENSHOT_BRUSH_WIDTH..=MAX_SCREENSHOT_BRUSH_WIDTH).contains(change) {
                    return Err(format!(
                        "screenshot context value must be between {MIN_SCREENSHOT_BRUSH_WIDTH} \
                         and {MAX_SCREENSHOT_BRUSH_WIDTH}, got {change}"
                    ));
                }
            }
            Self::ScreenshotMagnifierZoom {
                zoom: ScreenshotValueChange::Set(zoom),
            } => {
                if !matches!(*zoom, 1 | 2 | 4 | 8 | 16 | 32) {
                    return Err(format!(
                        "magnifier zoom must be one of 1, 2, 4, 8, 16, 32, got {zoom}"
                    ));
                }
            }
            Self::ScreenshotPaletteColor { index } if !(1..=7).contains(index) => {
                return Err(format!(
                    "palette color index must be between 1 and 7, got {index}"
                ));
            }
            _ => (),
        }
        Ok(())
    }
}

/// The editing tool in the screenshot editor.
///
/// `select`/`draw`/`blur`/`arrow`/`rectangle`) and the config file.
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ScreenshotTool {
    /// Region selection tool.
    #[default]
    Select,
    /// Freehand brush tool.
    Draw,
    /// Blur tool.
    Blur,
    /// Straight arrow tool.
    Arrow,
    /// Rectangle outline tool.
    Rectangle,
}

impl std::fmt::Display for ScreenshotTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Select => "select",
            Self::Draw => "draw",
            Self::Blur => "blur",
            Self::Arrow => "arrow",
            Self::Rectangle => "rectangle",
        };
        f.write_str(s)
    }
}

impl FromStr for ScreenshotTool {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "select" => Ok(Self::Select),
            "draw" => Ok(Self::Draw),
            "blur" => Ok(Self::Blur),
            "arrow" => Ok(Self::Arrow),
            "rectangle" => Ok(Self::Rectangle),
            _ => Err(
                "invalid tool: can be \"select\", \"draw\", \"blur\", \"arrow\" or \"rectangle\"",
            ),
        }
    }
}

/// Image format for screenshots written to disk.
///
/// Shared between the `screenshot-format` config setting, the `confirm-screenshot` action and
/// CLI parsing (lowercase `png`/`webp`/`avif`). The clipboard image is always PNG.
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ScreenshotFormat {
    /// PNG image format.
    #[default]
    Png,
    /// WebP image format.
    Webp,
    /// AVIF image format.
    Avif,
}

impl ScreenshotFormat {
    /// File extension for this format, without a leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
        }
    }
}

impl std::fmt::Display for ScreenshotFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

impl FromStr for ScreenshotFormat {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "png" => Ok(Self::Png),
            "webp" => Ok(Self::Webp),
            "avif" => Ok(Self::Avif),
            _ => Err("invalid format: can be \"png\", \"webp\" or \"avif\""),
        }
    }
}

/// Minimum stroke width in the screenshot editor, in captured-frame pixels.
///
/// Also the lower bound of the absolute `ScreenshotContext` value accepted at validation
/// time; each pointer-context target clamps to its own range at runtime.
pub const MIN_SCREENSHOT_BRUSH_WIDTH: u32 = 1;

/// Maximum stroke width in the screenshot editor, in captured-frame pixels.
///
/// Also the upper bound of the absolute `ScreenshotContext` value accepted at validation
/// time; each pointer-context target clamps to its own range at runtime.
pub const MAX_SCREENSHOT_BRUSH_WIDTH: u32 = 128;

/// An absolute value or a signed adjustment for a screenshot editor setting.
///
/// On the wire and in the config, an absolute value is a plain number (`6`) and an adjustment
/// is a signed string (`"+1"`, `"-1"`). The CLI accepts a bare positive number as an absolute
/// value and a signed number as an adjustment through [`FromStr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotValueChange {
    /// Set an absolute value.
    Set(u32),
    /// Adjust the current value by a signed amount.
    Adjust(i32),
}

impl Serialize for ScreenshotValueChange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Set(value) => serializer.serialize_u32(*value),
            Self::Adjust(delta) => serializer.serialize_str(&format!("{delta:+}")),
        }
    }
}

impl<'de> Deserialize<'de> for ScreenshotValueChange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ScreenshotValueChange;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a non-negative integer, or a signed integer string")
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                u32::try_from(value)
                    .map(ScreenshotValueChange::Set)
                    .map_err(|_| E::custom(format!("value out of range: {value}")))
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                u32::try_from(value)
                    .map(ScreenshotValueChange::Set)
                    .map_err(|_| {
                        E::custom(format!("absolute value must be non-negative, got {value}"))
                    })
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(feature = "json-schema")]
impl schemars::JsonSchema for ScreenshotValueChange {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ScreenshotValueChange".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "anyOf": [
                {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": u32::MAX,
                    "description": "Absolute value to set.",
                },
                {
                    "type": "string",
                    "pattern": "^[+-][0-9]+$",
                    "description": "Signed adjustment, like \"+1\" or \"-1\".",
                },
            ],
        })
    }
}

impl std::fmt::Display for ScreenshotValueChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Set(value) => write!(f, "{value}"),
            Self::Adjust(delta) => write!(f, "{delta:+}"),
        }
    }
}

impl FromStr for ScreenshotValueChange {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.as_bytes().first() {
            Some(b'+' | b'-') => {
                let digits = &s[1..];
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("invalid value: expected a signed integer adjustment");
                }
                // The digits carry no sign, so a magnitude equal to 2^31 (the i32::MIN
                // magnitude) fits in i64; the final check rejects anything larger.
                let magnitude = digits
                    .parse::<i64>()
                    .map_err(|_| "invalid value: adjustment out of range")?;
                let delta = if s.as_bytes()[0] == b'-' {
                    -magnitude
                } else {
                    magnitude
                };
                i32::try_from(delta)
                    .map(Self::Adjust)
                    .map_err(|_| "invalid value: adjustment out of range")
            }
            _ => s
                .parse::<u32>()
                .map(Self::Set)
                .map_err(|_| "invalid value: expected a non-negative integer"),
        }
    }
}

/// An opaque RGB color in the screenshot editor.
///
/// Parsed and serialized as a `#RRGGBB` string; there is no alpha channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenshotColor {
    /// Red component.
    pub r: u8,
    /// Green component.
    pub g: u8,
    /// Blue component.
    pub b: u8,
}

impl ScreenshotColor {
    /// Creates a color from its RGB components.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The color's components, normalized to `[0.0, 1.0]` for Cairo.
    pub fn rgb(self) -> (f64, f64, f64) {
        (
            f64::from(self.r) / 255.,
            f64::from(self.g) / 255.,
            f64::from(self.b) / 255.,
        )
    }
}

impl Default for ScreenshotColor {
    /// The default brush color of the screenshot editor: red.
    fn default() -> Self {
        Self::new(0xff, 0, 0)
    }
}

impl FromStr for ScreenshotColor {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const ERR: &str = "invalid color: expected an #RRGGBB string";

        let hex = s.strip_prefix('#').ok_or(ERR)?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ERR);
        }
        // Length and hex checks above make this parse infallible.
        let value = u32::from_str_radix(hex, 16).map_err(|_| ERR)?;
        Ok(Self {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        })
    }
}

impl Serialize for ScreenshotColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b))
    }
}

impl<'de> Deserialize<'de> for ScreenshotColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ScreenshotColor;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an #RRGGBB color string")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}

#[cfg(feature = "json-schema")]
impl schemars::JsonSchema for ScreenshotColor {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ScreenshotColor".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^#[0-9a-fA-F]{6}$",
            "description": "An opaque RGB color in #RRGGBB form.",
        })
    }
}

/// Change in window or column size.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum SizeChange {
    /// Set the size in logical pixels.
    SetFixed(i32),
    /// Set the size as a proportion of the working area.
    SetProportion(f64),
    /// Add or subtract to the current size in logical pixels.
    AdjustFixed(i32),
    /// Add or subtract to the current size as a proportion of the working area.
    AdjustProportion(f64),
}

/// Change in floating window position.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum PositionChange {
    /// Set the position in logical pixels.
    SetFixed(f64),
    /// Set the position as a proportion of the working area.
    SetProportion(f64),
    /// Add or subtract to the current position in logical pixels.
    AdjustFixed(f64),
    /// Add or subtract to the current position as a proportion of the working area.
    AdjustProportion(f64),
}

/// Workspace reference (id, index or name) to operate on.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum WorkspaceReferenceArg {
    /// Id of the workspace.
    Id(u64),
    /// Index of the workspace.
    Index(u8),
    /// Name of the workspace.
    Name(String),
}

/// Layout to switch to.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum LayoutSwitchTarget {
    /// The next configured layout.
    Next,
    /// The previous configured layout.
    Prev,
    /// The specific layout by index.
    Index(u8),
}

/// How windows display in a column.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ColumnDisplay {
    /// Windows are tiled vertically across the working area height.
    Normal,
    /// Windows are in tabs.
    Tabbed,
}

/// Output actions that niri can perform.
// Variants in this enum should match the spelling of the ones in niri-config. Most thigs from
// niri-config should be present here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[cfg_attr(feature = "clap", command(subcommand_value_name = "ACTION"))]
#[cfg_attr(feature = "clap", command(subcommand_help_heading = "Actions"))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum OutputAction {
    /// Turn off the output.
    Off,
    /// Turn on the output.
    On,
    /// Set the output mode.
    Mode {
        /// Mode to set, or "auto" for automatic selection.
        ///
        /// Run `niri msg outputs` to see the available modes.
        #[cfg_attr(feature = "clap", arg())]
        mode: ModeToSet,
    },
    /// Set a custom output mode.
    CustomMode {
        /// Custom mode to set.
        #[cfg_attr(feature = "clap", arg())]
        mode: ConfiguredMode,
    },
    /// Set a custom VESA CVT modeline.
    #[cfg_attr(feature = "clap", arg())]
    Modeline {
        /// The rate at which pixels are drawn in MHz.
        #[cfg_attr(feature = "clap", arg())]
        clock: f64,
        /// Horizontal active pixels.
        #[cfg_attr(feature = "clap", arg())]
        hdisplay: u16,
        /// Horizontal sync pulse start position in pixels.
        #[cfg_attr(feature = "clap", arg())]
        hsync_start: u16,
        /// Horizontal sync pulse end position in pixels.
        #[cfg_attr(feature = "clap", arg())]
        hsync_end: u16,
        /// Total horizontal number of pixels before resetting the horizontal drawing position to
        /// zero.
        #[cfg_attr(feature = "clap", arg())]
        htotal: u16,

        /// Vertical active pixels.
        #[cfg_attr(feature = "clap", arg())]
        vdisplay: u16,
        /// Vertical sync pulse start position in pixels.
        #[cfg_attr(feature = "clap", arg())]
        vsync_start: u16,
        /// Vertical sync pulse end position in pixels.
        #[cfg_attr(feature = "clap", arg())]
        vsync_end: u16,
        /// Total vertical number of pixels before resetting the vertical drawing position to zero.
        #[cfg_attr(feature = "clap", arg())]
        vtotal: u16,
        /// Horizontal sync polarity: "+hsync" or "-hsync".
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        hsync_polarity: HSyncPolarity,
        /// Vertical sync polarity: "+vsync" or "-vsync".
        #[cfg_attr(feature = "clap", arg(allow_hyphen_values = true))]
        vsync_polarity: VSyncPolarity,
    },
    /// Set the output scale.
    Scale {
        /// Scale factor to set, or "auto" for automatic selection.
        #[cfg_attr(feature = "clap", arg())]
        scale: ScaleToSet,
    },
    /// Set the output transform.
    Transform {
        /// Transform to set, counter-clockwise.
        #[cfg_attr(feature = "clap", arg())]
        transform: Transform,
    },
    /// Set the output position.
    Position {
        /// Position to set, or "auto" for automatic selection.
        #[cfg_attr(feature = "clap", command(subcommand))]
        position: PositionToSet,
    },
    /// Set the variable refresh rate mode.
    Vrr {
        /// Variable refresh rate mode to set.
        #[cfg_attr(feature = "clap", command(flatten))]
        vrr: VrrToSet,
    },
    /// Set the maximum bits per channel (bit depth).
    MaxBpc {
        /// Maximum bits per channel to set.
        #[cfg_attr(feature = "clap", arg())]
        max_bpc: MaxBpc,
    },
}

/// Output mode to set.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ModeToSet {
    /// Niri will pick the mode automatically.
    Automatic,
    /// Specific mode.
    Specific(ConfiguredMode),
}

/// Output mode as set in the config file.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ConfiguredMode {
    /// Width in physical pixels.
    pub width: u16,
    /// Height in physical pixels.
    pub height: u16,
    /// Refresh rate.
    pub refresh: Option<f64>,
}

/// Modeline horizontal syncing polarity.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum HSyncPolarity {
    /// Positive polarity.
    PHSync,
    /// Negative polarity.
    NHSync,
}

/// Modeline vertical syncing polarity.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum VSyncPolarity {
    /// Positive polarity.
    PVSync,
    /// Negative polarity.
    NVSync,
}

/// Output scale to set.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ScaleToSet {
    /// Niri will pick the scale automatically.
    Automatic,
    /// Specific scale.
    Specific(f64),
}

/// Output position to set.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
#[cfg_attr(feature = "clap", command(subcommand_value_name = "POSITION"))]
#[cfg_attr(feature = "clap", command(subcommand_help_heading = "Position Values"))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum PositionToSet {
    /// Position the output automatically.
    #[cfg_attr(feature = "clap", command(name = "auto"))]
    Automatic,
    /// Set a specific position.
    #[cfg_attr(feature = "clap", command(name = "set"))]
    Specific(ConfiguredPosition),
}

/// Output position as set in the config file.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "clap", derive(clap::Args))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ConfiguredPosition {
    /// Logical X position.
    pub x: i32,
    /// Logical Y position.
    pub y: i32,
}

/// Output VRR to set.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "clap", derive(clap::Args))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct VrrToSet {
    /// Whether to enable variable refresh rate.
    #[cfg_attr(
        feature = "clap",
        arg(
            value_name = "ON|OFF",
            action = clap::ArgAction::Set,
            value_parser = clap::builder::BoolishValueParser::new(),
            hide_possible_values = true,
        ),
    )]
    pub vrr: bool,
    /// Only enable when the output shows a window matching the variable-refresh-rate window rule.
    #[cfg_attr(feature = "clap", arg(long))]
    pub on_demand: bool,
}

/// Connected output.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Output {
    /// Name of the output.
    pub name: String,
    /// Textual description of the manufacturer.
    pub make: String,
    /// Textual description of the model.
    pub model: String,
    /// Serial of the output, if known.
    pub serial: Option<String>,
    /// Physical width and height of the output in millimeters, if known.
    pub physical_size: Option<(u32, u32)>,
    /// Available modes for the output.
    pub modes: Vec<Mode>,
    /// Index of the current mode in [`Self::modes`].
    ///
    /// `None` if the output is disabled.
    pub current_mode: Option<usize>,
    /// Whether the current_mode is a custom mode.
    pub is_custom_mode: bool,
    /// Whether the output supports variable refresh rate.
    pub vrr_supported: bool,
    /// Whether variable refresh rate is enabled on the output.
    pub vrr_enabled: bool,
    /// Logical output information.
    ///
    /// `None` if the output is not mapped to any logical output (for example, if it is disabled).
    pub logical: Option<LogicalOutput>,
    /// Maximum bits per channel (bit depth), if known.
    pub max_bpc: Option<u8>,
}

/// Output mode.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Mode {
    /// Width in physical pixels.
    pub width: u16,
    /// Height in physical pixels.
    pub height: u16,
    /// Refresh rate in millihertz.
    pub refresh_rate: u32,
    /// Whether this mode is preferred by the monitor.
    pub is_preferred: bool,
}

/// Logical output in the compositor's coordinate space.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct LogicalOutput {
    /// Logical X position.
    pub x: i32,
    /// Logical Y position.
    pub y: i32,
    /// Width in logical pixels.
    pub width: u32,
    /// Height in logical pixels.
    pub height: u32,
    /// Scale factor.
    pub scale: f64,
    /// Transform.
    pub transform: Transform,
}

/// Output transform, which goes counter-clockwise.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Transform {
    /// Untransformed.
    Normal,
    /// Rotated by 90°.
    #[serde(rename = "90")]
    _90,
    /// Rotated by 180°.
    #[serde(rename = "180")]
    _180,
    /// Rotated by 270°.
    #[serde(rename = "270")]
    _270,
    /// Flipped horizontally.
    Flipped,
    /// Rotated by 90° and flipped horizontally.
    #[cfg_attr(feature = "clap", value(name("flipped-90")))]
    Flipped90,
    /// Flipped vertically.
    #[cfg_attr(feature = "clap", value(name("flipped-180")))]
    Flipped180,
    /// Rotated by 270° and flipped horizontally.
    #[cfg_attr(feature = "clap", value(name("flipped-270")))]
    Flipped270,
}

/// Output maximum bits per channel.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum MaxBpc {
    /// 6-bit.
    #[serde(rename = "6")]
    _6 = 6,
    /// 8-bit.
    #[default]
    #[serde(rename = "8")]
    _8 = 8,
    /// 10-bit.
    #[serde(rename = "10")]
    _10 = 10,
    /// 12-bit.
    #[serde(rename = "12")]
    _12 = 12,
    /// 14-bit.
    #[serde(rename = "14")]
    _14 = 14,
    /// 16-bit.
    #[serde(rename = "16")]
    _16 = 16,
}

/// Toplevel window.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Window {
    /// Unique id of this window.
    ///
    /// This id remains constant while this window is open.
    ///
    /// Do not assume that window ids will always increase without wrapping, or start at 1. That is
    /// an implementation detail subject to change. For example, ids may change to be randomly
    /// generated for each new window.
    pub id: u64,
    /// Title, if set.
    pub title: Option<String>,
    /// Application ID, if set.
    pub app_id: Option<String>,
    /// Process ID that created the Wayland connection for this window, if known.
    ///
    /// Currently, windows created by xdg-desktop-portal-gnome will have a `None` PID, but this may
    /// change in the future.
    pub pid: Option<i32>,
    /// Id of the workspace this window is on, if any.
    pub workspace_id: Option<u64>,
    /// Whether this window is currently focused.
    ///
    /// There can be either one focused window or zero (e.g. when a layer-shell surface has focus).
    pub is_focused: bool,
    /// Whether this window is currently floating.
    ///
    /// If the window isn't floating then it is in the tiling layout.
    pub is_floating: bool,
    /// Whether this window requests your attention.
    pub is_urgent: bool,
    /// Position- and size-related properties of the window.
    pub layout: WindowLayout,
    /// Timestamp when the window was most recently focused.
    ///
    /// This timestamp is intended for most-recently-used window switchers, i.e. Alt-Tab. It only
    /// updates after some debounce time so that quick window switching doesn't mark intermediate
    /// windows as recently focused.
    ///
    /// The timestamp comes from the monotonic clock.
    pub focus_timestamp: Option<Timestamp>,
}

/// A moment in time.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Timestamp {
    /// Number of whole seconds.
    pub secs: u64,
    /// Fractional part of the timestamp in nanoseconds (10<sup>-9</sup> seconds).
    pub nanos: u32,
}

/// Position- and size-related properties of a [`Window`].
///
/// Optional properties will be unset for some windows, do not rely on them being present. Whether
/// some optional properties are present or absent for certain window types may change across niri
/// releases.
///
/// All sizes and positions are in *logical pixels* unless stated otherwise. Logical sizes may be
/// fractional. For example, at 1.25 monitor scale, a 2-physical-pixel-wide window border is 1.6
/// logical pixels wide.
///
/// This struct contains positions and sizes both for full tiles ([`Self::tile_size`],
/// [`Self::tile_pos_in_workspace_view`]) and the window geometry ([`Self::window_size`],
/// [`Self::window_offset_in_tile`]). For visual displays, use the tile properties, as they
/// correspond to what the user visually considers "window". The window properties on the other
/// hand are mainly useful when you need to know the underlying Wayland window sizes, e.g. for
/// application debugging.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct WindowLayout {
    /// Location of a tiled window within a workspace: (column index, tile index in column).
    ///
    /// The indices are 1-based, i.e. the leftmost column is at index 1 and the topmost tile in a
    /// column is at index 1. This is consistent with [`Action::FocusColumn`] and
    /// [`Action::FocusWindowInColumn`].
    pub pos_in_scrolling_layout: Option<(usize, usize)>,
    /// Size of the tile this window is in, including decorations like borders.
    pub tile_size: (f64, f64),
    /// Size of the window's visual geometry itself.
    ///
    /// Does not include niri decorations like borders.
    ///
    /// Currently, Wayland toplevel windows can only be integer-sized in logical pixels, even
    /// though it doesn't necessarily align to physical pixels.
    pub window_size: (i32, i32),
    /// Tile position within the current view of the workspace.
    ///
    /// This is the same "workspace view" as in gradients' `relative-to` in the niri config.
    pub tile_pos_in_workspace_view: Option<(f64, f64)>,
    /// Location of the window's visual geometry within its tile.
    ///
    /// This includes things like border sizes. For fullscreened fixed-size windows this includes
    /// the distance from the corner of the black backdrop to the corner of the (centered) window
    /// contents.
    pub window_offset_in_tile: (f64, f64),
}

/// Output configuration change result.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum OutputConfigChanged {
    /// The target output was connected and the change was applied.
    Applied,
    /// The target output was not found, the change will be applied when it is connected.
    OutputWasMissing,
}

/// A workspace.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Workspace {
    /// Unique id of this workspace.
    ///
    /// This id remains constant regardless of the workspace moving around and across monitors.
    ///
    /// Do not assume that workspace ids will always increase without wrapping, or start at 1. That
    /// is an implementation detail subject to change. For example, ids may change to be randomly
    /// generated for each new workspace.
    pub id: u64,
    /// Index of the workspace on its monitor.
    ///
    /// This is the same index you can use for requests like `niri msg action focus-workspace`.
    ///
    /// This index *will change* as you move and re-order workspace. It is merely the workspace's
    /// current position on its monitor. Workspaces on different monitors can have the same index.
    ///
    /// If you need a unique workspace id that doesn't change, see [`Self::id`].
    pub idx: u8,
    /// Optional name of the workspace.
    pub name: Option<String>,
    /// Name of the output that the workspace is on.
    ///
    /// Can be `None` if no outputs are currently connected.
    pub output: Option<String>,
    /// Whether the workspace currently has an urgent window in its output.
    pub is_urgent: bool,
    /// Whether the workspace is currently active on its output.
    ///
    /// Every output has one active workspace, the one that is currently visible on that output.
    pub is_active: bool,
    /// Whether the workspace is currently focused.
    ///
    /// There's only one focused workspace across all outputs.
    pub is_focused: bool,
    /// Id of the active window on this workspace, if any.
    pub active_window_id: Option<u64>,
}

/// Configured keyboard layouts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct KeyboardLayouts {
    /// XKB names of the configured layouts.
    pub names: Vec<String>,
    /// Index of the currently active layout in `names`.
    pub current_idx: u8,
}

/// A layer-shell layer.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Layer {
    /// The background layer.
    Background,
    /// The bottom layer.
    Bottom,
    /// The top layer.
    Top,
    /// The overlay layer.
    Overlay,
}

/// Keyboard interactivity modes for a layer-shell surface.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum LayerSurfaceKeyboardInteractivity {
    /// Surface cannot receive keyboard focus.
    None,
    /// Surface receives keyboard focus whenever possible.
    Exclusive,
    /// Surface receives keyboard focus on demand, e.g. when clicked.
    OnDemand,
}

/// A layer-shell surface.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct LayerSurface {
    /// Namespace provided by the layer-shell client.
    pub namespace: String,
    /// Name of the output the surface is on.
    pub output: String,
    /// Layer that the surface is on.
    pub layer: Layer,
    /// The surface's keyboard interactivity mode.
    pub keyboard_interactivity: LayerSurfaceKeyboardInteractivity,
}

/// A screencast.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct Cast {
    /// Stream ID of the screencast that uniquely identifies it.
    pub stream_id: u64,
    /// Session ID of the screencast.
    ///
    /// A session can have multiple screencast streams. Then multiple `Cast`s will have the same
    /// `session_id`. Though, usually there's only one stream per session.
    ///
    /// Do not confuse `session_id` with [`stream_id`](Self::stream_id).
    pub session_id: u64,
    /// Kind of this screencast.
    pub kind: CastKind,
    /// Target being captured.
    pub target: CastTarget,
    /// Whether this is a Dynamic Cast Target screencast.
    ///
    /// Meaning that actions like `SetDynamicCastWindow` will act on this screencast.
    ///
    /// Keep in mind that the target can change even if this is `false`.
    pub is_dynamic_target: bool,
    /// Whether the cast is currently streaming frames.
    ///
    /// This can be `false` for example when switching away to a different scene in OBS, which
    /// pauses the stream.
    pub is_active: bool,
    /// Process ID of the screencast consumer, if known.
    ///
    /// Currently, only wlr-screencopy screencasts can have a pid.
    pub pid: Option<i32>,
    /// PipeWire node ID of the screencast stream.
    ///
    /// This is `None` for wlr-screencopy casts, and also for PipeWire casts before the node is
    /// created (when the cast is just starting up).
    pub pw_node_id: Option<u32>,
}

/// Kind of screencast.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum CastKind {
    /// PipeWire screencast, typically via xdg-desktop-portal-gnome.
    PipeWire,
    /// wlr-screencopy protocol screencast.
    ///
    /// Tools like wf-recorder, and the xdg-desktop-portal-wlr portal.
    ///
    /// Only wlr-screencopy with damage tracking is reported here. Screencopy without damage is
    /// treated as a regular screenshot and not reported as a screencast.
    WlrScreencopy,
    /// ext-image-copy-capture protocol screencast.
    ///
    /// Tools like wayvnc, wl-mirror, and the xdg-desktop-portal-wlr portal.
    ///
    /// A client capturing both an output and the cursor creates two sessions, which are reported
    /// as two `Cast`s sharing one [`session_id`](Cast::session_id).
    ExtImageCopyCapture,
}

/// Target of a screencast.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum CastTarget {
    /// The target is not yet set, or was cleared.
    Nothing {},
    /// Casting an output.
    Output {
        /// Name of the screencasted output.
        name: String,
    },
    /// Casting a window.
    Window {
        /// ID of the screencasted window.
        id: u64,
    },
}

/// A compositor event.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Event {
    /// The workspace configuration has changed.
    WorkspacesChanged {
        /// The new workspace configuration.
        ///
        /// This configuration completely replaces the previous configuration. I.e. if any
        /// workspaces are missing from here, then they were deleted.
        workspaces: Vec<Workspace>,
    },
    /// The workspace urgency changed.
    WorkspaceUrgencyChanged {
        /// Id of the workspace.
        id: u64,
        /// Whether this workspace has an urgent window.
        urgent: bool,
    },
    /// A workspace was activated on an output.
    ///
    /// This doesn't always mean the workspace became focused, just that it's now the active
    /// workspace on its output. All other workspaces on the same output become inactive.
    WorkspaceActivated {
        /// Id of the newly active workspace.
        id: u64,
        /// Whether this workspace also became focused.
        ///
        /// If `true`, this is now the single focused workspace. All other workspaces are no longer
        /// focused, but they may remain active on their respective outputs.
        focused: bool,
    },
    /// An active window changed on a workspace.
    WorkspaceActiveWindowChanged {
        /// Id of the workspace on which the active window changed.
        workspace_id: u64,
        /// Id of the new active window, if any.
        active_window_id: Option<u64>,
    },
    /// The window configuration has changed.
    WindowsChanged {
        /// The new window configuration.
        ///
        /// This configuration completely replaces the previous configuration. I.e. if any windows
        /// are missing from here, then they were closed.
        windows: Vec<Window>,
    },
    /// A new toplevel window was opened, or an existing toplevel window changed.
    WindowOpenedOrChanged {
        /// The new or updated window.
        ///
        /// If the window is focused, all other windows are no longer focused.
        window: Window,
    },
    /// A toplevel window was closed.
    WindowClosed {
        /// Id of the removed window.
        id: u64,
    },
    /// Window focus changed.
    ///
    /// All other windows are no longer focused.
    WindowFocusChanged {
        /// Id of the newly focused window, or `None` if no window is now focused.
        id: Option<u64>,
    },
    /// Window focus timestamp changed.
    ///
    /// This event is separate from [`Event::WindowFocusChanged`] because the focus timestamp only
    /// updates after some debounce time so that quick window switching doesn't mark intermediate
    /// windows as recently focused.
    WindowFocusTimestampChanged {
        /// Id of the window.
        id: u64,
        /// The new focus timestamp.
        focus_timestamp: Option<Timestamp>,
    },
    /// Window urgency changed.
    WindowUrgencyChanged {
        /// Id of the window.
        id: u64,
        /// The new urgency state of the window.
        urgent: bool,
    },
    /// The layout of one or more windows has changed.
    WindowLayoutsChanged {
        /// Pairs consisting of a window id and new layout information for the window.
        changes: Vec<(u64, WindowLayout)>,
    },
    /// The configured keyboard layouts have changed.
    KeyboardLayoutsChanged {
        /// The new keyboard layout configuration.
        keyboard_layouts: KeyboardLayouts,
    },
    /// The keyboard layout switched.
    KeyboardLayoutSwitched {
        /// Index of the newly active layout.
        idx: u8,
    },
    /// The overview was opened or closed.
    OverviewOpenedOrClosed {
        /// The new state of the overview.
        is_open: bool,
    },
    /// The configuration was reloaded.
    ///
    /// You will always receive this event when connecting to the event stream, indicating the last
    /// config load attempt.
    ConfigLoaded {
        /// Whether the loading failed.
        ///
        /// For example, the config file couldn't be parsed.
        failed: bool,
    },
    /// A screenshot was captured.
    ScreenshotCaptured {
        /// The file path where the screenshot was saved, if it was written to disk.
        ///
        /// If `None`, the screenshot was either only copied to the clipboard, or the path couldn't
        /// be converted to a `String` (e.g. contained invalid UTF-8 bytes).
        path: Option<String>,
    },
    /// The screencasts have changed.
    CastsChanged {
        /// The new screencast information.
        ///
        /// This configuration completely replaces the previous configuration. I.e. if any casts
        /// are missing from here, then they were stopped.
        casts: Vec<Cast>,
    },
    /// A screencast started, or an existing cast changed.
    CastStartedOrChanged {
        /// The cast that started or changed.
        cast: Cast,
    },
    /// A screencast stopped.
    CastStopped {
        /// Stream ID of the stopped screencast.
        stream_id: u64,
    },
}

impl From<Duration> for Timestamp {
    fn from(value: Duration) -> Self {
        Timestamp {
            secs: value.as_secs(),
            nanos: value.subsec_nanos(),
        }
    }
}

impl From<Timestamp> for Duration {
    fn from(value: Timestamp) -> Self {
        Duration::new(value.secs, value.nanos)
    }
}

impl FromStr for WorkspaceReferenceArg {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let reference = if let Ok(index) = s.parse::<i32>() {
            if let Ok(idx) = u8::try_from(index) {
                Self::Index(idx)
            } else {
                return Err("workspace index must be between 0 and 255");
            }
        } else {
            Self::Name(s.to_string())
        };

        Ok(reference)
    }
}

impl FromStr for SizeChange {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('%') {
            Some((value, empty)) => {
                if !empty.is_empty() {
                    return Err("trailing characters after '%' are not allowed");
                }

                match value.bytes().next() {
                    Some(b'-' | b'+') => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::AdjustProportion(value))
                    }
                    Some(_) => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::SetProportion(value))
                    }
                    None => Err("value is missing"),
                }
            }
            None => {
                let value = s;
                match value.bytes().next() {
                    Some(b'-' | b'+') => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::AdjustFixed(value))
                    }
                    Some(_) => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::SetFixed(value))
                    }
                    None => Err("value is missing"),
                }
            }
        }
    }
}

impl FromStr for PositionChange {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('%') {
            Some((value, empty)) => {
                if !empty.is_empty() {
                    return Err("trailing characters after '%' are not allowed");
                }

                match value.bytes().next() {
                    Some(b'-' | b'+') => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::AdjustProportion(value))
                    }
                    Some(_) => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::SetProportion(value))
                    }
                    None => Err("value is missing"),
                }
            }
            None => {
                let value = s;
                match value.bytes().next() {
                    Some(b'-' | b'+') => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::AdjustFixed(value))
                    }
                    Some(_) => {
                        let value = value.parse().map_err(|_| "error parsing value")?;
                        Ok(Self::SetFixed(value))
                    }
                    None => Err("value is missing"),
                }
            }
        }
    }
}

impl FromStr for LayoutSwitchTarget {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "next" => Ok(Self::Next),
            "prev" => Ok(Self::Prev),
            other => match other.parse() {
                Ok(layout) => Ok(Self::Index(layout)),
                _ => Err(r#"invalid layout action, can be "next", "prev" or a layout index"#),
            },
        }
    }
}

impl FromStr for ColumnDisplay {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(Self::Normal),
            "tabbed" => Ok(Self::Tabbed),
            _ => Err(r#"invalid column display, can be "normal" or "tabbed""#),
        }
    }
}

impl FromStr for Transform {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(Self::Normal),
            "90" => Ok(Self::_90),
            "180" => Ok(Self::_180),
            "270" => Ok(Self::_270),
            "flipped" => Ok(Self::Flipped),
            "flipped-90" => Ok(Self::Flipped90),
            "flipped-180" => Ok(Self::Flipped180),
            "flipped-270" => Ok(Self::Flipped270),
            _ => Err(concat!(
                r#"invalid transform, can be "90", "180", "270", "#,
                r#""flipped", "flipped-90", "flipped-180" or "flipped-270""#
            )),
        }
    }
}

impl TryFrom<u8> for MaxBpc {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            6 => Ok(MaxBpc::_6),
            8 => Ok(MaxBpc::_8),
            10 => Ok(MaxBpc::_10),
            12 => Ok(MaxBpc::_12),
            14 => Ok(MaxBpc::_14),
            16 => Ok(MaxBpc::_16),
            _ => Err("invalid max-bpc, can be 6, 8, 10, 12, 14, 16"),
        }
    }
}

impl FromStr for MaxBpc {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.parse::<u8>().unwrap_or_default())
    }
}

impl FromStr for Layer {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "background" => Ok(Self::Background),
            "bottom" => Ok(Self::Bottom),
            "top" => Ok(Self::Top),
            "overlay" => Ok(Self::Overlay),
            _ => Err("invalid layer, can be \"background\", \"bottom\", \"top\" or \"overlay\""),
        }
    }
}

impl FromStr for ModeToSet {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Self::Automatic);
        }

        let mode = s.parse()?;
        Ok(Self::Specific(mode))
    }
}

impl FromStr for ConfiguredMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((width, rest)) = s.split_once('x') else {
            return Err("no 'x' separator found");
        };

        let (height, refresh) = match rest.split_once('@') {
            Some((height, refresh)) => (height, Some(refresh)),
            None => (rest, None),
        };

        let width = width.parse().map_err(|_| "error parsing width")?;
        let height = height.parse().map_err(|_| "error parsing height")?;
        let refresh = refresh
            .map(str::parse)
            .transpose()
            .map_err(|_| "error parsing refresh rate")?;

        Ok(Self {
            width,
            height,
            refresh,
        })
    }
}

impl FromStr for HSyncPolarity {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "+hsync" => Ok(Self::PHSync),
            "-hsync" => Ok(Self::NHSync),
            _ => Err(r#"invalid horizontal sync polarity, can be "+hsync" or "-hsync"#),
        }
    }
}

impl FromStr for VSyncPolarity {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "+vsync" => Ok(Self::PVSync),
            "-vsync" => Ok(Self::NVSync),
            _ => Err(r#"invalid vertical sync polarity, can be "+vsync" or "-vsync"#),
        }
    }
}

impl FromStr for ScaleToSet {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Self::Automatic);
        }

        let scale = s.parse().map_err(|_| "error parsing scale")?;
        Ok(Self::Specific(scale))
    }
}

macro_rules! ensure {
    ($cond:expr, $fmt:literal $($arg:tt)* ) => {
        if !$cond {
            return Err(format!($fmt $($arg)*));
        }
    };
}

impl RegionGeometry {
    /// Validates that the region has a non-zero size and that its endpoints fit in `i32`.
    pub fn validate(&self) -> Result<(), String> {
        ensure!(self.width > 0, "region width {} must be > 0", self.width);
        ensure!(self.height > 0, "region height {} must be > 0", self.height);
        ensure!(
            i64::from(self.x) + i64::from(self.width) <= i64::from(i32::MAX),
            "region right edge overflows i32 (x {}, width {})",
            self.x,
            self.width
        );
        ensure!(
            i64::from(self.y) + i64::from(self.height) <= i64::from(i32::MAX),
            "region bottom edge overflows i32 (y {}, height {})",
            self.y,
            self.height
        );
        Ok(())
    }
}

impl OutputAction {
    /// Validates some required constraints on the modeline and custom mode.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            OutputAction::Modeline {
                hdisplay,
                hsync_start,
                hsync_end,
                htotal,
                vdisplay,
                vsync_start,
                vsync_end,
                vtotal,
                ..
            } => {
                ensure!(
                    hdisplay < hsync_start,
                    "hdisplay {} must be < hsync_start {}",
                    hdisplay,
                    hsync_start
                );
                ensure!(
                    hsync_start < hsync_end,
                    "hsync_start {} must be < hsync_end {}",
                    hsync_start,
                    hsync_end
                );
                ensure!(
                    hsync_end < htotal,
                    "hsync_end {} must be < htotal {}",
                    hsync_end,
                    htotal
                );
                ensure!(0 < *htotal, "htotal {} must be > 0", htotal);
                ensure!(
                    vdisplay < vsync_start,
                    "vdisplay {} must be < vsync_start {}",
                    vdisplay,
                    vsync_start
                );
                ensure!(
                    vsync_start < vsync_end,
                    "vsync_start {} must be < vsync_end {}",
                    vsync_start,
                    vsync_end
                );
                ensure!(
                    vsync_end < vtotal,
                    "vsync_end {} must be < vtotal {}",
                    vsync_end,
                    vtotal
                );
                ensure!(0 < *vtotal, "vtotal {} must be > 0", vtotal);
                Ok(())
            }
            OutputAction::CustomMode {
                mode: ConfiguredMode { refresh, .. },
            } => {
                if refresh.is_none() {
                    return Err("refresh rate is required for custom modes".to_string());
                }
                if let Some(refresh) = refresh {
                    if *refresh <= 0. {
                        return Err(format!("custom mode refresh rate {refresh} must be > 0"));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_change() {
        assert_eq!(
            "10".parse::<SizeChange>().unwrap(),
            SizeChange::SetFixed(10),
        );
        assert_eq!(
            "+10".parse::<SizeChange>().unwrap(),
            SizeChange::AdjustFixed(10),
        );
        assert_eq!(
            "-10".parse::<SizeChange>().unwrap(),
            SizeChange::AdjustFixed(-10),
        );
        assert_eq!(
            "10%".parse::<SizeChange>().unwrap(),
            SizeChange::SetProportion(10.),
        );
        assert_eq!(
            "+10%".parse::<SizeChange>().unwrap(),
            SizeChange::AdjustProportion(10.),
        );
        assert_eq!(
            "-10%".parse::<SizeChange>().unwrap(),
            SizeChange::AdjustProportion(-10.),
        );

        assert!("-".parse::<SizeChange>().is_err());
        assert!("10% ".parse::<SizeChange>().is_err());
    }

    #[test]
    fn parse_position_change() {
        assert_eq!(
            "10".parse::<PositionChange>().unwrap(),
            PositionChange::SetFixed(10.),
        );
        assert_eq!(
            "+10".parse::<PositionChange>().unwrap(),
            PositionChange::AdjustFixed(10.),
        );
        assert_eq!(
            "-10".parse::<PositionChange>().unwrap(),
            PositionChange::AdjustFixed(-10.),
        );

        assert_eq!(
            "10%".parse::<PositionChange>().unwrap(),
            PositionChange::SetProportion(10.)
        );
        assert_eq!(
            "+10%".parse::<PositionChange>().unwrap(),
            PositionChange::AdjustProportion(10.)
        );
        assert_eq!(
            "-10%".parse::<PositionChange>().unwrap(),
            PositionChange::AdjustProportion(-10.)
        );
        assert!("-".parse::<PositionChange>().is_err());
        assert!("10% ".parse::<PositionChange>().is_err());
    }

    #[test]
    fn zoom_actions_round_trip() {
        for action in [
            Action::ZoomIn {},
            Action::ZoomOut {},
            Action::SetZoomLevel { level: 2.5 },
            Action::ResetZoom {},
            Action::ZoomLock {},
            Action::Zoom {
                level: 2.,
                lock: false,
            },
            Action::Zoom {
                level: 2.,
                lock: true,
            },
        ] {
            let json = serde_json::to_string(&action).unwrap();
            let parsed: Action = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        }
    }

    #[test]
    fn zoom_request_round_trip() {
        let json = serde_json::to_string(&Request::Zoom).unwrap();
        assert_eq!(json, "\"Zoom\"");
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Request::Zoom));
    }

    #[test]
    fn zoom_response_round_trip() {
        let response = Response::Zoom(vec![
            ZoomState {
                output: String::from("DP-1"),
                level: 2.,
                target_level: 4.,
                effective_level: 1.5,
                focal: (960., 540.5),
                locked: false,
            },
            ZoomState {
                output: String::from("HDMI-A-1"),
                level: 1.,
                target_level: 1.,
                effective_level: 1.,
                focal: (320., 180.),
                locked: true,
            },
        ]);

        let json = serde_json::to_string(&response).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();
        let Response::Zoom(states) = parsed else {
            panic!("expected Zoom response, got {parsed:?}");
        };

        assert_eq!(states.len(), 2);
        assert_eq!(
            states[0],
            ZoomState {
                output: String::from("DP-1"),
                level: 2.,
                target_level: 4.,
                effective_level: 1.5,
                focal: (960., 540.5),
                locked: false,
            }
        );
        assert_eq!(
            states[1],
            ZoomState {
                output: String::from("HDMI-A-1"),
                level: 1.,
                target_level: 1.,
                effective_level: 1.,
                focal: (320., 180.),
                locked: true,
            }
        );
    }

    #[test]
    fn zoom_state_fields_serialize() {
        let state = ZoomState {
            output: String::from("DP-1"),
            level: 2.5,
            target_level: 4.,
            effective_level: 1.25,
            focal: (100.5, 200.25),
            locked: true,
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["output"], "DP-1");
        assert_eq!(json["level"], 2.5);
        assert_eq!(json["target_level"], 4.);
        assert_eq!(json["effective_level"], 1.25);
        assert_eq!(json["focal"], serde_json::json!([100.5, 200.25]));
        assert_eq!(json["locked"], true);
    }

    #[test]
    fn region_geometry_validate() {
        let valid = RegionGeometry {
            x: -100,
            y: 200,
            width: 300,
            height: 400,
        };
        assert_eq!(valid.validate(), Ok(()));

        // Zero size is rejected.
        assert!(RegionGeometry { width: 0, ..valid }.validate().is_err());
        assert!(RegionGeometry { height: 0, ..valid }.validate().is_err());

        // Endpoints overflowing i32 are rejected.
        assert!(RegionGeometry {
            x: i32::MAX,
            width: 1,
            ..valid
        }
        .validate()
        .is_err());
        assert!(RegionGeometry {
            y: i32::MAX,
            height: 1,
            ..valid
        }
        .validate()
        .is_err());

        // Boundary values that still fit are accepted.
        assert_eq!(
            RegionGeometry {
                x: i32::MIN,
                y: i32::MIN,
                width: u32::MAX,
                height: u32::MAX,
            }
            .validate(),
            Ok(())
        );
    }
    fn action_from_json(json: &str) -> Result<Action, serde_json::Error> {
        let request: Request = serde_json::from_str(json)?;
        let Request::Action(action) = request else {
            panic!("expected an Action request, got {request:?}");
        };
        Ok(action)
    }

    #[test]
    fn screenshot_open_old_json_defaults_to_select() {
        // Requests written against the old schema, without a `tool`, must keep working.
        let action =
            action_from_json(r#"{"Action":{"Screenshot":{"show_pointer":false,"path":null}}}"#)
                .unwrap();
        let Action::Screenshot {
            show_pointer,
            path,
            tool,
        } = action
        else {
            panic!("expected Screenshot, got {action:?}");
        };
        assert!(!show_pointer);
        assert_eq!(path, None);
        assert_eq!(tool, ScreenshotTool::Select);

        let action = action_from_json(
            r#"{"Action":{"Screenshot":{"tool":"draw","show_pointer":false,"path":"/tmp/a.png"}}}"#,
        )
        .unwrap();
        let Action::Screenshot {
            show_pointer,
            path,
            tool,
        } = action
        else {
            panic!("expected Screenshot, got {action:?}");
        };
        assert!(!show_pointer);
        assert_eq!(path.as_deref(), Some("/tmp/a.png"));
        assert_eq!(tool, ScreenshotTool::Draw);
    }

    #[test]
    fn screenshot_editor_actions_parse_from_json() {
        let cases: &[(&str, Action)] = &[
            (
                r#"{"Action":{"ScreenshotTool":{"tool":"blur"}}}"#,
                Action::ScreenshotTool {
                    tool: ScreenshotTool::Blur,
                },
            ),
            (
                r#"{"Action":{"ScreenshotSetPalette":{"open":false}}}"#,
                Action::ScreenshotSetPalette { open: false },
            ),
            (
                r#"{"Action":{"ScreenshotPaletteColor":{"index":3}}}"#,
                Action::ScreenshotPaletteColor { index: 3 },
            ),
            (
                r##"{"Action":{"ScreenshotBrushColor":{"color":"#0080ff"}}}"##,
                Action::ScreenshotBrushColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff),
                },
            ),
            (
                r#"{"Action":{"ScreenshotContext":{"change":6}}}"#,
                Action::ScreenshotContext {
                    change: ScreenshotValueChange::Set(6),
                },
            ),
            (
                r#"{"Action":{"ScreenshotContext":{"change":"+1"}}}"#,
                Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(1),
                },
            ),
            (
                r#"{"Action":{"ScreenshotContext":{"change":"-1"}}}"#,
                Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(-1),
                },
            ),
            (
                r#"{"Action":{"ScreenshotSetPointer":{"show_pointer":false}}}"#,
                Action::ScreenshotSetPointer {
                    show_pointer: false,
                },
            ),
            (
                r#"{"Action":{"ScreenshotMoveSelection":{"x":-20,"y":0}}}"#,
                Action::ScreenshotMoveSelection { x: -20, y: 0 },
            ),
            (
                r#"{"Action":{"ScreenshotResizeSelection":{"width":20,"height":-4}}}"#,
                Action::ScreenshotResizeSelection {
                    width: 20,
                    height: -4,
                },
            ),
            (
                r#"{"Action":{"ScreenshotMagnifierZoom":{"zoom":4}}}"#,
                Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Set(4),
                },
            ),
            (
                r#"{"Action":{"ScreenshotMagnifierZoom":{"zoom":"-2"}}}"#,
                Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Adjust(-2),
                },
            ),
            (
                r#"{"Action":{"ConfirmScreenshot":{"write_to_disk":true}}}"#,
                Action::ConfirmScreenshot {
                    write_to_disk: true,
                    format: None,
                    quality: None,
                },
            ),
            (
                r#"{"Action":{"ConfirmScreenshot":{"write_to_disk":false,"format":"webp"}}}"#,
                Action::ConfirmScreenshot {
                    write_to_disk: false,
                    format: Some(ScreenshotFormat::Webp),
                    quality: None,
                },
            ),
            (
                r#"{"Action":{"ScreenshotConfirm":{}}}"#,
                Action::ScreenshotConfirm {},
            ),
            (
                r#"{"Action":{"ScreenshotCancel":{}}}"#,
                Action::ScreenshotCancel {},
            ),
            (
                r#"{"Action":{"CancelScreenshot":{}}}"#,
                Action::CancelScreenshot {},
            ),
            (
                r#"{"Action":{"ScreenshotTogglePalette":{}}}"#,
                Action::ScreenshotTogglePalette {},
            ),
            (
                r#"{"Action":{"ScreenshotFocusNext":{}}}"#,
                Action::ScreenshotFocusNext {},
            ),
            (
                r#"{"Action":{"ScreenshotFocusPrevious":{}}}"#,
                Action::ScreenshotFocusPrevious {},
            ),
            (
                r#"{"Action":{"ScreenshotTogglePointer":{}}}"#,
                Action::ScreenshotTogglePointer {},
            ),
            (
                r#"{"Action":{"ScreenshotTool":{"tool":"arrow"}}}"#,
                Action::ScreenshotTool {
                    tool: ScreenshotTool::Arrow,
                },
            ),
            (
                r#"{"Action":{"ScreenshotTool":{"tool":"rectangle"}}}"#,
                Action::ScreenshotTool {
                    tool: ScreenshotTool::Rectangle,
                },
            ),
            (
                r##"{"Action":{"ScreenshotArrowColor":{"color":"#0080ff"}}}"##,
                Action::ScreenshotArrowColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff),
                },
            ),
            (
                r##"{"Action":{"ScreenshotRectangleColor":{"color":"#0080ff"}}}"##,
                Action::ScreenshotRectangleColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff),
                },
            ),
            (
                r#"{"Action":{"ScreenshotToggleAxes":{}}}"#,
                Action::ScreenshotToggleAxes {},
            ),
            (
                r#"{"Action":{"ScreenshotSetAxes":{"enabled":false}}}"#,
                Action::ScreenshotSetAxes { enabled: false },
            ),
        ];

        for (json, expected) in cases {
            let action = action_from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"));
            // Action does not implement PartialEq; compare the serialized forms.
            assert_eq!(
                serde_json::to_value(&action).unwrap(),
                serde_json::to_value(expected).unwrap(),
                "{json}: got {action:?}"
            );
            let json_round_trip = serde_json::to_string(&Request::Action(action)).unwrap();
            assert!(
                action_from_json(&json_round_trip).is_ok(),
                "{json}: round trip failed for {json_round_trip}"
            );
        }
    }

    #[test]
    fn screenshot_actions_reject_bad_arguments() {
        for json in [
            // Invalid tool enum value.
            r#"{"Action":{"ScreenshotTool":{"tool":"crop"}}}"#,
            r#"{"Action":{"Screenshot":{"tool":"crop","show_pointer":true}}}"#,
            // Malformed color strings.
            r#"{"Action":{"ScreenshotBrushColor":{"color":"0080ff"}}}"#,
            r##"{"Action":{"ScreenshotBrushColor":{"color":"#80ff"}}}"##,
            r##"{"Action":{"ScreenshotBrushColor":{"color":"#0080ffcc"}}}"##,
            r#"{"Action":{"ScreenshotBrushColor":{"color":"red"}}}"#,
            // Malformed value changes.
            r#"{"Action":{"ScreenshotContext":{"change":"abc"}}}"#,
            r#"{"Action":{"ScreenshotContext":{"change":-1}}}"#,
            r#"{"Action":{"ScreenshotContext":{"change":1.5}}}"#,
            r#"{"Action":{"ScreenshotContext":{"change":4294967296}}}"#,
            r#"{"Action":{"ScreenshotContext":{"change":"high"}}}"#,
            r#"{"Action":{"ScreenshotMagnifierZoom":{"zoom":"+"}}}"#,
            r#"{"Action":{"ScreenshotMagnifierZoom":{"zoom":"-abc"}}}"#,
            // Wrong types.
            r#"{"Action":{"ScreenshotPaletteColor":{"index":"3"}}}"#,
            r#"{"Action":{"ScreenshotMoveSelection":{"x":"-20","y":0}}}"#,
            r#"{"Action":{"ScreenshotSetAxes":{"enabled":"true"}}}"#,
            // The old per-tool width and blur-strength actions no longer exist, and neither
            // does the intermediate factor name.
            r#"{"Action":{"ScreenshotBrushWidth":{"width":6}}}"#,
            r#"{"Action":{"ScreenshotArrowWidth":{"width":8}}}"#,
            r#"{"Action":{"ScreenshotRectangleWidth":{"width":"-1"}}}"#,
            r#"{"Action":{"ScreenshotBlurStrength":{"strength":2}}}"#,
            r#"{"Action":{"ScreenshotFactor":{"factor":"+1"}}}"#,
            r##"{"Action":{"ScreenshotRectangleColor":{"color":"#80ff"}}}"##,
            // Invalid format enum value.
            r#"{"Action":{"ConfirmScreenshot":{"write_to_disk":true,"format":"jpeg"}}}"#,
        ] {
            let json: &str = json;
            assert!(
                action_from_json(json).is_err(),
                "expected a parse error for {json}"
            );
        }
    }

    #[test]
    fn screenshot_format_parsing() {
        for (name, format) in [
            ("png", ScreenshotFormat::Png),
            ("webp", ScreenshotFormat::Webp),
            ("avif", ScreenshotFormat::Avif),
        ] {
            assert_eq!(name.parse::<ScreenshotFormat>().unwrap(), format);
            assert_eq!(format.to_string(), name);
            assert_eq!(serde_json::to_value(format).unwrap(), name);
        }

        for bad in ["jpeg", "", "PNG", "png "] {
            assert!(bad.parse::<ScreenshotFormat>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn screenshot_quality_validation() {
        for quality in [-1., 100.1, f32::NAN, f32::INFINITY] {
            assert!(Action::ConfirmScreenshot {
                write_to_disk: true,
                format: None,
                quality: Some(quality)
            }
            .validate()
            .is_err());
        }
        for quality in [0., 37.5, 100.] {
            let json = format!(
                r#"{{"Action":{{"ConfirmScreenshot":{{"write_to_disk":true,"format":"webp","quality":{quality}}}}}}}"#
            );
            let action = action_from_json(&json).unwrap();
            assert!(action.validate().is_ok());
            assert!(
                matches!(action, Action::ConfirmScreenshot { quality: Some(q), .. } if q == quality)
            );
        }
        assert!(Action::ConfirmScreenshot {
            write_to_disk: true,
            format: Some(ScreenshotFormat::Avif),
            quality: Some(0.)
        }
        .validate()
        .is_err());
    }

    #[test]
    fn screenshot_actions_validate() {
        // An absolute value is limited to 1-128 for every pointer context; adjustments are
        // always accepted and clamped at runtime.
        for change in [0, 129, u32::MAX] {
            let action = Action::ScreenshotContext {
                change: ScreenshotValueChange::Set(change),
            };
            assert!(
                action.validate().is_err(),
                "value {change} must be rejected"
            );
        }
        for change in [MIN_SCREENSHOT_BRUSH_WIDTH, MAX_SCREENSHOT_BRUSH_WIDTH, 6] {
            let action = Action::ScreenshotContext {
                change: ScreenshotValueChange::Set(change),
            };
            assert_eq!(action.validate(), Ok(()));
        }
        for delta in [i32::MIN, -1, 0, 1, i32::MAX] {
            let action = Action::ScreenshotContext {
                change: ScreenshotValueChange::Adjust(delta),
            };
            assert_eq!(action.validate(), Ok(()));
        }

        // Absolute magnifier zoom is a fixed ladder; adjustments are clamped at runtime.
        for zoom in [0, 3, 5, 64, u32::MAX] {
            let action = Action::ScreenshotMagnifierZoom {
                zoom: ScreenshotValueChange::Set(zoom),
            };
            assert!(action.validate().is_err(), "zoom {zoom} must be rejected");
        }
        for zoom in [1, 2, 4, 8, 16, 32] {
            let action = Action::ScreenshotMagnifierZoom {
                zoom: ScreenshotValueChange::Set(zoom),
            };
            assert_eq!(action.validate(), Ok(()));
        }

        assert_eq!(Action::ScreenshotToggleAxes {}.validate(), Ok(()));
        assert_eq!(
            Action::ScreenshotSetAxes { enabled: true }.validate(),
            Ok(())
        );

        // Palette indices are 1-based and bounded by the maximum palette size.
        for index in [0, 8, u8::MAX] {
            let action = Action::ScreenshotPaletteColor { index };
            assert!(action.validate().is_err(), "index {index} must be rejected");
        }
        for index in 1..=7 {
            let action = Action::ScreenshotPaletteColor { index };
            assert_eq!(action.validate(), Ok(()));
        }

        // Unrelated actions validate fine.
        assert_eq!(
            Action::ScreenshotMoveSelection {
                x: i32::MIN,
                y: i32::MAX
            }
            .validate(),
            Ok(())
        );
        assert_eq!(Action::CancelScreenshot {}.validate(), Ok(()));
    }

    #[test]
    fn screenshot_value_change_from_str() {
        assert_eq!("6".parse(), Ok(ScreenshotValueChange::Set(6)));
        assert_eq!("+1".parse(), Ok(ScreenshotValueChange::Adjust(1)));
        assert_eq!("-1".parse(), Ok(ScreenshotValueChange::Adjust(-1)));
        assert_eq!(
            "-2147483648".parse(),
            Ok(ScreenshotValueChange::Adjust(i32::MIN))
        );

        assert!("".parse::<ScreenshotValueChange>().is_err());
        assert!("+".parse::<ScreenshotValueChange>().is_err());
        assert!("-".parse::<ScreenshotValueChange>().is_err());
        assert!("abc".parse::<ScreenshotValueChange>().is_err());
        assert!("-1.5".parse::<ScreenshotValueChange>().is_err());
        assert!("4294967296".parse::<ScreenshotValueChange>().is_err());
        assert!("+2147483648".parse::<ScreenshotValueChange>().is_err());
        assert!("-2147483649".parse::<ScreenshotValueChange>().is_err());
    }

    #[test]
    fn screenshot_color_parsing() {
        assert_eq!(
            "#0080ff".parse(),
            Ok(ScreenshotColor::new(0x00, 0x80, 0xff))
        );
        assert_eq!(
            "#A6D9F2".parse(),
            Ok(ScreenshotColor::new(0xa6, 0xd9, 0xf2))
        );

        for bad in [
            "",
            "#",
            "0080ff",
            "#80ff",
            "#0080ffcc",
            "red",
            "#gggggg",
            " #0080ff",
        ] {
            assert!(
                bad.parse::<ScreenshotColor>().is_err(),
                "expected an error for {bad:?}"
            );
        }

        let color = ScreenshotColor::new(0x80, 0xc7, 0xff);
        assert_eq!(serde_json::to_string(&color).unwrap(), "\"#80c7ff\"");
        let parsed: ScreenshotColor = serde_json::from_str("\"#80c7ff\"").unwrap();
        assert_eq!(parsed, color);
        assert_eq!(color.rgb(), (128. / 255., 199. / 255., 1.));
        assert_eq!(ScreenshotColor::default(), ScreenshotColor::new(0xff, 0, 0));
    }

    #[cfg(feature = "clap")]
    #[test]
    fn screenshot_actions_parse_from_cli() {
        use clap::Parser;

        let cases: &[(&[&str], Action)] = &[
            (
                &["screenshot", "--tool", "draw", "--show-pointer", "false"],
                Action::Screenshot {
                    show_pointer: false,
                    path: None,
                    tool: ScreenshotTool::Draw,
                },
            ),
            (
                &["screenshot"],
                Action::Screenshot {
                    show_pointer: true,
                    path: None,
                    tool: ScreenshotTool::Select,
                },
            ),
            (
                &["screenshot-tool", "blur"],
                Action::ScreenshotTool {
                    tool: ScreenshotTool::Blur,
                },
            ),
            (
                &["screenshot-set-palette", "true"],
                Action::ScreenshotSetPalette { open: true },
            ),
            (
                &["screenshot-palette-color", "3"],
                Action::ScreenshotPaletteColor { index: 3 },
            ),
            (
                &["screenshot-brush-color", "#0080ff"],
                Action::ScreenshotBrushColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff),
                },
            ),
            (
                &["screenshot-context", "6"],
                Action::ScreenshotContext {
                    change: ScreenshotValueChange::Set(6),
                },
            ),
            (
                &["screenshot-context", "+1"],
                Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(1),
                },
            ),
            // Leading hyphen values must not be parsed as options.
            (
                &["screenshot-context", "-1"],
                Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(-1),
                },
            ),
            (
                &["screenshot-set-pointer", "false"],
                Action::ScreenshotSetPointer {
                    show_pointer: false,
                },
            ),
            (
                &["screenshot-move-selection", "--x", "-20", "--y", "0"],
                Action::ScreenshotMoveSelection { x: -20, y: 0 },
            ),
            (
                &["screenshot-move-selection", "--x", "-20"],
                Action::ScreenshotMoveSelection { x: -20, y: 0 },
            ),
            (
                &[
                    "screenshot-resize-selection",
                    "--width",
                    "-5",
                    "--height",
                    "2",
                ],
                Action::ScreenshotResizeSelection {
                    width: -5,
                    height: 2,
                },
            ),
            (
                &["screenshot-magnifier-zoom", "4"],
                Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Set(4),
                },
            ),
            (
                &["screenshot-magnifier-zoom", "-1"],
                Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Adjust(-1),
                },
            ),
            (
                &["confirm-screenshot", "--write-to-disk", "false"],
                Action::ConfirmScreenshot {
                    write_to_disk: false,
                    format: None,
                    quality: None,
                },
            ),
            (
                &[
                    "confirm-screenshot",
                    "--format",
                    "avif",
                    "--quality",
                    "37.5",
                ],
                Action::ConfirmScreenshot {
                    write_to_disk: true,
                    format: Some(ScreenshotFormat::Avif),
                    quality: Some(37.5),
                },
            ),
            (
                &["confirm-screenshot"],
                Action::ConfirmScreenshot {
                    write_to_disk: true,
                    format: None,
                    quality: None,
                },
            ),
            (&["screenshot-confirm"], Action::ScreenshotConfirm {}),
            (&["screenshot-cancel"], Action::ScreenshotCancel {}),
            (&["cancel-screenshot"], Action::CancelScreenshot {}),
            (&["screenshot-undo"], Action::ScreenshotUndo {}),
            (&["screenshot-redo"], Action::ScreenshotRedo {}),
            (
                &["screenshot-toggle-palette"],
                Action::ScreenshotTogglePalette {},
            ),
            (
                &["screenshot-toggle-pointer"],
                Action::ScreenshotTogglePointer {},
            ),
            (&["screenshot-focus-next"], Action::ScreenshotFocusNext {}),
            (
                &["screenshot-focus-previous"],
                Action::ScreenshotFocusPrevious {},
            ),
            (
                &["screenshot-arrow-color", "#0080ff"],
                Action::ScreenshotArrowColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff),
                },
            ),
            (
                &["screenshot-rectangle-color", "#0080ff"],
                Action::ScreenshotRectangleColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff),
                },
            ),
            (&["screenshot-toggle-axes"], Action::ScreenshotToggleAxes {}),
            (
                &["screenshot-set-axes", "false"],
                Action::ScreenshotSetAxes { enabled: false },
            ),
            (
                &["screenshot-tool", "arrow"],
                Action::ScreenshotTool {
                    tool: ScreenshotTool::Arrow,
                },
            ),
            (
                &["screenshot-tool", "rectangle"],
                Action::ScreenshotTool {
                    tool: ScreenshotTool::Rectangle,
                },
            ),
        ];

        for (args, expected) in cases {
            let argv: Vec<&str> = ["niri msg action"]
                .into_iter()
                .chain(args.iter().copied())
                .collect();
            let action = Action::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            // Action does not implement PartialEq; compare the serialized forms.
            assert_eq!(
                serde_json::to_value(&action).unwrap(),
                serde_json::to_value(expected).unwrap(),
                "{argv:?}: got {action:?}"
            );
        }

        // The per-tool width and blur-strength actions are replaced by the single
        // `screenshot-context` action; their old names and the intermediate
        // `screenshot-factor` name must not parse.
        for name in [
            "screenshot-brush-width",
            "screenshot-arrow-width",
            "screenshot-rectangle-width",
            "screenshot-blur-strength",
            "screenshot-factor",
        ] {
            let argv = ["niri", "msg", "action", name, "6"];
            assert!(
                Action::try_parse_from(&argv).is_err(),
                "{argv:?} must be rejected"
            );
        }
    }
}
