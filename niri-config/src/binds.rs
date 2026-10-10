use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use bitflags::bitflags;
use knuffel::errors::DecodeError;
use miette::miette;
use niri_ipc::{
    ColumnDisplay, LayoutSwitchTarget, PositionChange, ScreenshotColor, ScreenshotFormat,
    ScreenshotTool, ScreenshotValueChange, SizeChange, WorkspaceReferenceArg,
    MAX_SCREENSHOT_BRUSH_WIDTH, MIN_SCREENSHOT_BRUSH_WIDTH,
};
use smithay::input::keyboard::keysyms::KEY_NoSymbol;
use smithay::input::keyboard::xkb::{keysym_from_name, KEYSYM_CASE_INSENSITIVE, KEYSYM_NO_FLAGS};
use smithay::input::keyboard::Keysym;

use crate::recent_windows::{MruDirection, MruFilter, MruScope};
use crate::utils::{expect_only_children, FloatOrInt, MergeWith};

#[derive(Debug, Default, PartialEq)]
pub struct Binds(pub Vec<Bind>);

#[derive(Debug, Clone, PartialEq)]
pub struct Bind {
    pub key: Key,
    pub action: Action,
    pub repeat: bool,
    pub cooldown: Option<Duration>,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
    pub hotkey_overlay_title: Option<Option<String>>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct Key {
    pub trigger: Trigger,
    pub modifiers: Modifiers,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum Trigger {
    Keysym(Keysym),
    MouseLeft,
    MouseRight,
    MouseMiddle,
    MouseBack,
    MouseForward,
    WheelScrollDown,
    WheelScrollUp,
    WheelScrollLeft,
    WheelScrollRight,
    TouchpadScrollDown,
    TouchpadScrollUp,
    TouchpadScrollLeft,
    TouchpadScrollRight,
    TabletStylusButton1,
    TabletStylusButton2,
    TabletStylusButton3,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Modifiers : u8 {
        const CTRL = 1;
        const SHIFT = 1 << 1;
        const ALT = 1 << 2;
        const SUPER = 1 << 3;
        const ISO_LEVEL3_SHIFT = 1 << 4;
        const ISO_LEVEL5_SHIFT = 1 << 5;
        const COMPOSITOR = 1 << 6;
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct SwitchBinds {
    #[knuffel(child)]
    pub lid_open: Option<SwitchAction>,
    #[knuffel(child)]
    pub lid_close: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_on: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_off: Option<SwitchAction>,
}

impl MergeWith<SwitchBinds> for SwitchBinds {
    fn merge_with(&mut self, part: &SwitchBinds) {
        merge_clone_opt!(
            (self, part),
            lid_open,
            lid_close,
            tablet_mode_on,
            tablet_mode_off,
        );
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct SwitchAction {
    #[knuffel(child, unwrap(arguments))]
    pub spawn: Vec<String>,
}

// Remember to add new actions to the CLI enum too.
#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub enum Action {
    Quit(#[knuffel(property(name = "skip-confirmation"), default)] bool),
    #[knuffel(skip)]
    ChangeVt(i32),
    Suspend,
    PowerOffMonitors,
    PowerOnMonitors,
    ToggleDebugTint,
    DebugToggleOpaqueRegions,
    DebugToggleDamage,
    Spawn(#[knuffel(arguments)] Vec<String>),
    SpawnSh(#[knuffel(argument)] String),
    DoScreenTransition(#[knuffel(property(name = "delay-ms"))] Option<u16>),
    // Opening the editor is not editor-local: it works while the UI is closed.
    //
    // `knuffel` cannot derive named-field variants, so this (and the other payload
    // screenshot actions below) are decoded manually in `decode_screenshot_action`.
    #[knuffel(skip)]
    Screenshot {
        show_pointer: bool,
        // Path; not settable from knuffel.
        path: Option<String>,
        tool: ScreenshotTool,
    },
    #[knuffel(skip)]
    ScreenshotTool {
        tool: ScreenshotTool,
    },
    ScreenshotTogglePalette,
    #[knuffel(skip)]
    ScreenshotSetPalette {
        open: bool,
    },
    #[knuffel(skip)]
    ScreenshotPaletteColor {
        index: u8,
    },
    #[knuffel(skip)]
    ScreenshotBrushColor {
        color: ScreenshotColor,
    },
    /// Adjusts the keyboard-focused control, falling back to pointer context:
    /// draw/arrow/rectangle width, blur strength, or magnifier zoom over canvas.
    /// Other controls and Select are a no-op. Mouse movement clears keyboard focus.
    #[knuffel(skip)]
    ScreenshotContext {
        change: ScreenshotValueChange,
    },
    #[knuffel(skip)]
    ScreenshotArrowColor {
        color: ScreenshotColor,
    },
    #[knuffel(skip)]
    ScreenshotRectangleColor {
        color: ScreenshotColor,
    },
    ScreenshotUndo,
    ScreenshotRedo,
    ScreenshotTogglePointer,
    #[knuffel(skip)]
    ScreenshotSetPointer {
        show_pointer: bool,
    },
    ScreenshotToggleAxes(#[knuffel(property(name = "hold"), default)] bool),
    #[knuffel(skip)]
    ScreenshotSetAxes {
        enabled: bool,
    },
    #[knuffel(skip)]
    ScreenshotMoveSelection {
        x: i32,
        y: i32,
    },
    #[knuffel(skip)]
    ScreenshotResizeSelection {
        width: i32,
        height: i32,
    },
    // Bound to a physical key; there is no matching IPC variant because IPC has no
    // paired key release.
    ScreenshotMoveSelectionHold,
    #[knuffel(skip)]
    ScreenshotMagnifierZoom {
        zoom: ScreenshotValueChange,
    },
    ScreenshotFocusNext,
    ScreenshotFocusPrevious,
    ScreenshotConfirm,
    ScreenshotCancel,
    #[knuffel(skip)]
    ConfirmScreenshot {
        write_to_disk: bool,
        format: Option<ScreenshotFormat>,
        quality: Option<f32>,
    },
    CancelScreenshot,
    ScreenshotScreen(
        #[knuffel(property(name = "write-to-disk"), default = true)] bool,
        #[knuffel(property(name = "show-pointer"), default = true)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    ScreenshotWindow(
        #[knuffel(property(name = "write-to-disk"), default = true)] bool,
        #[knuffel(property(name = "show-pointer"), default = false)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    #[knuffel(skip)]
    ScreenshotWindowById {
        id: u64,
        write_to_disk: bool,
        show_pointer: bool,
        path: Option<String>,
    },
    ToggleKeyboardShortcutsInhibit,
    CloseWindow,
    #[knuffel(skip)]
    CloseWindowById(u64),
    FullscreenWindow,
    #[knuffel(skip)]
    FullscreenWindowById(u64),
    ToggleWindowedFullscreen,
    #[knuffel(skip)]
    ToggleWindowedFullscreenById(u64),
    #[knuffel(skip)]
    FocusWindow(u64),
    FocusWindowInColumn(#[knuffel(argument)] u8),
    FocusWindowPrevious,
    FocusColumnLeft,
    #[knuffel(skip)]
    FocusColumnLeftUnderMouse,
    FocusColumnRight,
    #[knuffel(skip)]
    FocusColumnRightUnderMouse,
    FocusColumnFirst,
    FocusColumnLast,
    FocusColumnRightOrFirst,
    FocusColumnLeftOrLast,
    FocusColumn(#[knuffel(argument)] usize),
    FocusWindowOrMonitorUp,
    FocusWindowOrMonitorDown,
    FocusColumnOrMonitorLeft,
    FocusColumnOrMonitorRight,
    FocusWindowDown,
    FocusWindowUp,
    FocusWindowDownOrColumnLeft,
    FocusWindowDownOrColumnRight,
    FocusWindowUpOrColumnLeft,
    FocusWindowUpOrColumnRight,
    FocusWindowOrWorkspaceDown,
    FocusWindowOrWorkspaceUp,
    FocusWindowTop,
    FocusWindowBottom,
    FocusWindowDownOrTop,
    FocusWindowUpOrBottom,
    MoveColumnLeft,
    MoveColumnRight,
    MoveColumnToFirst,
    MoveColumnToLast,
    MoveColumnLeftOrToMonitorLeft,
    MoveColumnRightOrToMonitorRight,
    MoveColumnToIndex(#[knuffel(argument)] usize),
    MoveWindowDown,
    MoveWindowUp,
    MoveWindowDownOrToWorkspaceDown,
    MoveWindowUpOrToWorkspaceUp,
    ConsumeOrExpelWindowLeft,
    #[knuffel(skip)]
    ConsumeOrExpelWindowLeftById(u64),
    ConsumeOrExpelWindowRight,
    #[knuffel(skip)]
    ConsumeOrExpelWindowRightById(u64),
    ConsumeWindowIntoColumn,
    ExpelWindowFromColumn,
    SwapWindowLeft,
    SwapWindowRight,
    ToggleColumnTabbedDisplay,
    SetColumnDisplay(#[knuffel(argument, str)] ColumnDisplay),
    CenterColumn,
    CenterWindow,
    #[knuffel(skip)]
    CenterWindowById(u64),
    CenterVisibleColumns,
    FocusWorkspaceDown,
    #[knuffel(skip)]
    FocusWorkspaceDownUnderMouse,
    FocusWorkspaceUp,
    #[knuffel(skip)]
    FocusWorkspaceUpUnderMouse,
    FocusWorkspace(#[knuffel(argument)] WorkspaceReference),
    FocusWorkspacePrevious,
    MoveWindowToWorkspaceDown(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveWindowToWorkspaceUp(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveWindowToWorkspace(
        #[knuffel(argument)] WorkspaceReference,
        #[knuffel(property(name = "focus"), default = true)] bool,
    ),
    #[knuffel(skip)]
    MoveWindowToWorkspaceById {
        window_id: u64,
        reference: WorkspaceReference,
        focus: bool,
    },
    MoveColumnToWorkspaceDown(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveColumnToWorkspaceUp(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveColumnToWorkspace(
        #[knuffel(argument)] WorkspaceReference,
        #[knuffel(property(name = "focus"), default = true)] bool,
    ),
    MoveWorkspaceDown,
    MoveWorkspaceUp,
    MoveWorkspaceToIndex(#[knuffel(argument)] usize),
    #[knuffel(skip)]
    MoveWorkspaceToIndexByRef {
        new_idx: usize,
        reference: WorkspaceReference,
    },
    #[knuffel(skip)]
    MoveWorkspaceToMonitorByRef {
        output_name: String,
        reference: WorkspaceReference,
    },
    MoveWorkspaceToMonitor(#[knuffel(argument)] String),
    SetWorkspaceName(#[knuffel(argument)] String),
    #[knuffel(skip)]
    SetWorkspaceNameByRef {
        name: String,
        reference: WorkspaceReference,
    },
    UnsetWorkspaceName,
    #[knuffel(skip)]
    UnsetWorkSpaceNameByRef(#[knuffel(argument)] WorkspaceReference),
    FocusMonitorLeft,
    FocusMonitorRight,
    FocusMonitorDown,
    FocusMonitorUp,
    FocusMonitorPrevious,
    FocusMonitorNext,
    FocusMonitor(#[knuffel(argument)] String),
    MoveWindowToMonitorLeft,
    MoveWindowToMonitorRight,
    MoveWindowToMonitorDown,
    MoveWindowToMonitorUp,
    MoveWindowToMonitorPrevious,
    MoveWindowToMonitorNext,
    MoveWindowToMonitor(#[knuffel(argument)] String),
    #[knuffel(skip)]
    MoveWindowToMonitorById {
        id: u64,
        output: String,
    },
    MoveColumnToMonitorLeft,
    MoveColumnToMonitorRight,
    MoveColumnToMonitorDown,
    MoveColumnToMonitorUp,
    MoveColumnToMonitorPrevious,
    MoveColumnToMonitorNext,
    MoveColumnToMonitor(#[knuffel(argument)] String),
    SetWindowWidth(#[knuffel(argument, str)] SizeChange),
    #[knuffel(skip)]
    SetWindowWidthById {
        id: u64,
        change: SizeChange,
    },
    SetWindowHeight(#[knuffel(argument, str)] SizeChange),
    #[knuffel(skip)]
    SetWindowHeightById {
        id: u64,
        change: SizeChange,
    },
    ResetWindowHeight,
    #[knuffel(skip)]
    ResetWindowHeightById(u64),
    SwitchPresetColumnWidth,
    SwitchPresetColumnWidthBack,
    SwitchPresetWindowWidth,
    SwitchPresetWindowWidthBack,
    #[knuffel(skip)]
    SwitchPresetWindowWidthById(u64),
    #[knuffel(skip)]
    SwitchPresetWindowWidthBackById(u64),
    SwitchPresetWindowHeight,
    SwitchPresetWindowHeightBack,
    #[knuffel(skip)]
    SwitchPresetWindowHeightById(u64),
    #[knuffel(skip)]
    SwitchPresetWindowHeightBackById(u64),
    MaximizeColumn,
    MaximizeWindowToEdges,
    #[knuffel(skip)]
    MaximizeWindowToEdgesById(u64),
    SetColumnWidth(#[knuffel(argument, str)] SizeChange),
    ExpandColumnToAvailableWidth,
    SwitchLayout(#[knuffel(argument, str)] LayoutSwitchTarget),
    ShowHotkeyOverlay,
    MoveWorkspaceToMonitorLeft,
    MoveWorkspaceToMonitorRight,
    MoveWorkspaceToMonitorDown,
    MoveWorkspaceToMonitorUp,
    MoveWorkspaceToMonitorPrevious,
    MoveWorkspaceToMonitorNext,
    ToggleWindowFloating,
    #[knuffel(skip)]
    ToggleWindowFloatingById(u64),
    MoveWindowToFloating,
    #[knuffel(skip)]
    MoveWindowToFloatingById(u64),
    MoveWindowToTiling,
    #[knuffel(skip)]
    MoveWindowToTilingById(u64),
    FocusFloating,
    FocusTiling,
    SwitchFocusBetweenFloatingAndTiling,
    #[knuffel(skip)]
    MoveFloatingWindowById {
        id: Option<u64>,
        x: PositionChange,
        y: PositionChange,
    },
    ToggleWindowRuleOpacity,
    #[knuffel(skip)]
    ToggleWindowRuleOpacityById(u64),
    SetDynamicCastWindow,
    #[knuffel(skip)]
    SetDynamicCastWindowById(u64),
    SetDynamicCastMonitor(#[knuffel(argument)] Option<String>),
    ClearDynamicCastTarget,
    #[knuffel(skip)]
    StopCast(u64),
    ToggleOverview,
    OpenOverview,
    CloseOverview,
    #[knuffel(skip)]
    ToggleWindowUrgent(u64),
    #[knuffel(skip)]
    SetWindowUrgent(u64),
    #[knuffel(skip)]
    UnsetWindowUrgent(u64),
    #[knuffel(skip)]
    LoadConfigFile(#[knuffel(argument)] Option<String>),
    #[knuffel(skip)]
    MruAdvance {
        direction: MruDirection,
        scope: Option<MruScope>,
        filter: Option<MruFilter>,
    },
    #[knuffel(skip)]
    MruConfirm,
    #[knuffel(skip)]
    MruCancel,
    #[knuffel(skip)]
    MruCloseCurrentWindow,
    #[knuffel(skip)]
    MruFirst,
    #[knuffel(skip)]
    MruLast,
    #[knuffel(skip)]
    MruSetScope(MruScope),
    #[knuffel(skip)]
    MruCycleScope,
    ZoomIn,
    ZoomOut,
    SetZoomLevel(#[knuffel(argument)] FloatOrInt<1, { i32::MAX }>),
    ResetZoom,
    ZoomLock(#[knuffel(property(name = "hold"), default)] bool),
    Zoom(
        #[knuffel(argument)] ZoomLevelPreset,
        #[knuffel(property(name = "hold"), default)] bool,
        #[knuffel(property(name = "lock"), default)] bool,
    ),

    // Doesn't look like knuffel allows gating on #[cfg(test)]...
    #[knuffel(skip)]
    TestAction,
}

/// A zoom level preset for `zoom`: a finite number strictly greater than 1.
///
/// A preset of 1 would make activation a no-op, so unlike [`FloatOrInt`] the
/// lower bound is exclusive. Values above `zoom.max-zoom` are allowed here and
/// clamped at runtime, since the maximum can change on config reload.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ZoomLevelPreset(pub f64);

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for ZoomLevelPreset {
    fn type_check(
        type_name: &Option<knuffel::span::Spanned<knuffel::ast::TypeName, S>>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(type_name) = &type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }
    }

    fn raw_decode(
        val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let value = match &**val {
            knuffel::ast::Literal::Int(value) => match i32::try_from(value) {
                Ok(v) => f64::from(v),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    return Ok(Self::default());
                }
            },
            knuffel::ast::Literal::Decimal(value) => match f64::try_from(value) {
                Ok(v) => v,
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    return Ok(Self::default());
                }
            },
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only numbers are recognized",
                ));
                return Ok(Self::default());
            }
        };

        if value.is_finite() && value > 1. {
            Ok(ZoomLevelPreset(value))
        } else {
            ctx.emit_error(DecodeError::conversion(val, "value must be greater than 1"));
            Ok(Self::default())
        }
    }
}

impl From<niri_ipc::Action> for Action {
    fn from(value: niri_ipc::Action) -> Self {
        match value {
            niri_ipc::Action::Quit { skip_confirmation } => Self::Quit(skip_confirmation),
            niri_ipc::Action::PowerOffMonitors {} => Self::PowerOffMonitors,
            niri_ipc::Action::PowerOnMonitors {} => Self::PowerOnMonitors,
            niri_ipc::Action::Spawn { command } => Self::Spawn(command),
            niri_ipc::Action::SpawnSh { command } => Self::SpawnSh(command),
            niri_ipc::Action::DoScreenTransition { delay_ms } => Self::DoScreenTransition(delay_ms),
            niri_ipc::Action::Screenshot {
                show_pointer,
                path,
                tool,
            } => Self::Screenshot {
                show_pointer,
                path,
                tool,
            },
            niri_ipc::Action::ScreenshotScreen {
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotScreen(write_to_disk, show_pointer, path),
            niri_ipc::Action::ScreenshotWindow {
                id: None,
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotWindow(write_to_disk, show_pointer, path),
            niri_ipc::Action::ScreenshotWindow {
                id: Some(id),
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotWindowById {
                id,
                write_to_disk,
                show_pointer,
                path,
            },
            niri_ipc::Action::ScreenshotTool { tool } => Self::ScreenshotTool { tool },
            niri_ipc::Action::ScreenshotTogglePalette {} => Self::ScreenshotTogglePalette,
            niri_ipc::Action::ScreenshotSetPalette { open } => Self::ScreenshotSetPalette { open },
            niri_ipc::Action::ScreenshotPaletteColor { index } => {
                Self::ScreenshotPaletteColor { index }
            }
            niri_ipc::Action::ScreenshotBrushColor { color } => {
                Self::ScreenshotBrushColor { color }
            }
            niri_ipc::Action::ScreenshotContext { change } => Self::ScreenshotContext { change },
            niri_ipc::Action::ScreenshotArrowColor { color } => {
                Self::ScreenshotArrowColor { color }
            }
            niri_ipc::Action::ScreenshotRectangleColor { color } => {
                Self::ScreenshotRectangleColor { color }
            }
            niri_ipc::Action::ScreenshotUndo {} => Self::ScreenshotUndo,
            niri_ipc::Action::ScreenshotRedo {} => Self::ScreenshotRedo,
            niri_ipc::Action::ScreenshotTogglePointer {} => Self::ScreenshotTogglePointer,
            niri_ipc::Action::ScreenshotSetPointer { show_pointer } => {
                Self::ScreenshotSetPointer { show_pointer }
            }
            niri_ipc::Action::ScreenshotToggleAxes {} => Self::ScreenshotToggleAxes(false),
            niri_ipc::Action::ScreenshotSetAxes { enabled } => Self::ScreenshotSetAxes { enabled },
            niri_ipc::Action::ScreenshotMoveSelection { x, y } => {
                Self::ScreenshotMoveSelection { x, y }
            }
            niri_ipc::Action::ScreenshotResizeSelection { width, height } => {
                Self::ScreenshotResizeSelection { width, height }
            }
            niri_ipc::Action::ScreenshotMagnifierZoom { zoom } => {
                Self::ScreenshotMagnifierZoom { zoom }
            }
            niri_ipc::Action::ScreenshotFocusNext {} => Self::ScreenshotFocusNext,
            niri_ipc::Action::ScreenshotFocusPrevious {} => Self::ScreenshotFocusPrevious,
            niri_ipc::Action::ScreenshotConfirm {} => Self::ScreenshotConfirm,
            niri_ipc::Action::ScreenshotCancel {} => Self::ScreenshotCancel,
            niri_ipc::Action::ConfirmScreenshot {
                write_to_disk,
                format,
                quality,
            } => Self::ConfirmScreenshot {
                write_to_disk,
                format,
                quality,
            },
            niri_ipc::Action::CancelScreenshot {} => Self::CancelScreenshot,
            niri_ipc::Action::ToggleKeyboardShortcutsInhibit {} => {
                Self::ToggleKeyboardShortcutsInhibit
            }
            niri_ipc::Action::CloseWindow { id: None } => Self::CloseWindow,
            niri_ipc::Action::CloseWindow { id: Some(id) } => Self::CloseWindowById(id),
            niri_ipc::Action::FullscreenWindow { id: None } => Self::FullscreenWindow,
            niri_ipc::Action::FullscreenWindow { id: Some(id) } => Self::FullscreenWindowById(id),
            niri_ipc::Action::ToggleWindowedFullscreen { id: None } => {
                Self::ToggleWindowedFullscreen
            }
            niri_ipc::Action::ToggleWindowedFullscreen { id: Some(id) } => {
                Self::ToggleWindowedFullscreenById(id)
            }
            niri_ipc::Action::FocusWindow { id } => Self::FocusWindow(id),
            niri_ipc::Action::FocusWindowInColumn { index } => Self::FocusWindowInColumn(index),
            niri_ipc::Action::FocusWindowPrevious {} => Self::FocusWindowPrevious,
            niri_ipc::Action::FocusColumnLeft {} => Self::FocusColumnLeft,
            niri_ipc::Action::FocusColumnRight {} => Self::FocusColumnRight,
            niri_ipc::Action::FocusColumnFirst {} => Self::FocusColumnFirst,
            niri_ipc::Action::FocusColumnLast {} => Self::FocusColumnLast,
            niri_ipc::Action::FocusColumnRightOrFirst {} => Self::FocusColumnRightOrFirst,
            niri_ipc::Action::FocusColumnLeftOrLast {} => Self::FocusColumnLeftOrLast,
            niri_ipc::Action::FocusColumn { index } => Self::FocusColumn(index),
            niri_ipc::Action::FocusWindowOrMonitorUp {} => Self::FocusWindowOrMonitorUp,
            niri_ipc::Action::FocusWindowOrMonitorDown {} => Self::FocusWindowOrMonitorDown,
            niri_ipc::Action::FocusColumnOrMonitorLeft {} => Self::FocusColumnOrMonitorLeft,
            niri_ipc::Action::FocusColumnOrMonitorRight {} => Self::FocusColumnOrMonitorRight,
            niri_ipc::Action::FocusWindowDown {} => Self::FocusWindowDown,
            niri_ipc::Action::FocusWindowUp {} => Self::FocusWindowUp,
            niri_ipc::Action::FocusWindowDownOrColumnLeft {} => Self::FocusWindowDownOrColumnLeft,
            niri_ipc::Action::FocusWindowDownOrColumnRight {} => Self::FocusWindowDownOrColumnRight,
            niri_ipc::Action::FocusWindowUpOrColumnLeft {} => Self::FocusWindowUpOrColumnLeft,
            niri_ipc::Action::FocusWindowUpOrColumnRight {} => Self::FocusWindowUpOrColumnRight,
            niri_ipc::Action::FocusWindowOrWorkspaceDown {} => Self::FocusWindowOrWorkspaceDown,
            niri_ipc::Action::FocusWindowOrWorkspaceUp {} => Self::FocusWindowOrWorkspaceUp,
            niri_ipc::Action::FocusWindowTop {} => Self::FocusWindowTop,
            niri_ipc::Action::FocusWindowBottom {} => Self::FocusWindowBottom,
            niri_ipc::Action::FocusWindowDownOrTop {} => Self::FocusWindowDownOrTop,
            niri_ipc::Action::FocusWindowUpOrBottom {} => Self::FocusWindowUpOrBottom,
            niri_ipc::Action::MoveColumnLeft {} => Self::MoveColumnLeft,
            niri_ipc::Action::MoveColumnRight {} => Self::MoveColumnRight,
            niri_ipc::Action::MoveColumnToFirst {} => Self::MoveColumnToFirst,
            niri_ipc::Action::MoveColumnToLast {} => Self::MoveColumnToLast,
            niri_ipc::Action::MoveColumnToIndex { index } => Self::MoveColumnToIndex(index),
            niri_ipc::Action::MoveColumnLeftOrToMonitorLeft {} => {
                Self::MoveColumnLeftOrToMonitorLeft
            }
            niri_ipc::Action::MoveColumnRightOrToMonitorRight {} => {
                Self::MoveColumnRightOrToMonitorRight
            }
            niri_ipc::Action::MoveWindowDown {} => Self::MoveWindowDown,
            niri_ipc::Action::MoveWindowUp {} => Self::MoveWindowUp,
            niri_ipc::Action::MoveWindowDownOrToWorkspaceDown {} => {
                Self::MoveWindowDownOrToWorkspaceDown
            }
            niri_ipc::Action::MoveWindowUpOrToWorkspaceUp {} => Self::MoveWindowUpOrToWorkspaceUp,
            niri_ipc::Action::ConsumeOrExpelWindowLeft { id: None } => {
                Self::ConsumeOrExpelWindowLeft
            }
            niri_ipc::Action::ConsumeOrExpelWindowLeft { id: Some(id) } => {
                Self::ConsumeOrExpelWindowLeftById(id)
            }
            niri_ipc::Action::ConsumeOrExpelWindowRight { id: None } => {
                Self::ConsumeOrExpelWindowRight
            }
            niri_ipc::Action::ConsumeOrExpelWindowRight { id: Some(id) } => {
                Self::ConsumeOrExpelWindowRightById(id)
            }
            niri_ipc::Action::ConsumeWindowIntoColumn {} => Self::ConsumeWindowIntoColumn,
            niri_ipc::Action::ExpelWindowFromColumn {} => Self::ExpelWindowFromColumn,
            niri_ipc::Action::SwapWindowRight {} => Self::SwapWindowRight,
            niri_ipc::Action::SwapWindowLeft {} => Self::SwapWindowLeft,
            niri_ipc::Action::ToggleColumnTabbedDisplay {} => Self::ToggleColumnTabbedDisplay,
            niri_ipc::Action::SetColumnDisplay { display } => Self::SetColumnDisplay(display),
            niri_ipc::Action::CenterColumn {} => Self::CenterColumn,
            niri_ipc::Action::CenterWindow { id: None } => Self::CenterWindow,
            niri_ipc::Action::CenterWindow { id: Some(id) } => Self::CenterWindowById(id),
            niri_ipc::Action::CenterVisibleColumns {} => Self::CenterVisibleColumns,
            niri_ipc::Action::FocusWorkspaceDown {} => Self::FocusWorkspaceDown,
            niri_ipc::Action::FocusWorkspaceUp {} => Self::FocusWorkspaceUp,
            niri_ipc::Action::FocusWorkspace { reference } => {
                Self::FocusWorkspace(WorkspaceReference::from(reference))
            }
            niri_ipc::Action::FocusWorkspacePrevious {} => Self::FocusWorkspacePrevious,
            niri_ipc::Action::MoveWindowToWorkspaceDown { focus } => {
                Self::MoveWindowToWorkspaceDown(focus)
            }
            niri_ipc::Action::MoveWindowToWorkspaceUp { focus } => {
                Self::MoveWindowToWorkspaceUp(focus)
            }
            niri_ipc::Action::MoveWindowToWorkspace {
                window_id: None,
                reference,
                focus,
            } => Self::MoveWindowToWorkspace(WorkspaceReference::from(reference), focus),
            niri_ipc::Action::MoveWindowToWorkspace {
                window_id: Some(window_id),
                reference,
                focus,
            } => Self::MoveWindowToWorkspaceById {
                window_id,
                reference: WorkspaceReference::from(reference),
                focus,
            },
            niri_ipc::Action::MoveColumnToWorkspaceDown { focus } => {
                Self::MoveColumnToWorkspaceDown(focus)
            }
            niri_ipc::Action::MoveColumnToWorkspaceUp { focus } => {
                Self::MoveColumnToWorkspaceUp(focus)
            }
            niri_ipc::Action::MoveColumnToWorkspace { reference, focus } => {
                Self::MoveColumnToWorkspace(WorkspaceReference::from(reference), focus)
            }
            niri_ipc::Action::MoveWorkspaceDown {} => Self::MoveWorkspaceDown,
            niri_ipc::Action::MoveWorkspaceUp {} => Self::MoveWorkspaceUp,
            niri_ipc::Action::SetWorkspaceName {
                name,
                workspace: None,
            } => Self::SetWorkspaceName(name),
            niri_ipc::Action::SetWorkspaceName {
                name,
                workspace: Some(reference),
            } => Self::SetWorkspaceNameByRef {
                name,
                reference: WorkspaceReference::from(reference),
            },
            niri_ipc::Action::UnsetWorkspaceName { reference: None } => Self::UnsetWorkspaceName,
            niri_ipc::Action::UnsetWorkspaceName {
                reference: Some(reference),
            } => Self::UnsetWorkSpaceNameByRef(WorkspaceReference::from(reference)),
            niri_ipc::Action::FocusMonitorLeft {} => Self::FocusMonitorLeft,
            niri_ipc::Action::FocusMonitorRight {} => Self::FocusMonitorRight,
            niri_ipc::Action::FocusMonitorDown {} => Self::FocusMonitorDown,
            niri_ipc::Action::FocusMonitorUp {} => Self::FocusMonitorUp,
            niri_ipc::Action::FocusMonitorPrevious {} => Self::FocusMonitorPrevious,
            niri_ipc::Action::FocusMonitorNext {} => Self::FocusMonitorNext,
            niri_ipc::Action::FocusMonitor { output } => Self::FocusMonitor(output),
            niri_ipc::Action::MoveWindowToMonitorLeft {} => Self::MoveWindowToMonitorLeft,
            niri_ipc::Action::MoveWindowToMonitorRight {} => Self::MoveWindowToMonitorRight,
            niri_ipc::Action::MoveWindowToMonitorDown {} => Self::MoveWindowToMonitorDown,
            niri_ipc::Action::MoveWindowToMonitorUp {} => Self::MoveWindowToMonitorUp,
            niri_ipc::Action::MoveWindowToMonitorPrevious {} => Self::MoveWindowToMonitorPrevious,
            niri_ipc::Action::MoveWindowToMonitorNext {} => Self::MoveWindowToMonitorNext,
            niri_ipc::Action::MoveWindowToMonitor { id: None, output } => {
                Self::MoveWindowToMonitor(output)
            }
            niri_ipc::Action::MoveWindowToMonitor {
                id: Some(id),
                output,
            } => Self::MoveWindowToMonitorById { id, output },
            niri_ipc::Action::MoveColumnToMonitorLeft {} => Self::MoveColumnToMonitorLeft,
            niri_ipc::Action::MoveColumnToMonitorRight {} => Self::MoveColumnToMonitorRight,
            niri_ipc::Action::MoveColumnToMonitorDown {} => Self::MoveColumnToMonitorDown,
            niri_ipc::Action::MoveColumnToMonitorUp {} => Self::MoveColumnToMonitorUp,
            niri_ipc::Action::MoveColumnToMonitorPrevious {} => Self::MoveColumnToMonitorPrevious,
            niri_ipc::Action::MoveColumnToMonitorNext {} => Self::MoveColumnToMonitorNext,
            niri_ipc::Action::MoveColumnToMonitor { output } => Self::MoveColumnToMonitor(output),
            niri_ipc::Action::SetWindowWidth { id: None, change } => Self::SetWindowWidth(change),
            niri_ipc::Action::SetWindowWidth {
                id: Some(id),
                change,
            } => Self::SetWindowWidthById { id, change },
            niri_ipc::Action::SetWindowHeight { id: None, change } => Self::SetWindowHeight(change),
            niri_ipc::Action::SetWindowHeight {
                id: Some(id),
                change,
            } => Self::SetWindowHeightById { id, change },
            niri_ipc::Action::ResetWindowHeight { id: None } => Self::ResetWindowHeight,
            niri_ipc::Action::ResetWindowHeight { id: Some(id) } => Self::ResetWindowHeightById(id),
            niri_ipc::Action::SwitchPresetColumnWidth {} => Self::SwitchPresetColumnWidth,
            niri_ipc::Action::SwitchPresetColumnWidthBack {} => Self::SwitchPresetColumnWidthBack,
            niri_ipc::Action::SwitchPresetWindowWidth { id: None } => Self::SwitchPresetWindowWidth,
            niri_ipc::Action::SwitchPresetWindowWidthBack { id: None } => {
                Self::SwitchPresetWindowWidthBack
            }
            niri_ipc::Action::SwitchPresetWindowWidth { id: Some(id) } => {
                Self::SwitchPresetWindowWidthById(id)
            }
            niri_ipc::Action::SwitchPresetWindowWidthBack { id: Some(id) } => {
                Self::SwitchPresetWindowWidthBackById(id)
            }
            niri_ipc::Action::SwitchPresetWindowHeight { id: None } => {
                Self::SwitchPresetWindowHeight
            }
            niri_ipc::Action::SwitchPresetWindowHeightBack { id: None } => {
                Self::SwitchPresetWindowHeightBack
            }
            niri_ipc::Action::SwitchPresetWindowHeight { id: Some(id) } => {
                Self::SwitchPresetWindowHeightById(id)
            }
            niri_ipc::Action::SwitchPresetWindowHeightBack { id: Some(id) } => {
                Self::SwitchPresetWindowHeightBackById(id)
            }
            niri_ipc::Action::MaximizeColumn {} => Self::MaximizeColumn,
            niri_ipc::Action::MaximizeWindowToEdges { id: None } => Self::MaximizeWindowToEdges,
            niri_ipc::Action::MaximizeWindowToEdges { id: Some(id) } => {
                Self::MaximizeWindowToEdgesById(id)
            }
            niri_ipc::Action::SetColumnWidth { change } => Self::SetColumnWidth(change),
            niri_ipc::Action::ExpandColumnToAvailableWidth {} => Self::ExpandColumnToAvailableWidth,
            niri_ipc::Action::SwitchLayout { layout } => Self::SwitchLayout(layout),
            niri_ipc::Action::ShowHotkeyOverlay {} => Self::ShowHotkeyOverlay,
            niri_ipc::Action::MoveWorkspaceToMonitorLeft {} => Self::MoveWorkspaceToMonitorLeft,
            niri_ipc::Action::MoveWorkspaceToMonitorRight {} => Self::MoveWorkspaceToMonitorRight,
            niri_ipc::Action::MoveWorkspaceToMonitorDown {} => Self::MoveWorkspaceToMonitorDown,
            niri_ipc::Action::MoveWorkspaceToMonitorUp {} => Self::MoveWorkspaceToMonitorUp,
            niri_ipc::Action::MoveWorkspaceToMonitorPrevious {} => {
                Self::MoveWorkspaceToMonitorPrevious
            }
            niri_ipc::Action::MoveWorkspaceToIndex {
                index,
                reference: Some(reference),
            } => Self::MoveWorkspaceToIndexByRef {
                new_idx: index,
                reference: WorkspaceReference::from(reference),
            },
            niri_ipc::Action::MoveWorkspaceToIndex {
                index,
                reference: None,
            } => Self::MoveWorkspaceToIndex(index),
            niri_ipc::Action::MoveWorkspaceToMonitor {
                output,
                reference: Some(reference),
            } => Self::MoveWorkspaceToMonitorByRef {
                output_name: output,
                reference: WorkspaceReference::from(reference),
            },
            niri_ipc::Action::MoveWorkspaceToMonitor {
                output,
                reference: None,
            } => Self::MoveWorkspaceToMonitor(output),
            niri_ipc::Action::MoveWorkspaceToMonitorNext {} => Self::MoveWorkspaceToMonitorNext,
            niri_ipc::Action::ToggleDebugTint {} => Self::ToggleDebugTint,
            niri_ipc::Action::DebugToggleOpaqueRegions {} => Self::DebugToggleOpaqueRegions,
            niri_ipc::Action::DebugToggleDamage {} => Self::DebugToggleDamage,
            niri_ipc::Action::ToggleWindowFloating { id: None } => Self::ToggleWindowFloating,
            niri_ipc::Action::ToggleWindowFloating { id: Some(id) } => {
                Self::ToggleWindowFloatingById(id)
            }
            niri_ipc::Action::MoveWindowToFloating { id: None } => Self::MoveWindowToFloating,
            niri_ipc::Action::MoveWindowToFloating { id: Some(id) } => {
                Self::MoveWindowToFloatingById(id)
            }
            niri_ipc::Action::MoveWindowToTiling { id: None } => Self::MoveWindowToTiling,
            niri_ipc::Action::MoveWindowToTiling { id: Some(id) } => {
                Self::MoveWindowToTilingById(id)
            }
            niri_ipc::Action::FocusFloating {} => Self::FocusFloating,
            niri_ipc::Action::FocusTiling {} => Self::FocusTiling,
            niri_ipc::Action::SwitchFocusBetweenFloatingAndTiling {} => {
                Self::SwitchFocusBetweenFloatingAndTiling
            }
            niri_ipc::Action::MoveFloatingWindow { id, x, y } => {
                Self::MoveFloatingWindowById { id, x, y }
            }
            niri_ipc::Action::ToggleWindowRuleOpacity { id: None } => Self::ToggleWindowRuleOpacity,
            niri_ipc::Action::ToggleWindowRuleOpacity { id: Some(id) } => {
                Self::ToggleWindowRuleOpacityById(id)
            }
            niri_ipc::Action::SetDynamicCastWindow { id: None } => Self::SetDynamicCastWindow,
            niri_ipc::Action::SetDynamicCastWindow { id: Some(id) } => {
                Self::SetDynamicCastWindowById(id)
            }
            niri_ipc::Action::SetDynamicCastMonitor { output } => {
                Self::SetDynamicCastMonitor(output)
            }
            niri_ipc::Action::ClearDynamicCastTarget {} => Self::ClearDynamicCastTarget,
            niri_ipc::Action::StopCast { session_id } => Self::StopCast(session_id),
            niri_ipc::Action::ToggleOverview {} => Self::ToggleOverview,
            niri_ipc::Action::OpenOverview {} => Self::OpenOverview,
            niri_ipc::Action::CloseOverview {} => Self::CloseOverview,
            niri_ipc::Action::ToggleWindowUrgent { id } => Self::ToggleWindowUrgent(id),
            niri_ipc::Action::SetWindowUrgent { id } => Self::SetWindowUrgent(id),
            niri_ipc::Action::UnsetWindowUrgent { id } => Self::UnsetWindowUrgent(id),
            niri_ipc::Action::LoadConfigFile { path } => Self::LoadConfigFile(path),
            niri_ipc::Action::ZoomIn {} => Self::ZoomIn,
            niri_ipc::Action::ZoomOut {} => Self::ZoomOut,
            niri_ipc::Action::SetZoomLevel { level } => Self::SetZoomLevel(FloatOrInt(level)),
            niri_ipc::Action::ResetZoom {} => Self::ResetZoom,
            niri_ipc::Action::ZoomLock {} => Self::ZoomLock(false),
            niri_ipc::Action::Zoom { level, lock } => {
                Self::Zoom(ZoomLevelPreset(level), false, lock)
            }
        }
    }
}

impl Action {
    /// Whether the action only makes sense while the screenshot editor is open.
    ///
    /// Editor-local actions form their own keybind category: they are excluded from bind
    /// matching while the UI is closed (so they don't swallow client key presses) and take
    /// priority over ordinary binds while it is open. `Binds` decoding allows the same
    /// `Key` once in each category, but rejects duplicates within one category.
    ///
    /// The opening `screenshot` action is ordinary: it must work while the UI is closed.
    pub fn is_screenshot_editor_local(&self) -> bool {
        matches!(
            self,
            Self::ScreenshotTool { .. }
                | Self::ScreenshotTogglePalette
                | Self::ScreenshotSetPalette { .. }
                | Self::ScreenshotPaletteColor { .. }
                | Self::ScreenshotBrushColor { .. }
                | Self::ScreenshotContext { .. }
                | Self::ScreenshotArrowColor { .. }
                | Self::ScreenshotRectangleColor { .. }
                | Self::ScreenshotUndo
                | Self::ScreenshotRedo
                | Self::ScreenshotTogglePointer
                | Self::ScreenshotSetPointer { .. }
                | Self::ScreenshotToggleAxes(_)
                | Self::ScreenshotSetAxes { .. }
                | Self::ScreenshotMoveSelection { .. }
                | Self::ScreenshotResizeSelection { .. }
                | Self::ScreenshotMoveSelectionHold
                | Self::ScreenshotMagnifierZoom { .. }
                | Self::ScreenshotFocusNext
                | Self::ScreenshotFocusPrevious
                | Self::ScreenshotConfirm
                | Self::ScreenshotCancel
                | Self::ConfirmScreenshot { .. }
                | Self::CancelScreenshot
        )
    }
}
/// Decodes a screenshot action node whose payload needs named arguments.
///
/// `knuffel`'s derive cannot decode enum variants with named fields, so the payload
/// screenshot actions are `#[knuffel(skip)]` on [`Action`] and decoded here by hand.
/// Returns `None` for node names this doesn't handle so the caller falls back to the
/// derived decode.
fn decode_screenshot_action<S: knuffel::traits::ErrorSpan>(
    node: &knuffel::ast::SpannedNode<S>,
    ctx: &mut knuffel::decode::Context<S>,
) -> Option<Result<Action, DecodeError<S>>> {
    let name = &**node.node_name;
    if !matches!(
        name,
        "screenshot"
            | "screenshot-tool"
            | "screenshot-set-palette"
            | "screenshot-palette-color"
            | "screenshot-brush-color"
            | "screenshot-context"
            | "screenshot-arrow-color"
            | "screenshot-rectangle-color"
            | "screenshot-set-pointer"
            | "screenshot-set-axes"
            | "screenshot-move-selection"
            | "screenshot-resize-selection"
            | "screenshot-magnifier-zoom"
            | "confirm-screenshot"
    ) {
        return None;
    }

    Some(decode_screenshot_action_inner(name, node, ctx))
}

fn decode_screenshot_action_inner<S: knuffel::traits::ErrorSpan>(
    name: &str,
    node: &knuffel::ast::SpannedNode<S>,
    ctx: &mut knuffel::decode::Context<S>,
) -> Result<Action, DecodeError<S>> {
    if let Some(type_name) = &node.type_name {
        ctx.emit_error(DecodeError::unexpected(
            type_name,
            "type name",
            "no type name expected for this node",
        ));
    }
    for child in node.children() {
        ctx.emit_error(DecodeError::unexpected(
            child,
            "node",
            format!("unexpected node `{}`", child.node_name.escape_default()),
        ));
    }

    /// Reads one optional property and reports any property that wasn't claimed.
    struct Props<'a, S> {
        node: &'a knuffel::ast::SpannedNode<S>,
        claimed: std::collections::HashSet<&'a str>,
    }

    impl<'a, S: knuffel::traits::ErrorSpan> Props<'a, S> {
        fn new(node: &'a knuffel::ast::SpannedNode<S>) -> Self {
            Self {
                node,
                claimed: Default::default(),
            }
        }

        fn get(&mut self, name: &'a str) -> Option<&'a knuffel::ast::Value<S>> {
            let entry = self
                .node
                .properties
                .iter()
                .find(|(key, _)| &****key == name)?;
            self.claimed.insert(name);
            Some(entry.1)
        }

        fn finish(self, ctx: &mut knuffel::decode::Context<S>) {
            for name in self.node.properties.keys() {
                if !self.claimed.contains(&***name) {
                    ctx.emit_error(DecodeError::unexpected(
                        name,
                        "property",
                        format!("unexpected property `{}`", name.escape_default()),
                    ));
                }
            }
        }
    }

    /// Decodes a property value through `DecodeScalar`, or returns `default` when absent.
    fn prop_or<S: knuffel::traits::ErrorSpan, T: knuffel::traits::DecodeScalar<S>>(
        props: &mut Props<S>,
        name: &'static str,
        default: T,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> T {
        match props.get(name) {
            Some(val) => match knuffel::traits::DecodeScalar::decode(val, ctx) {
                Ok(v) => v,
                Err(e) => {
                    ctx.emit_error(e);
                    default
                }
            },
            None => default,
        }
    }

    /// Decodes a property string value through `FromStr`, or returns `default` when absent.
    fn str_prop_or<S, T>(
        props: &mut Props<S>,
        name: &'static str,
        default: T,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> T
    where
        S: knuffel::traits::ErrorSpan,
        T: FromStr,
        T::Err: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        match props.get(name) {
            Some(val) => {
                let s: String = match knuffel::traits::DecodeScalar::decode(val, ctx) {
                    Ok(s) => s,
                    Err(e) => {
                        ctx.emit_error(e);
                        return default;
                    }
                };
                match s.parse() {
                    Ok(v) => v,
                    Err(e) => {
                        ctx.emit_error(DecodeError::conversion(&val.literal, e));
                        default
                    }
                }
            }
            None => default,
        }
    }

    /// Decodes an optional property string value through `FromStr`, `None` when absent.
    fn opt_str_prop<S, T>(
        props: &mut Props<S>,
        name: &'static str,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Option<T>
    where
        S: knuffel::traits::ErrorSpan,
        T: FromStr,
        T::Err: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        let val = props.get(name)?;
        let s: String = match knuffel::traits::DecodeScalar::decode(val, ctx) {
            Ok(s) => s,
            Err(e) => {
                ctx.emit_error(e);
                return None;
            }
        };
        match s.parse() {
            Ok(v) => Some(v),
            Err(e) => {
                ctx.emit_error(DecodeError::conversion(&val.literal, e));
                None
            }
        }
    }

    /// Reports any leftover arguments and all properties of an argument-taking node.
    fn reject_extras<S: knuffel::traits::ErrorSpan>(
        mut iter_args: std::slice::Iter<'_, knuffel::ast::Value<S>>,
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(val) = iter_args.next() {
            ctx.emit_error(DecodeError::unexpected(
                &val.literal,
                "argument",
                "unexpected argument",
            ));
        }
        for name in node.properties.keys() {
            ctx.emit_error(DecodeError::unexpected(
                name,
                "property",
                format!("unexpected property `{}`", name.escape_default()),
            ));
        }
    }

    /// Takes the single argument of an action.
    fn single_arg<'a, S: knuffel::traits::ErrorSpan>(
        name: &str,
        node: &'a knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<&'a knuffel::ast::Value<S>, DecodeError<S>> {
        let mut iter_args = node.arguments.iter();
        let val = iter_args.next().ok_or_else(|| {
            DecodeError::missing(node, format!("additional argument `{name}` is required"))
        })?;
        reject_extras(iter_args, node, ctx);
        Ok(val)
    }

    /// Decodes the single argument of an action through `DecodeScalar`.
    fn arg<S: knuffel::traits::ErrorSpan, T: knuffel::traits::DecodeScalar<S>>(
        name: &str,
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<T, DecodeError<S>> {
        let val = single_arg(name, node, ctx)?;
        knuffel::traits::DecodeScalar::decode(val, ctx)
    }

    /// Decodes the single `FromStr` string argument of an action.
    fn str_arg<S, T>(
        name: &str,
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<T, DecodeError<S>>
    where
        S: knuffel::traits::ErrorSpan,
        T: FromStr,
        T::Err: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        let val = single_arg(name, node, ctx)?;
        let s: String = knuffel::traits::DecodeScalar::decode(val, ctx)?;
        s.parse()
            .map_err(|e| DecodeError::conversion(&val.literal, e))
    }

    /// Decodes the single argument of an action, which may be a KDL string passed through
    /// `FromStr` (a signed adjustment) or a bare integer (an absolute value).
    fn value_change_arg<S>(
        name: &str,
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<ScreenshotValueChange, DecodeError<S>>
    where
        S: knuffel::traits::ErrorSpan,
    {
        let val = single_arg(name, node, ctx)?;

        if let Some(type_name) = &val.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        let value = match &*val.literal {
            // A bare number is an absolute value.
            knuffel::ast::Literal::Int(int) => match u32::try_from(int) {
                Ok(v) => ScreenshotValueChange::Set(v),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(&val.literal, e));
                    ScreenshotValueChange::Set(0)
                }
            },
            // A quoted, signed string is an adjustment.
            knuffel::ast::Literal::String(s) => match s.parse() {
                Ok(v) => v,
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(&val.literal, e));
                    ScreenshotValueChange::Set(0)
                }
            },
            _ => {
                ctx.emit_error(DecodeError::scalar_kind(
                    knuffel::decode::Kind::Int,
                    &val.literal,
                ));
                ScreenshotValueChange::Set(0)
            }
        };

        Ok(value)
    }

    let action = match name {
        "screenshot" => {
            for val in &node.arguments {
                ctx.emit_error(DecodeError::unexpected(
                    &val.literal,
                    "argument",
                    "no arguments expected for this node",
                ));
            }
            let mut props = Props::new(node);
            let show_pointer = prop_or(&mut props, "show-pointer", true, ctx);
            let tool = str_prop_or(&mut props, "tool", ScreenshotTool::Select, ctx);
            props.finish(ctx);
            Action::Screenshot {
                show_pointer,
                path: None,
                tool,
            }
        }
        "screenshot-tool" => Action::ScreenshotTool {
            tool: str_arg("tool", node, ctx)?,
        },
        "screenshot-set-palette" => Action::ScreenshotSetPalette {
            open: arg("open", node, ctx)?,
        },
        "screenshot-palette-color" => {
            let index: u8 = arg("index", node, ctx)?;
            if !(1..=7).contains(&index) {
                ctx.emit_error(DecodeError::conversion(
                    &node.arguments[0].literal,
                    "palette color index must be between 1 and 7",
                ));
            }
            Action::ScreenshotPaletteColor { index }
        }
        "screenshot-brush-color" => Action::ScreenshotBrushColor {
            color: str_arg("color", node, ctx)?,
        },
        "screenshot-context" => {
            let change = value_change_arg("change", node, ctx)?;
            // The pointer context is not known at parse time, so absolute values accept the
            // full stroke-width range; each target clamps to its own range at runtime.
            if let ScreenshotValueChange::Set(change) = change {
                if !(MIN_SCREENSHOT_BRUSH_WIDTH..=MAX_SCREENSHOT_BRUSH_WIDTH).contains(&change) {
                    ctx.emit_error(DecodeError::conversion(
                        &node.arguments[0].literal,
                        format!(
                            "screenshot context value must be between \
                             {MIN_SCREENSHOT_BRUSH_WIDTH} and {MAX_SCREENSHOT_BRUSH_WIDTH}"
                        ),
                    ));
                }
            }
            Action::ScreenshotContext { change }
        }
        "screenshot-arrow-color" => Action::ScreenshotArrowColor {
            color: str_arg("color", node, ctx)?,
        },
        "screenshot-rectangle-color" => Action::ScreenshotRectangleColor {
            color: str_arg("color", node, ctx)?,
        },
        "screenshot-set-axes" => Action::ScreenshotSetAxes {
            enabled: arg("enabled", node, ctx)?,
        },
        "screenshot-set-pointer" => Action::ScreenshotSetPointer {
            show_pointer: arg("show-pointer", node, ctx)?,
        },
        "screenshot-move-selection" => {
            for val in &node.arguments {
                ctx.emit_error(DecodeError::unexpected(
                    &val.literal,
                    "argument",
                    "no arguments expected for this node",
                ));
            }
            let mut props = Props::new(node);
            let x = prop_or(&mut props, "x", 0i32, ctx);
            let y = prop_or(&mut props, "y", 0i32, ctx);
            props.finish(ctx);
            Action::ScreenshotMoveSelection { x, y }
        }
        "screenshot-resize-selection" => {
            for val in &node.arguments {
                ctx.emit_error(DecodeError::unexpected(
                    &val.literal,
                    "argument",
                    "no arguments expected for this node",
                ));
            }
            let mut props = Props::new(node);
            let width = prop_or(&mut props, "width", 0i32, ctx);
            let height = prop_or(&mut props, "height", 0i32, ctx);
            props.finish(ctx);
            Action::ScreenshotResizeSelection { width, height }
        }
        "screenshot-magnifier-zoom" => {
            let zoom = value_change_arg("zoom", node, ctx)?;
            if let ScreenshotValueChange::Set(zoom) = zoom {
                if !matches!(zoom, 1 | 2 | 4 | 8 | 16 | 32) {
                    ctx.emit_error(DecodeError::conversion(
                        &node.arguments[0].literal,
                        "magnifier zoom must be one of 1, 2, 4, 8, 16, 32",
                    ));
                }
            }
            Action::ScreenshotMagnifierZoom { zoom }
        }
        "confirm-screenshot" => {
            for val in &node.arguments {
                ctx.emit_error(DecodeError::unexpected(
                    &val.literal,
                    "argument",
                    "no arguments expected for this node",
                ));
            }
            let mut props = Props::new(node);
            let write_to_disk = prop_or(&mut props, "write-to-disk", true, ctx);
            let format = opt_str_prop(&mut props, "format", ctx);
            let quality: Option<crate::FloatOrInt<0, 100>> =
                prop_or(&mut props, "quality", None, ctx);
            let quality = quality.map(|value| value.0 as f32);
            if write_to_disk
                && format == Some(ScreenshotFormat::Avif)
                && quality.is_some_and(|q| q < 1.)
            {
                ctx.emit_error(DecodeError::unexpected(
                    &node.node_name,
                    "quality",
                    "AVIF quality must be between 1 and 100",
                ));
            }
            props.finish(ctx);
            Action::ConfirmScreenshot {
                write_to_disk,
                format,
                quality,
            }
        }
        _ => unreachable!("checked above"),
    };

    Ok(action)
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WorkspaceReference {
    Id(u64),
    Index(u8),
    Name(String),
}

impl From<WorkspaceReferenceArg> for WorkspaceReference {
    fn from(reference: WorkspaceReferenceArg) -> WorkspaceReference {
        match reference {
            WorkspaceReferenceArg::Id(id) => Self::Id(id),
            WorkspaceReferenceArg::Index(i) => Self::Index(i),
            WorkspaceReferenceArg::Name(n) => Self::Name(n),
        }
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for WorkspaceReference {
    fn type_check(
        type_name: &Option<knuffel::span::Spanned<knuffel::ast::TypeName, S>>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(type_name) = &type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }
    }

    fn raw_decode(
        val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<WorkspaceReference, DecodeError<S>> {
        match &**val {
            knuffel::ast::Literal::String(ref s) => Ok(WorkspaceReference::Name(s.clone().into())),
            knuffel::ast::Literal::Int(ref value) => match value.try_into() {
                Ok(v) => Ok(WorkspaceReference::Index(v)),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    Ok(WorkspaceReference::Index(0))
                }
            },
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only numbers and strings are recognized",
                ));
                Ok(WorkspaceReference::Index(0))
            }
        }
    }
}

impl<S> knuffel::Decode<S> for Binds
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        expect_only_children(node, ctx);

        // Duplicate keys are tracked per bind category so that an ordinary bind and an
        // editor-local screenshot bind may share a trigger: only the one matching the
        // current UI state is eligible, so they never compete.
        let mut seen_keys: HashMap<(Key, bool), &knuffel::ast::SpannedNode<S>> = HashMap::new();

        let mut binds = Vec::new();

        for child in node.children() {
            match Bind::decode_node(child, ctx) {
                Err(e) => {
                    ctx.emit_error(e);
                }
                Ok(bind) => {
                    match seen_keys.entry((bind.key, bind.action.is_screenshot_editor_local())) {
                        Entry::Occupied(entry) => {
                            // Even though it's technically incorrect, we use
                            // `DecodeError::Missing` here because it labels the bind with
                            // "node starts here", which is the least bad option
                            ctx.emit_error(DecodeError::missing(
                                entry.get(),
                                "keybind first defined here",
                            ));

                            ctx.emit_error(DecodeError::unexpected(
                                &child.node_name,
                                "keybind",
                                "duplicate keybind later defined here",
                            ));
                        }
                        Entry::Vacant(entry) => {
                            entry.insert(child);
                            binds.push(bind);
                        }
                    }
                }
            }
        }

        Ok(Self(binds))
    }
}

impl<S> knuffel::Decode<S> for Bind
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        if let Some(type_name) = &node.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        for val in node.arguments.iter() {
            ctx.emit_error(DecodeError::unexpected(
                &val.literal,
                "argument",
                "no arguments expected for this node",
            ));
        }

        let key = node
            .node_name
            .parse::<Key>()
            .map_err(|e| DecodeError::conversion(&node.node_name, e.wrap_err("invalid keybind")))?;

        let mut repeat = true;
        let mut cooldown = None;
        let mut allow_when_locked = false;
        let mut allow_when_locked_node = None;
        let mut allow_inhibiting = true;
        let mut hotkey_overlay_title = None;
        for (name, val) in &node.properties {
            match &***name {
                "repeat" => {
                    repeat = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "cooldown-ms" => {
                    cooldown = Some(Duration::from_millis(
                        knuffel::traits::DecodeScalar::decode(val, ctx)?,
                    ));
                }
                "allow-when-locked" => {
                    allow_when_locked = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    allow_when_locked_node = Some(name);
                }
                "allow-inhibiting" => {
                    allow_inhibiting = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "hotkey-overlay-title" => {
                    hotkey_overlay_title = Some(knuffel::traits::DecodeScalar::decode(val, ctx)?);
                }
                name_str => {
                    ctx.emit_error(DecodeError::unexpected(
                        name,
                        "property",
                        format!("unexpected property `{}`", name_str.escape_default()),
                    ));
                }
            }
        }

        let mut children = node.children();

        // If the action is invalid but the key is fine, we still want to return something.
        // That way, the parent can handle the existence of duplicate keybinds,
        // even if their contents are not valid.
        let dummy = Self {
            key,
            action: Action::Spawn(vec![]),
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        };

        if let Some(child) = children.next() {
            for unwanted_child in children {
                ctx.emit_error(DecodeError::unexpected(
                    unwanted_child,
                    "node",
                    "only one action is allowed per keybind",
                ));
            }
            let action = match decode_screenshot_action(child, ctx) {
                Some(res) => res,
                None => Action::decode_node(child, ctx),
            };
            match action {
                Ok(action) => {
                    if !matches!(action, Action::Spawn(_) | Action::SpawnSh(_)) {
                        if let Some(node) = allow_when_locked_node {
                            ctx.emit_error(DecodeError::unexpected(
                                node,
                                "property",
                                "allow-when-locked can only be set on spawn binds",
                            ));
                        }
                    }

                    // The toggle-inhibit action must always be uninhibitable.
                    // Otherwise, it would be impossible to trigger it.
                    if matches!(action, Action::ToggleKeyboardShortcutsInhibit) {
                        allow_inhibiting = false;
                    }

                    // Scroll triggers have no release event, so a hold bound to
                    // one could never end.
                    let needs_release = matches!(action, Action::Zoom(_, true, _))
                        || matches!(action, Action::ZoomLock(true))
                        || matches!(action, Action::ScreenshotMoveSelectionHold)
                        || matches!(action, Action::ScreenshotToggleAxes(true));
                    if needs_release
                        && matches!(
                            key.trigger,
                            Trigger::WheelScrollDown
                                | Trigger::WheelScrollUp
                                | Trigger::WheelScrollLeft
                                | Trigger::WheelScrollRight
                                | Trigger::TouchpadScrollDown
                                | Trigger::TouchpadScrollUp
                                | Trigger::TouchpadScrollLeft
                                | Trigger::TouchpadScrollRight
                        )
                    {
                        ctx.emit_error(DecodeError::unsupported(
                            &node.node_name,
                            "this action requires a trigger with a release event",
                        ));
                    }

                    Ok(Self {
                        key,
                        action,
                        repeat,
                        cooldown,
                        allow_when_locked,
                        allow_inhibiting,
                        hotkey_overlay_title,
                    })
                }
                Err(e) => {
                    ctx.emit_error(e);
                    Ok(dummy)
                }
            }
        } else {
            ctx.emit_error(DecodeError::missing(
                node,
                "expected an action for this keybind",
            ));
            Ok(dummy)
        }
    }
}

impl FromStr for Key {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Modifiers::empty();

        let mut split = s.split('+');
        let key = split.next_back().unwrap();

        for part in split {
            let part = part.trim();
            if part.eq_ignore_ascii_case("mod") {
                modifiers |= Modifiers::COMPOSITOR
            } else if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
                modifiers |= Modifiers::CTRL;
            } else if part.eq_ignore_ascii_case("shift") {
                modifiers |= Modifiers::SHIFT;
            } else if part.eq_ignore_ascii_case("alt") {
                modifiers |= Modifiers::ALT;
            } else if part.eq_ignore_ascii_case("super") || part.eq_ignore_ascii_case("win") {
                modifiers |= Modifiers::SUPER;
            } else if part.eq_ignore_ascii_case("iso_level3_shift")
                || part.eq_ignore_ascii_case("mod5")
            {
                modifiers |= Modifiers::ISO_LEVEL3_SHIFT;
            } else if part.eq_ignore_ascii_case("iso_level5_shift")
                || part.eq_ignore_ascii_case("mod3")
            {
                modifiers |= Modifiers::ISO_LEVEL5_SHIFT;
            } else {
                return Err(miette!("invalid modifier: {part}"));
            }
        }

        let trigger = if key.eq_ignore_ascii_case("MouseLeft") {
            Trigger::MouseLeft
        } else if key.eq_ignore_ascii_case("MouseRight") {
            Trigger::MouseRight
        } else if key.eq_ignore_ascii_case("MouseMiddle") {
            Trigger::MouseMiddle
        } else if key.eq_ignore_ascii_case("MouseBack") {
            Trigger::MouseBack
        } else if key.eq_ignore_ascii_case("MouseForward") {
            Trigger::MouseForward
        } else if key.eq_ignore_ascii_case("WheelScrollDown") {
            Trigger::WheelScrollDown
        } else if key.eq_ignore_ascii_case("WheelScrollUp") {
            Trigger::WheelScrollUp
        } else if key.eq_ignore_ascii_case("WheelScrollLeft") {
            Trigger::WheelScrollLeft
        } else if key.eq_ignore_ascii_case("WheelScrollRight") {
            Trigger::WheelScrollRight
        } else if key.eq_ignore_ascii_case("TouchpadScrollDown") {
            Trigger::TouchpadScrollDown
        } else if key.eq_ignore_ascii_case("TouchpadScrollUp") {
            Trigger::TouchpadScrollUp
        } else if key.eq_ignore_ascii_case("TouchpadScrollLeft") {
            Trigger::TouchpadScrollLeft
        } else if key.eq_ignore_ascii_case("TouchpadScrollRight") {
            Trigger::TouchpadScrollRight
        } else if key.eq_ignore_ascii_case("TabletStylusButton1") {
            Trigger::TabletStylusButton1
        } else if key.eq_ignore_ascii_case("TabletStylusButton2") {
            Trigger::TabletStylusButton2
        } else if key.eq_ignore_ascii_case("TabletStylusButton3") {
            Trigger::TabletStylusButton3
        } else {
            let mut keysym = keysym_from_name(key, KEYSYM_CASE_INSENSITIVE);
            // The keyboard event handling code can receive either
            // XF86ScreenSaver or XF86Screensaver, because there is no
            // case mapping defined between these keysyms. If we just
            // use the case-insensitive version of keysym_from_name it
            // is not possible to bind the uppercase version, because the
            // case-insensitive match prefers the lowercase version when
            // there is a choice.
            //
            // Therefore, when we match this key with the initial
            // case-insensitive match we try a further case-sensitive match
            // (so that either key can be bound). If that fails, we change
            // to the uppercase version because:
            //
            // - A comment in xkb_keysym_from_name (in libxkbcommon) tells us that the uppercase
            //   version is the "best" of the two. [0]
            // - The xkbcommon crate only has a constant for ScreenSaver. [1]
            //
            // [0]: https://github.com/xkbcommon/libxkbcommon/blob/45a118d5325b051343b4b174f60c1434196fa7d4/src/keysym.c#L276
            // [1]: https://docs.rs/xkbcommon/latest/xkbcommon/xkb/keysyms/index.html#:~:text=KEY%5FXF86ScreenSaver
            //
            // See https://github.com/niri-wm/niri/issues/1969
            if keysym == Keysym::XF86_Screensaver {
                keysym = keysym_from_name(key, KEYSYM_NO_FLAGS);
                if keysym.raw() == KEY_NoSymbol {
                    keysym = Keysym::XF86_ScreenSaver;
                }
            }
            if keysym.raw() == KEY_NoSymbol {
                return Err(miette!("invalid key: {key}"));
            }
            Trigger::Keysym(keysym)
        };

        Ok(Key { trigger, modifiers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xf86_screensaver() {
        assert_eq!(
            "XF86ScreenSaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_ScreenSaver),
                modifiers: Modifiers::empty(),
            },
        );
        assert_eq!(
            "XF86Screensaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_Screensaver),
                modifiers: Modifiers::empty(),
            }
        );
        assert_eq!(
            "xf86screensaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_ScreenSaver),
                modifiers: Modifiers::empty(),
            }
        );
    }
    #[test]
    fn parse_screenshot_open_action() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Alt+U { screenshot tool="select" show-pointer=false; }
                Alt+Shift+U { screenshot tool="draw" show-pointer=false; }
                Print { screenshot; }
            }
            "#,
        )
        .unwrap();

        let actions: Vec<&Action> = config.binds.0.iter().map(|bind| &bind.action).collect();
        assert_eq!(
            actions,
            [
                &Action::Screenshot {
                    show_pointer: false,
                    path: None,
                    tool: ScreenshotTool::Select,
                },
                &Action::Screenshot {
                    show_pointer: false,
                    path: None,
                    tool: ScreenshotTool::Draw,
                },
                &Action::Screenshot {
                    show_pointer: true,
                    path: None,
                    tool: ScreenshotTool::Select,
                },
            ]
        );
    }

    #[test]
    fn parse_screenshot_editor_actions() {
        let config = crate::Config::parse_mem(
            r##"
            binds {
                V { screenshot-tool "select"; }
                B { screenshot-tool "draw"; }
                R { screenshot-tool "blur"; }
                C { screenshot-toggle-palette; }
                Mod+C { screenshot-set-palette true; }
                "1" { screenshot-palette-color 1; }
                "7" { screenshot-palette-color 7; }
                Mod+B { screenshot-brush-color "#0080ff"; }
                Minus { screenshot-context "-1"; }
                Equal { screenshot-context "+1"; }
                Mod+Equal { screenshot-context 6; }
                Ctrl+Z { screenshot-undo; }
                Ctrl+Shift+Z { screenshot-redo; }
                P { screenshot-toggle-pointer; }
                Mod+P { screenshot-set-pointer false; }
                Left { screenshot-move-selection x=-1 y=0; }
                Mod+Right { screenshot-move-selection x=20; }
                Shift+Left { screenshot-resize-selection width=-1 height=0; }
                Mod+Down { screenshot-resize-selection height=4; }
                Space repeat=false { screenshot-move-selection-hold; }
                WheelScrollUp { screenshot-magnifier-zoom "+1"; }
                WheelScrollDown { screenshot-magnifier-zoom "-1"; }
                Mod+WheelScrollUp { screenshot-magnifier-zoom 4; }
                Tab { screenshot-focus-next; }
                Shift+Tab { screenshot-focus-previous; }
                Return { screenshot-confirm; }
                Escape { screenshot-cancel; }
                Ctrl+S { confirm-screenshot write-to-disk=true; }
                Ctrl+C { confirm-screenshot; }
                Ctrl+V { confirm-screenshot format="avif" quality=37.5; }
                Ctrl+W { cancel-screenshot; }
                T { screenshot-tool "arrow"; }
                Mod+T { screenshot-tool "rectangle"; }
                Mod+1 { screenshot-arrow-color "#00ff00"; }
                Mod+2 { screenshot-context "+2"; }
                Mod+3 { screenshot-context 1; }
                Mod+4 { screenshot-rectangle-color "#0000ff"; }
                Mod+5 { screenshot-context 128; }
                X { screenshot-toggle-axes; }
                Mod+X { screenshot-set-axes false; }
                Shift_L repeat=false { screenshot-toggle-axes hold=true; }
            }
            "##,
        )
        .unwrap();

        let actions: Vec<&Action> = config.binds.0.iter().map(|bind| &bind.action).collect();
        assert_eq!(
            actions,
            [
                &Action::ScreenshotTool {
                    tool: ScreenshotTool::Select
                },
                &Action::ScreenshotTool {
                    tool: ScreenshotTool::Draw
                },
                &Action::ScreenshotTool {
                    tool: ScreenshotTool::Blur
                },
                &Action::ScreenshotTogglePalette,
                &Action::ScreenshotSetPalette { open: true },
                &Action::ScreenshotPaletteColor { index: 1 },
                &Action::ScreenshotPaletteColor { index: 7 },
                &Action::ScreenshotBrushColor {
                    color: ScreenshotColor::new(0x00, 0x80, 0xff)
                },
                &Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(-1)
                },
                &Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(1)
                },
                &Action::ScreenshotContext {
                    change: ScreenshotValueChange::Set(6)
                },
                &Action::ScreenshotUndo,
                &Action::ScreenshotRedo,
                &Action::ScreenshotTogglePointer,
                &Action::ScreenshotSetPointer {
                    show_pointer: false
                },
                &Action::ScreenshotMoveSelection { x: -1, y: 0 },
                &Action::ScreenshotMoveSelection { x: 20, y: 0 },
                &Action::ScreenshotResizeSelection {
                    width: -1,
                    height: 0
                },
                &Action::ScreenshotResizeSelection {
                    width: 0,
                    height: 4
                },
                &Action::ScreenshotMoveSelectionHold,
                &Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Adjust(1)
                },
                &Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Adjust(-1)
                },
                &Action::ScreenshotMagnifierZoom {
                    zoom: ScreenshotValueChange::Set(4)
                },
                &Action::ScreenshotFocusNext,
                &Action::ScreenshotFocusPrevious,
                &Action::ScreenshotConfirm,
                &Action::ScreenshotCancel,
                &Action::ConfirmScreenshot {
                    write_to_disk: true,
                    format: None,
                    quality: None,
                },
                &Action::ConfirmScreenshot {
                    write_to_disk: true,
                    format: None,
                    quality: None,
                },
                &Action::ConfirmScreenshot {
                    write_to_disk: true,
                    format: Some(ScreenshotFormat::Avif),
                    quality: Some(37.5),
                },
                &Action::CancelScreenshot,
                &Action::ScreenshotTool {
                    tool: ScreenshotTool::Arrow
                },
                &Action::ScreenshotTool {
                    tool: ScreenshotTool::Rectangle
                },
                &Action::ScreenshotArrowColor {
                    color: ScreenshotColor::new(0x00, 0xff, 0x00)
                },
                &Action::ScreenshotContext {
                    change: ScreenshotValueChange::Adjust(2)
                },
                &Action::ScreenshotContext {
                    change: ScreenshotValueChange::Set(1)
                },
                &Action::ScreenshotRectangleColor {
                    color: ScreenshotColor::new(0x00, 0x00, 0xff)
                },
                &Action::ScreenshotContext {
                    change: ScreenshotValueChange::Set(128)
                },
                &Action::ScreenshotToggleAxes(false),
                &Action::ScreenshotSetAxes { enabled: false },
                &Action::ScreenshotToggleAxes(true),
            ]
        );
    }

    #[test]
    fn parse_screenshot_editor_actions_invalid() {
        for action in [
            r#"V { confirm-screenshot quality=-1; }"#,
            r#"V { confirm-screenshot quality=100.1; }"#,
            r#"V { confirm-screenshot format="avif" quality=0; }"#,
            r#"V { confirm-screenshot quality="high"; }"#,
            // Unknown tool.
            r##"V { screenshot-tool "crop"; }"##,
            // Bad colors.
            r##"V { screenshot-brush-color "0080ff"; }"##,
            r##"V { screenshot-brush-color "#80ff"; }"##,
            r##"V { screenshot-brush-color "#0080ffcc"; }"##,
            // Malformed adjustments and types.
            r#"V { screenshot-context "abc"; }"#,
            r#"V { screenshot-context -1; }"#,
            r#"V { screenshot-context 1.5; }"#,
            r#"V { screenshot-context "+"; }"#,
            r#"V { screenshot-context; }"#,
            r#"V { screenshot-set-pointer; }"#,
            r#"V { screenshot-move-selection x="-1"; }"#,
            r#"V { screenshot-palette-color "3"; }"#,
            // Absolute values out of range.
            r#"V { screenshot-palette-color 0; }"#,
            r#"V { screenshot-palette-color 8; }"#,
            r#"V { screenshot-context 0; }"#,
            r#"V { screenshot-context 129; }"#,
            r#"V { screenshot-magnifier-zoom 3; }"#,
            r#"V { screenshot-magnifier-zoom 64; }"#,
            r#"V { screenshot-magnifier-zoom 0; }"#,
            // Per-tool colors still reuse the color form.
            r##"V { screenshot-arrow-color "green"; }"##,
            r##"V { screenshot-arrow-color "#00ff0080"; }"##,
            r##"V { screenshot-rectangle-color "#fff"; }"##,
            // The old per-tool width and blur-strength actions no longer exist, and neither
            // does the intermediate factor name.
            r#"V { screenshot-brush-width 6; }"#,
            r#"V { screenshot-arrow-width 8; }"#,
            r#"V { screenshot-rectangle-width "-1"; }"#,
            r#"V { screenshot-blur-strength 3; }"#,
            r#"V { screenshot-factor "+1"; }"#,
            r#"V { screenshot-set-axes; }"#,
            r#"V { screenshot-set-axes "yes"; }"#,
            // Unknown action name.
            r#"V { screenshot-scribble; }"#,
            // Unexpected extra content.
            r#"V { screenshot-tool "draw" "extra"; }"#,
            r#"V { screenshot-undo x=1; }"#,
            r#"V { screenshot-move-selection -1; }"#,
            r#"V { screenshot move-selection=1; }"#,
            r##"V { screenshot tool="bogus"; }"##,
            // `screenshot` takes no positional argument; path is IPC-only.
            r#"Print { screenshot "/tmp/x.png"; }"#,
            // Unknown format value, wrong type, and format on actions that don't take it.
            r##"V { confirm-screenshot format="jpeg"; }"##,
            r#"V { confirm-screenshot format=1; }"#,
            r##"V { screenshot format="webp"; }"##,
            r##"V { screenshot-screen format="webp"; }"##,
        ] {
            let text = format!("binds {{ {action} }}");
            assert!(
                crate::Config::parse_mem(&text).is_err(),
                "expected parse error for: {text}"
            );
        }
    }

    #[test]
    fn screenshot_local_and_ordinary_share_trigger() {
        // The same key may be bound once in the ordinary category and once among
        // screenshot editor-local binds; only the matching one runs.
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+Z { screenshot-undo; }
                Mod+Z { close-window; }
            }
            "#,
        )
        .unwrap();

        assert_eq!(config.binds.0.len(), 2);
        assert!(config.binds.0[0].action.is_screenshot_editor_local());
        assert!(!config.binds.0[1].action.is_screenshot_editor_local());
    }

    #[test]
    fn duplicate_bind_within_category_is_error() {
        for (first, second) in [
            ("screenshot-undo;", "screenshot-redo;"),
            ("screenshot-undo;", "cancel-screenshot;"),
            ("screenshot-undo;", "confirm-screenshot;"),
            // Ordinary duplicates keep failing.
            ("close-window;", "close-window;"),
            ("close-window;", "spawn \"x\";"),
            // Two different triggers still share by key+modifiers.
        ] {
            let text = format!("binds {{\n    Mod+Z {{ {first} }}\n    Mod+Z {{ {second} }}\n}}");
            assert!(
                crate::Config::parse_mem(&text).is_err(),
                "expected duplicate error for: {text}"
            );
        }
    }

    #[test]
    fn move_selection_hold_needs_release_trigger() {
        // Scroll triggers have no release event, so a hold bound to one could never end.
        for trigger in [
            "WheelScrollUp",
            "WheelScrollDown",
            "TouchpadScrollUp",
            "TouchpadScrollDown",
        ] {
            let text = format!("binds {{\n    {trigger} {{ screenshot-move-selection-hold; }}\n}}");
            assert!(
                crate::Config::parse_mem(&text).is_err(),
                "expected parse error for: {text}"
            );
        }

        for trigger in ["Space", "Mod+A", "MouseRight"] {
            let text = format!("binds {{\n    {trigger} {{ screenshot-move-selection-hold; }}\n}}");
            assert!(crate::Config::parse_mem(&text).is_ok(), "{text}");
        }
    }

    #[test]
    fn parse_iso_level_shifts() {
        assert_eq!(
            "ISO_Level3_Shift+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL3_SHIFT
            },
        );
        assert_eq!(
            "Mod5+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL3_SHIFT
            },
        );

        assert_eq!(
            "ISO_Level5_Shift+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL5_SHIFT
            },
        );
        assert_eq!(
            "Mod3+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL5_SHIFT
            },
        );
    }

    #[test]
    fn parse_zoom_actions() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+equal { zoom-in; }
                Mod+minus { zoom-out; }
                Mod+0 { set-zoom-level 2; }
                Mod+9 { set-zoom-level 1.5; }
                Mod+Shift+0 { reset-zoom; }
                Mod+Shift+L { zoom-lock; }
            }
            "#,
        )
        .unwrap();

        let actions: Vec<&Action> = config.binds.0.iter().map(|bind| &bind.action).collect();
        assert_eq!(
            actions,
            [
                &Action::ZoomIn,
                &Action::ZoomOut,
                &Action::SetZoomLevel(FloatOrInt(2.)),
                &Action::SetZoomLevel(FloatOrInt(1.5)),
                &Action::ResetZoom,
                &Action::ZoomLock(false),
            ]
        );
    }

    #[test]
    fn parse_zoom_level_action() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+Z { zoom 2.0; }
                Mod+Shift+Z { zoom 4; }
                Mod+Alt+Z { zoom 2.0 lock=true; }
                Mod+X { zoom 2.0 hold=true; }
                Mod+MouseBack { zoom 3 hold=true; }
                Mod+TabletStylusButton1 { zoom 1.5 hold=true; }
                Mod+Alt+X { zoom 2.0 hold=true lock=true; }
                Mod+L { zoom-lock; }
                Mod+Alt+L { zoom-lock hold=true; }
            }
            "#,
        )
        .unwrap();

        let actions: Vec<_> = config.binds.0.iter().map(|bind| &bind.action).collect();
        assert_eq!(
            actions,
            [
                &Action::Zoom(ZoomLevelPreset(2.), false, false),
                &Action::Zoom(ZoomLevelPreset(4.), false, false),
                &Action::Zoom(ZoomLevelPreset(2.), false, true),
                &Action::Zoom(ZoomLevelPreset(2.), true, false),
                &Action::Zoom(ZoomLevelPreset(3.), true, false),
                &Action::Zoom(ZoomLevelPreset(1.5), true, false),
                &Action::Zoom(ZoomLevelPreset(2.), true, true),
                &Action::ZoomLock(false),
                &Action::ZoomLock(true),
            ]
        );
    }

    #[test]
    fn parse_zoom_preset_invalid() {
        // Presets must be finite and strictly greater than 1; 1x is not a
        // meaningful activation level.
        for action in [
            "zoom 1",
            "zoom 1.0",
            "zoom 0.5",
            "zoom 0",
            "zoom -1",
            "zoom 1 hold=true",
            "zoom 0.5 lock=true",
            "zoom -2 hold=true lock=true",
        ] {
            let text = format!("binds {{ Mod+Z {{ {action}; }} }}");
            assert!(
                crate::Config::parse_mem(&text).is_err(),
                "expected parse error for: {text}"
            );
        }
    }

    #[test]
    fn parse_zoom_hold_rejects_scroll_triggers() {
        // Scroll triggers have no release event, so a hold bound to one could
        // never end.
        for trigger in [
            "WheelScrollUp",
            "WheelScrollDown",
            "WheelScrollLeft",
            "WheelScrollRight",
            "TouchpadScrollUp",
            "TouchpadScrollDown",
            "TouchpadScrollLeft",
            "TouchpadScrollRight",
        ] {
            for action in ["zoom 2.0 hold=true", "zoom-lock hold=true"] {
                let text = format!("binds {{\n    Mod+{trigger} {{ {action}; }}\n}}");
                assert!(
                    crate::Config::parse_mem(&text).is_err(),
                    "expected parse error for: {text}"
                );
            }
        }

        // Press-only actions stay valid on scroll triggers.
        for action in ["zoom 2.0", "zoom 2.0 lock=true", "zoom-lock"] {
            let text = format!("binds {{\n    Mod+WheelScrollUp {{ {action}; }}\n}}");
            assert!(crate::Config::parse_mem(&text).is_ok());
        }
    }
}
