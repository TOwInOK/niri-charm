//! niri config parsing.
//!
//! The config can be constructed from multiple files (includes). To support this, many types are
//! split into two. For example, `Layout` and `LayoutPart` where `Layout` is the final config and
//! `LayoutPart` is one part parsed from one config file.
//!
//! The convention for `Default` impls is to set the initial values before the parsing occurs.
//! Then, parsing will update the values with those parsed from the config.
//!
//! The `Default` values match those from `default-config.kdl` in almost all cases, with a notable
//! exception of `binds {}` and some window rules.

#[macro_use]
extern crate tracing;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use knuffel::errors::DecodeError;
use knuffel::Decode as _;
use miette::{miette, Context as _, IntoDiagnostic as _};

#[macro_use]
pub mod macros;

pub mod animations;
pub mod appearance;
pub mod binds;
pub mod debug;
pub mod error;
pub mod gestures;
pub mod input;
pub mod layer_rule;
pub mod layout;
pub mod misc;
pub mod output;
pub mod recent_windows;
pub mod screenshot;
pub mod utils;
pub mod window_rule;
pub mod workspace;

pub use crate::animations::{Animation, Animations};
pub use crate::appearance::*;
pub use crate::binds::*;
pub use crate::debug::Debug;
pub use crate::error::{ConfigIncludeError, ConfigParseResult};
pub use crate::gestures::Gestures;
pub use crate::input::{Input, ModKey, ScrollMethod, TrackLayout, WarpMouseToFocusMode, Xkb};
pub use crate::layer_rule::LayerRule;
pub use crate::layout::*;
pub use crate::misc::*;
pub use crate::output::{Output, OutputName, Outputs, Position, Vrr};
use crate::recent_windows::RecentWindowsPart;
pub use crate::recent_windows::{MruDirection, MruFilter, MruPreviews, MruScope, RecentWindows};
use crate::screenshot::ScreenshotConfigPart;
pub use crate::screenshot::{
    BlurStrength, ScreenshotAxes, ScreenshotBlur, ScreenshotBrush, ScreenshotConfig,
    ScreenshotMagnifier, ScreenshotMargin, ScreenshotSelection, ScreenshotToolbar,
    ScreenshotToolbarAlign, ScreenshotToolbarPosition, ScreenshotTooltips,
};
pub use crate::utils::FloatOrInt;
use crate::utils::{expand_home_path, Flag, MergeWith as _};
pub use crate::window_rule::{
    FloatingPosition, OnXdgActivate, PopupsRule, RelativeTo, ResolvedPopupsRules, WindowRule,
};
pub use crate::workspace::{Workspace, WorkspaceLayoutPart};

const RECURSION_LIMIT: u8 = 10;

#[derive(Debug, Default, PartialEq)]
pub struct Config {
    pub input: Input,
    pub outputs: Outputs,
    pub spawn_at_startup: Vec<SpawnAtStartup>,
    pub spawn_sh_at_startup: Vec<SpawnShAtStartup>,
    pub layout: Layout,
    pub prefer_no_csd: bool,
    pub cursor: Cursor,
    pub screenshot_path: ScreenshotPath,
    pub screenshot_format: niri_ipc::ScreenshotFormat,
    pub clipboard: Clipboard,
    pub hotkey_overlay: HotkeyOverlay,
    pub config_notification: ConfigNotification,
    pub animations: Animations,
    pub blur: Blur,
    pub gestures: Gestures,
    pub overview: Overview,
    pub zoom: Zoom,
    pub screenshot: ScreenshotConfig,
    pub environment: Environment,
    pub xwayland_satellite: XwaylandSatellite,
    pub window_rules: Vec<WindowRule>,
    pub layer_rules: Vec<LayerRule>,
    pub binds: Binds,
    pub switch_events: SwitchBinds,
    pub debug: Debug,
    pub workspaces: Vec<Workspace>,
    pub recent_windows: RecentWindows,
}

#[derive(Debug, Clone)]
pub enum ConfigPath {
    /// Explicitly set config path.
    ///
    /// Load the config only from this path, never create it.
    Explicit(PathBuf),

    /// Default config path.
    ///
    /// Prioritize the user path, fallback to the system path, fallback to creating the user path
    /// at compositor startup.
    Regular {
        /// User config path, usually `$XDG_CONFIG_HOME/niri/config.kdl`.
        user_path: PathBuf,
        /// System config path, usually `/etc/niri/config.kdl`.
        system_path: PathBuf,
    },
}

// Newtypes for putting information into the knuffel context.
struct BasePath(PathBuf);
struct RootBase(PathBuf);
struct Recursion(u8);

// FIXME: This is currently used to track all the files that needed to be watched. The name
// `Includes` is not really correct and should be renamed in the future.
#[derive(Default)]
struct Includes(Vec<PathBuf>);

#[derive(Default)]
struct IncludeErrors(Vec<knuffel::Error>);
// Used for recursive include detection.
//
// We don't *need* it because we have a recursion limit, but it makes for nicer error messages.
struct IncludeStack(HashSet<PathBuf>);
struct SawMruBinds(Rc<Cell<bool>>);

// Rather than listing all fields and deriving knuffel::Decode, we implement
// knuffel::DecodeChildren by hand, since we need custom logic for every field anyway: we want to
// merge the values into the config from the context as we go to support the positionality of
// includes. The reason we need this type at all is because knuffel's only entry point that allows
// setting default values on a context is `parse_with_context()` that needs a type to parse.
pub struct ConfigPart;

impl<S> knuffel::DecodeChildren<S> for ConfigPart
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_children(
        nodes: &[knuffel::ast::SpannedNode<S>],
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let _span = tracy_client::span!("decode config file");

        let config = ctx.get::<Rc<RefCell<Config>>>().unwrap().clone();
        let includes = ctx.get::<Rc<RefCell<Includes>>>().unwrap().clone();
        let include_errors = ctx.get::<Rc<RefCell<IncludeErrors>>>().unwrap().clone();
        let recursion = ctx.get::<Recursion>().unwrap().0;
        let saw_mru_binds = ctx.get::<SawMruBinds>().unwrap().0.clone();

        let mut seen = HashSet::new();

        for node in nodes {
            let name = &**node.node_name;

            // Within one config file, splitting sections into multiple parts is not allowed to
            // reduce confusion. The exceptions here aren't multipart; they all add new values.
            if !matches!(
                name,
                "output"
                    | "spawn-at-startup"
                    | "spawn-sh-at-startup"
                    | "window-rule"
                    | "layer-rule"
                    | "workspace"
                    | "include"
            ) && !seen.insert(name)
            {
                ctx.emit_error(DecodeError::unexpected(
                    &node.node_name,
                    "node",
                    format!("duplicate node `{name}`, single node expected"),
                ));
                continue;
            }

            macro_rules! m_merge {
                ($field:ident) => {{
                    let part = knuffel::Decode::decode_node(node, ctx)?;
                    config.borrow_mut().$field.merge_with(&part);
                }};
            }

            macro_rules! m_push {
                ($field:ident) => {{
                    let part = knuffel::Decode::decode_node(node, ctx)?;
                    config.borrow_mut().$field.push(part);
                }};
            }

            match name {
                "input" => m_merge!(input),
                "cursor" => m_merge!(cursor),
                "clipboard" => m_merge!(clipboard),
                "hotkey-overlay" => m_merge!(hotkey_overlay),
                "config-notification" => m_merge!(config_notification),
                "animations" => m_merge!(animations),
                "blur" => m_merge!(blur),
                "gestures" => m_merge!(gestures),
                "overview" => m_merge!(overview),
                "xwayland-satellite" => m_merge!(xwayland_satellite),
                "switch-events" => m_merge!(switch_events),
                "debug" => m_merge!(debug),
                "zoom" => {
                    let part = ZoomPart::decode_node(node, ctx)?;
                    if let (Some(min), Some(max)) = (part.follow_min_speed, part.follow_max_speed) {
                        if max.0 < min.0 {
                            ctx.emit_error(DecodeError::unexpected(
                                &node.node_name,
                                "node",
                                "follow-max-speed must be >= follow-min-speed",
                            ));
                        }
                    }
                    config.borrow_mut().zoom.merge_with(&part);
                }
                "screenshot" => {
                    let part = ScreenshotConfigPart::decode_node(node, ctx)?;
                    part.validate(node, ctx);
                    config.borrow_mut().screenshot.merge_with(&part);
                }

                // Multipart sections.
                "output" => {
                    let part = Output::decode_node(node, ctx)?;
                    config.borrow_mut().outputs.0.push(part);
                }
                "spawn-at-startup" => m_push!(spawn_at_startup),
                "spawn-sh-at-startup" => m_push!(spawn_sh_at_startup),
                "window-rule" => m_push!(window_rules),
                "layer-rule" => m_push!(layer_rules),
                "workspace" => m_push!(workspaces),

                // Single-part sections.
                "binds" => {
                    let part = Binds::decode_node(node, ctx)?;

                    // We replace conflicting binds, rather than error, to support the use-case
                    // where you import some preconfigured-dots.kdl, then override some binds with
                    // your own.
                    let mut config = config.borrow_mut();
                    let binds = &mut config.binds.0;
                    // Remove existing binds matching any new bind.
                    binds.retain(|bind| !part.0.iter().any(|new| new.key == bind.key));
                    // Add all new binds.
                    binds.extend(part.0);
                }
                "environment" => {
                    let part = Environment::decode_node(node, ctx)?;
                    config.borrow_mut().environment.0.extend(part.0);
                }

                "prefer-no-csd" => {
                    config.borrow_mut().prefer_no_csd = Flag::decode_node(node, ctx)?.0
                }

                "screenshot-path" => {
                    let part = knuffel::Decode::decode_node(node, ctx)?;
                    config.borrow_mut().screenshot_path = part;
                }

                "screenshot-format" => {
                    let part = ScreenshotFormatArg::decode_node(node, ctx)?;
                    config.borrow_mut().screenshot_format = part.0;
                }

                "layout" => {
                    let mut part = LayoutPart::decode_node(node, ctx)?;

                    // Preserve the behavior we'd always had for the border section:
                    // - `layout {}` gives border = off
                    // - `layout { border {} }` gives border = on
                    // - `layout { border { off } }` gives border = off
                    //
                    // This behavior is inconsistent with the rest of the config where adding an
                    // empty section generally doesn't change the outcome. Particularly, shadows
                    // are also disabled by default (like borders), and they always had an `on`
                    // instead of an `off` for this reason, so that writing `layout { shadow {} }`
                    // still results in shadow = off, as it should.
                    //
                    // Unfortunately, the default config has always had wording that heavily
                    // implies that `layout { border {} }` enables the borders. This wording is
                    // sure to be present in a lot of users' configs by now, which we can't change.
                    //
                    // Another way to make things consistent would be to default borders to on.
                    // However, that is annoying because it would mean changing many tests that
                    // rely on borders being off by default. This would also contradict the
                    // intended default borders value (off).
                    //
                    // So, let's just work around the problem here, preserving the original
                    // behavior.
                    if recursion == 0 {
                        if let Some(border) = part.border.as_mut() {
                            if !border.on && !border.off {
                                border.on = true;
                            }
                        }
                    }

                    config.borrow_mut().layout.merge_with(&part);
                }

                "recent-windows" => {
                    let part = RecentWindowsPart::decode_node(node, ctx)?;

                    let mut config = config.borrow_mut();

                    // When an MRU binds section is encountered for the first time, clear out the
                    // default MRU binds.
                    if !saw_mru_binds.get() && part.binds.is_some() {
                        saw_mru_binds.set(true);
                        config.recent_windows.binds.clear();
                    }

                    config.recent_windows.merge_with(&part);
                }

                "include" => {
                    // Parse the path argument
                    let mut iter_args = node.arguments.iter();
                    let path_val = iter_args.next().ok_or_else(|| {
                        DecodeError::missing(
                            node,
                            "additional argument for include path is required",
                        )
                    })?;
                    // Check for extra arguments
                    if let Some(val) = iter_args.next() {
                        ctx.emit_error(DecodeError::unexpected(
                            &val.literal,
                            "argument",
                            "unexpected argument",
                        ));
                    }

                    // Parse the optional property
                    let mut optional = false;
                    for (name, val) in &node.properties {
                        match &***name {
                            "optional" => {
                                optional = knuffel::traits::DecodeScalar::decode(val, ctx)?;
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

                    // Check for unexpected children
                    for child in node.children() {
                        ctx.emit_error(DecodeError::unexpected(
                            child,
                            "node",
                            format!("unexpected node `{}`", child.node_name.escape_default()),
                        ));
                    }

                    // We use DecodeError::Missing throughout this block because it results in the
                    // least confusing error messages while still allowing to provide a span.

                    let decoded_path = knuffel::traits::DecodeScalar::decode(path_val, ctx)?;

                    // Expand ~ into the home dir
                    let Some(path) = expand_home_path(decoded_path, node, ctx) else {
                        continue;
                    };

                    let recursion = ctx.get::<Recursion>().unwrap().0 + 1;
                    if recursion == RECURSION_LIMIT {
                        ctx.emit_error(DecodeError::missing(
                            node,
                            format!(
                                "reached the recursion limit; \
                                 includes cannot be {RECURSION_LIMIT} levels deep"
                            ),
                        ));
                        continue;
                    }

                    let Some(filename) = path.file_name().and_then(OsStr::to_str) else {
                        ctx.emit_error(DecodeError::missing(
                            node,
                            "include path doesn't have a valid file name",
                        ));
                        continue;
                    };
                    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();

                    // Check for recursive include for a nicer error message.
                    let mut include_stack = ctx.get::<IncludeStack>().unwrap().0.clone();
                    if !include_stack.insert(path.to_path_buf()) {
                        ctx.emit_error(DecodeError::missing(
                            node,
                            "recursive include (file includes itself)",
                        ));
                        continue;
                    }

                    // Store even if the include fails to read or parse, so it gets watched.
                    includes.borrow_mut().0.push(path.to_path_buf());

                    match fs::read_to_string(&path) {
                        Ok(text) => {
                            // Try to get filename relative to the root base config folder for
                            // clearer error messages.
                            let root_base = &ctx.get::<RootBase>().unwrap().0;
                            // Failing to strip prefix usually means absolute path; show it in full.
                            let relative_path = path.strip_prefix(root_base).ok().unwrap_or(&path);
                            let filename = relative_path.to_str().unwrap_or(filename);

                            let part = knuffel::parse_with_context::<
                                ConfigPart,
                                knuffel::span::Span,
                                _,
                            >(filename, &text, |ctx| {
                                ctx.set(BasePath(base));
                                ctx.set(RootBase(root_base.clone()));
                                ctx.set(Recursion(recursion));
                                ctx.set(includes.clone());
                                ctx.set(include_errors.clone());
                                ctx.set(IncludeStack(include_stack));
                                ctx.set(SawMruBinds(saw_mru_binds.clone()));
                                ctx.set(config.clone());
                            });

                            match part {
                                Ok(_) => {}
                                Err(err) => {
                                    include_errors.borrow_mut().0.push(err);

                                    ctx.emit_error(DecodeError::missing(
                                        node,
                                        "failed to parse included config",
                                    ));
                                }
                            }
                        }
                        Err(err) => {
                            if optional && err.kind() == std::io::ErrorKind::NotFound {
                                // Warn about missing optional includes
                                warn!("optional include not found: {path:?}");
                            } else {
                                // Report all other errors normally
                                ctx.emit_error(DecodeError::missing(
                                    node,
                                    format!("failed to read included config from {path:?}: {err}"),
                                ));
                            }
                        }
                    }
                }

                name => {
                    ctx.emit_error(DecodeError::unexpected(
                        node,
                        "node",
                        format!("unexpected node `{}`", name.escape_default()),
                    ));
                }
            }
        }

        Ok(Self)
    }
}

impl Config {
    pub fn load_default() -> Self {
        let res = Config::parse(
            Path::new("default-config.kdl"),
            include_str!("../../resources/default-config.kdl"),
        );

        // Includes in the default config can break its parsing at runtime.
        assert!(
            res.includes.is_empty(),
            "default config must not have includes",
        );

        res.config.unwrap()
    }

    pub fn load(path: &Path) -> ConfigParseResult<Self, miette::Report> {
        let contents = match fs::read_to_string(path) {
            Ok(x) => x,
            Err(err) => {
                return ConfigParseResult::from_err(
                    miette!(err).context(format!("error reading {path:?}")),
                );
            }
        };

        Self::parse(path, &contents).map_config_res(|res| {
            let config = res.context("error parsing")?;
            debug!("loaded config from {path:?}");
            Ok(config)
        })
    }

    pub fn parse(path: &Path, text: &str) -> ConfigParseResult<Self, ConfigIncludeError> {
        let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let filename = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("config.kdl");

        let config = Rc::new(RefCell::new(Config::default()));
        let includes = Rc::new(RefCell::new(Includes(Vec::new())));
        let include_errors = Rc::new(RefCell::new(IncludeErrors(Vec::new())));
        let include_stack = HashSet::from([path.to_path_buf()]);

        let part = knuffel::parse_with_context::<ConfigPart, knuffel::span::Span, _>(
            filename,
            text,
            |ctx| {
                ctx.set(BasePath(base.clone()));
                ctx.set(RootBase(base));
                ctx.set(Recursion(0));
                ctx.set(includes.clone());
                ctx.set(include_errors.clone());
                ctx.set(IncludeStack(include_stack));
                ctx.set(SawMruBinds(Rc::new(Cell::new(false))));
                ctx.set(config.clone());
            },
        );

        let includes = includes.take().0;
        let include_errors = include_errors.take().0;
        let config = part
            .map(|_| config.take())
            .map_err(move |err| ConfigIncludeError {
                main: err,
                includes: include_errors,
            });

        ConfigParseResult { config, includes }
    }

    pub fn parse_mem(text: &str) -> Result<Self, ConfigIncludeError> {
        Self::parse(Path::new("config.kdl"), text).config
    }
}

impl ConfigPath {
    /// Loads the config, returns an error if it doesn't exist.
    pub fn load(&self) -> ConfigParseResult<Config, miette::Report> {
        let _span = tracy_client::span!("ConfigPath::load");

        self.load_inner(|user_path, system_path| {
            Err(miette!(
                "no config file found; create one at {user_path:?} or {system_path:?}",
            ))
        })
        .map_config_res(|res| res.context("error loading config"))
    }

    /// Loads the config, or creates it if it doesn't exist.
    ///
    /// Returns a tuple containing the path that was created, if any, and the loaded config.
    ///
    /// If the config was created, but for some reason could not be read afterwards,
    /// this may return `(Some(_), Err(_))`.
    pub fn load_or_create(&self) -> (Option<&Path>, ConfigParseResult<Config, miette::Report>) {
        let _span = tracy_client::span!("ConfigPath::load_or_create");

        let mut created_at = None;

        let result = self
            .load_inner(|user_path, _| {
                Self::create(user_path, &mut created_at)
                    .map(|()| user_path)
                    .with_context(|| format!("error creating config at {user_path:?}"))
            })
            .map_config_res(|res| res.context("error loading config"));

        (created_at, result)
    }

    fn load_inner<'a>(
        &'a self,
        maybe_create: impl FnOnce(&'a Path, &'a Path) -> miette::Result<&'a Path>,
    ) -> ConfigParseResult<Config, miette::Report> {
        let path = match self {
            ConfigPath::Explicit(path) => path.as_path(),
            ConfigPath::Regular {
                user_path,
                system_path,
            } => {
                if user_path.exists() {
                    user_path.as_path()
                } else if system_path.exists() {
                    system_path.as_path()
                } else {
                    match maybe_create(user_path.as_path(), system_path.as_path()) {
                        Ok(x) => x,
                        Err(err) => return ConfigParseResult::from_err(miette!(err)),
                    }
                }
            }
        };
        Config::load(path)
    }

    fn create<'a>(path: &'a Path, created_at: &mut Option<&'a Path>) -> miette::Result<()> {
        if let Some(default_parent) = path.parent() {
            fs::create_dir_all(default_parent)
                .into_diagnostic()
                .with_context(|| format!("error creating config directory {default_parent:?}"))?;
        }

        // Create the config and fill it with the default config if it doesn't exist.
        let mut new_file = match File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
        {
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            res => res,
        }
        .into_diagnostic()
        .with_context(|| format!("error opening config file at {path:?}"))?;

        *created_at = Some(path);

        let default = include_bytes!("../../resources/default-config.kdl");

        new_file
            .write_all(default)
            .into_diagnostic()
            .with_context(|| format!("error writing default config to {path:?}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn can_create_default_config() {
        let _ = Config::load_default();
    }

    #[test]
    fn default_repeat_params() {
        let config = Config::parse_mem("").unwrap();
        assert_eq!(config.input.keyboard.repeat_delay, 600);
        assert_eq!(config.input.keyboard.repeat_rate, 25);
    }

    #[track_caller]
    fn do_parse(text: &str) -> Config {
        Config::parse_mem(text)
            .map_err(miette::Report::new)
            .unwrap()
    }

    #[test]
    fn parse_on_xdg_activate() {
        let parsed = do_parse(
            r#"
            window-rule { on-xdg-activate "ignore"; }
            window-rule { on-xdg-activate "set-urgent"; }
            window-rule { on-xdg-activate "focus"; }
            "#,
        );

        assert_eq!(
            parsed
                .window_rules
                .iter()
                .map(|rule| rule.on_xdg_activate)
                .collect::<Vec<_>>(),
            vec![
                Some(OnXdgActivate::Ignore),
                Some(OnXdgActivate::SetUrgent),
                Some(OnXdgActivate::Focus),
            ]
        );
    }

    #[test]
    fn parse() {
        do_parse(
            r##"
            input {
                keyboard {
                    repeat-delay 600
                    repeat-rate 25
                    track-layout "window"
                    xkb {
                        layout "us,ru"
                        options "grp:win_space_toggle"
                    }
                }

                touchpad {
                    tap
                    dwt
                    dwtp
                    drag true
                    click-method "clickfinger"
                    accel-speed 0.2
                    accel-profile "flat"
                    scroll-method "two-finger"
                    scroll-button 272
                    scroll-button-lock
                    tap-button-map "left-middle-right"
                    disabled-on-external-mouse
                    scroll-factor 0.9
                    pinch-sensitivity 1.8
                }

                mouse {
                    natural-scroll
                    accel-speed 0.4
                    accel-profile "flat"
                    scroll-method "no-scroll"
                    scroll-button 273
                    middle-emulation
                    scroll-factor 0.2
                }

                trackpoint {
                    off
                    natural-scroll
                    accel-speed 0.0
                    accel-profile "flat"
                    scroll-method "on-button-down"
                    scroll-button 274
                }

                trackball {
                    off
                    natural-scroll
                    accel-speed 0.0
                    accel-profile "flat"
                    scroll-method "edge"
                    scroll-button 275
                    scroll-button-lock
                    left-handed
                    middle-emulation
                }

                tablet {
                    map-to-output "eDP-1"
                    map-to-focused-output
                    map-to-focused-window
                    calibration-matrix 1.0 2.0 3.0 \
                                       4.0 5.0 6.0
                }

                touch {
                    map-to-output "eDP-1"
                }

                disable-power-key-handling

                warp-mouse-to-focus
                focus-follows-mouse
                workspace-auto-back-and-forth

                mod-key "Mod5"
                mod-key-nested "Super"
            }

            output "eDP-1" {
                focus-at-startup
                scale 2
                transform "flipped-90"
                position x=10 y=20
                mode "1920x1080@144"
                max-bpc 10
                variable-refresh-rate on-demand=true
                background-color "rgba(25, 25, 102, 1.0)"
                hot-corners {
                    off
                    top-left
                    top-right
                    bottom-left
                    bottom-right
                }
            }

            output "eDP-2" {
                mode custom=true "1920x1080@144"
            }

            output "eDP-3" {
                modeline 173.00  1920 2048 2248 2576  1080 1083 1088 1120 "-hsync" "+vsync"
            }

            layout {
                focus-ring {
                    width 5
                    active-color 0 100 200 255
                    inactive-color 255 200 100 0
                    active-gradient from="rgba(10, 20, 30, 1.0)" to="#0080ffff" relative-to="workspace-view"
                }

                border {
                    width 3
                    inactive-color "rgba(255, 200, 100, 0.0)"
                }

                shadow {
                    offset x=10 y=-20
                }

                tab-indicator {
                    width 10
                    position "top"
                }

                preset-column-widths {
                    proportion 0.25
                    proportion 0.5
                    fixed 960
                    fixed 1280
                }

                preset-window-heights {
                    proportion 0.25
                    proportion 0.5
                    fixed 960
                    fixed 1280
                }

                default-column-width { proportion 0.25; }

                gaps 8

                struts {
                    left 1
                    right 2
                    top 3
                }

                center-focused-column "on-overflow"

                default-column-display "tabbed"

                insert-hint {
                    color "rgb(255, 200, 127)"
                    gradient from="rgba(10, 20, 30, 1.0)" to="#0080ffff" relative-to="workspace-view"
                }
            }

            spawn-at-startup "alacritty" "-e" "fish"
            spawn-sh-at-startup "qs -c ~/source/qs/MyAwesomeShell"

            prefer-no-csd

            cursor {
                xcursor-theme "breeze_cursors"
                xcursor-size 16
                hide-when-typing
                hide-after-inactive-ms 3000
            }

            screenshot-path "~/Screenshots/screenshot.png"

            clipboard {
                disable-primary
            }

            hotkey-overlay {
                skip-at-startup
            }

            animations {
                slowdown 2.0

                workspace-switch {
                    spring damping-ratio=1.0 stiffness=1000 epsilon=0.0001
                }

                horizontal-view-movement {
                    duration-ms 100
                    curve "ease-out-expo"
                }

                window-open { off; }

                window-close {
                    curve "cubic-bezier" 0.05 0.7 0.1 1
                }

                recent-windows-close {
                    off
                }
            }

            gestures {
                dnd-edge-view-scroll {
                    trigger-width 10
                    max-speed 50
                }
            }

            environment {
                QT_QPA_PLATFORM "wayland"
                DISPLAY null
            }

            window-rule {
                match app-id=".*alacritty"
                exclude title="~"
                exclude is-active=true is-focused=false

                open-on-output "eDP-1"
                open-maximized true
                open-fullscreen false
                open-floating false
                open-focused true
                default-window-height { fixed 500; }
                default-column-display "tabbed"
                default-floating-position x=100 y=-200 relative-to="bottom-left"
                on-xdg-activate "ignore"

                focus-ring {
                    off
                    width 3
                }

                border {
                    on
                    width 8.5
                }

                tab-indicator {
                    active-color "#f00"
                }

                pinch-sensitivity 1.2
            }

            layer-rule {
                match namespace="^notifications$"
                block-out-from "screencast"
            }

            binds {
                Mod+Escape hotkey-overlay-title="Inhibit" { toggle-keyboard-shortcuts-inhibit; }
                Mod+Shift+Escape allow-inhibiting=true { toggle-keyboard-shortcuts-inhibit; }
                Mod+T allow-when-locked=true { spawn "alacritty"; }
                Mod+Q hotkey-overlay-title=null { close-window; }
                Mod+Shift+H { focus-monitor-left; }
                Mod+Shift+O { focus-monitor "eDP-1"; }
                Mod+Ctrl+Shift+L { move-window-to-monitor-right; }
                Mod+Ctrl+Alt+O { move-window-to-monitor "eDP-1"; }
                Mod+Ctrl+Alt+P { move-column-to-monitor "DP-1"; }
                Mod+Comma { consume-window-into-column; }
                Mod+1 { focus-workspace 1; }
                Mod+Shift+1 { focus-workspace "workspace-1"; }
                Mod+Shift+E allow-inhibiting=false { quit skip-confirmation=true; }
                Mod+WheelScrollDown cooldown-ms=150 { focus-workspace-down; }
                Super+Alt+S allow-when-locked=true { spawn-sh "pkill orca || exec orca"; }
            }

            switch-events {
                tablet-mode-on { spawn "bash" "-c" "gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled true"; }
                tablet-mode-off { spawn "bash" "-c" "gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled false"; }
            }

            debug {
                render-drm-device "/dev/dri/renderD129"
                ignore-drm-device "/dev/dri/renderD128"
                ignore-drm-device "/dev/dri/renderD130"
            }

            workspace "workspace-1" {
                open-on-output "eDP-1"
                open-on-output "DP-1"
            }
            workspace "workspace-2"
            workspace "workspace-3"

            recent-windows {
                off

                highlight {
                    padding 15
                    active-color "#00ff00"
                }

                previews {
                    max-height 960
                }

                binds {
                    Alt+Tab { next-window; }
                    Alt+grave { next-window filter="app-id"; }
                    Super+Tab { next-window scope="output"; }
                }
            }
            "##,
        );
    }

    #[test]
    fn parse_zoom_defaults() {
        let config = do_parse("");
        assert_eq!(config.zoom.max_zoom, 10.);
        assert_eq!(config.zoom.increment_factor, 1.2);
        assert_eq!(config.zoom.deadzone_size, 0.5);
        assert_eq!(config.zoom.pinch_fingers, None);
    }

    #[test]
    fn parse_zoom_sampling() {
        // Linear is the default.
        let config = do_parse("");
        assert_eq!(config.zoom.sampling, ZoomSampling::Linear);

        let config = do_parse(r#"zoom { sampling "linear"; }"#);
        assert_eq!(config.zoom.sampling, ZoomSampling::Linear);

        let config = do_parse(r#"zoom { sampling "nearest"; }"#);
        assert_eq!(config.zoom.sampling, ZoomSampling::Nearest);

        let config = do_parse(r#"zoom { sampling "auto" threshold=4.0; }"#);
        assert_eq!(config.zoom.sampling, ZoomSampling::Auto { threshold: 4. });

        // Integer thresholds are accepted.
        let config = do_parse(r#"zoom { sampling "auto" threshold=4; }"#);
        assert_eq!(config.zoom.sampling, ZoomSampling::Auto { threshold: 4. });

        // A later zoom block overrides the sampling policy, while a zoom
        // block without sampling leaves it alone.
        let mut zoom = Zoom {
            sampling: ZoomSampling::Nearest,
            ..Zoom::default()
        };

        let part: ZoomPart = knuffel::parse("part.kdl", "max-zoom 5;")
            .map_err(miette::Report::new)
            .unwrap();
        zoom.merge_with(&part);
        assert_eq!(zoom.sampling, ZoomSampling::Nearest);
        assert_eq!(zoom.max_zoom, 5.);

        let part: ZoomPart = knuffel::parse("part.kdl", r#"sampling "auto" threshold=2;"#)
            .map_err(miette::Report::new)
            .unwrap();
        zoom.merge_with(&part);
        assert_eq!(zoom.sampling, ZoomSampling::Auto { threshold: 2. });
    }

    #[test]
    fn parse_zoom_sampling_invalid() {
        for text in [
            // The mode argument is required.
            "zoom { sampling; }",
            // Unknown modes are rejected.
            r#"zoom { sampling "bilinear"; }"#,
            r#"zoom { sampling "auto"; }"#,
            // Auto requires an explicit finite threshold greater than 1.
            r#"zoom { sampling "auto" threshold=1; }"#,
            r#"zoom { sampling "auto" threshold=1.0; }"#,
            r#"zoom { sampling "auto" threshold=0.5; }"#,
            r#"zoom { sampling "auto" threshold=-2; }"#,
            r#"zoom { sampling "auto" threshold=#inf; }"#,
            r#"zoom { sampling "auto" threshold=#nan; }"#,
            r#"zoom { sampling "auto" threshold="high"; }"#,
            // Threshold is only valid for auto.
            r#"zoom { sampling "linear" threshold=4; }"#,
            r#"zoom { sampling "nearest" threshold=4; }"#,
            // No extra arguments, properties or children.
            r#"zoom { sampling "linear" "nearest"; }"#,
            r#"zoom { sampling "auto" threshold=4 foo=1; }"#,
            r#"zoom { sampling "linear" { child; } }"#,
        ] {
            assert!(
                Config::parse_mem(text).is_err(),
                "expected parse error for: {text}"
            );
        }
    }

    #[test]
    fn zoom_sampling_uses_nearest() {
        // Nothing is magnified at or below scale 1.
        for scale in [0.5, 1.] {
            assert!(!ZoomSampling::Linear.uses_nearest(scale));
            assert!(!ZoomSampling::Nearest.uses_nearest(scale));
            assert!(!ZoomSampling::Auto { threshold: 2. }.uses_nearest(scale));
        }

        // Linear never uses nearest; nearest always does once magnified.
        for scale in [1.5, 10.] {
            assert!(!ZoomSampling::Linear.uses_nearest(scale));
            assert!(ZoomSampling::Nearest.uses_nearest(scale));
        }

        // Auto switches at the threshold, inclusive.
        let auto = ZoomSampling::Auto { threshold: 4. };
        assert!(!auto.uses_nearest(1.5));
        assert!(!auto.uses_nearest(3.999));
        assert!(auto.uses_nearest(4.));
        assert!(auto.uses_nearest(10.));
    }

    #[test]
    fn parse_zoom() {
        let config = do_parse(
            r#"
            zoom {
                max-zoom 4
                increment-factor 1.5
                deadzone-size 0.25
            }
            "#,
        );
        assert_eq!(config.zoom.max_zoom, 4.);
        assert_eq!(config.zoom.increment_factor, 1.5);
        assert_eq!(config.zoom.deadzone_size, 0.25);

        // Missing fields keep their defaults.
        let config = do_parse(
            r#"
            zoom {
                max-zoom 5
            }
            "#,
        );
        assert_eq!(config.zoom.max_zoom, 5.);
        assert_eq!(config.zoom.increment_factor, 1.2);
        assert_eq!(config.zoom.deadzone_size, 0.5);

        // Integer literals are accepted for every field.
        let config = do_parse(
            r#"
            zoom {
                max-zoom 4
                increment-factor 2
                deadzone-size 1
            }
            "#,
        );
        assert_eq!(config.zoom.max_zoom, 4.);
        assert_eq!(config.zoom.increment_factor, 2.);
        assert_eq!(config.zoom.deadzone_size, 1.);

        // Fractional literals.
        let config = do_parse(
            r#"
            zoom {
                max-zoom 3.5
                increment-factor 1.15
                deadzone-size 0.35
            }
            "#,
        );
        assert_eq!(config.zoom.max_zoom, 3.5);
        assert_eq!(config.zoom.increment_factor, 1.15);
        assert_eq!(config.zoom.deadzone_size, 0.35);
    }

    #[test]
    fn parse_zoom_invalid() {
        for text in [
            "zoom { max-zoom 0 }",
            "zoom { max-zoom 0.5 }",
            "zoom { increment-factor 1 }",
            "zoom { increment-factor 1.0 }",
            "zoom { increment-factor 0.5 }",
            "zoom { deadzone-size -1 }",
            "zoom { deadzone-size -0.1 }",
            "zoom { deadzone-size 2 }",
            "zoom { deadzone-size 1.5 }",
            "zoom { pinch-fingers 0; }",
            "zoom { pinch-fingers 1; }",
            "zoom { pinch-fingers -1; }",
            "zoom { pinch-fingers 1.5; }",
        ] {
            assert!(
                Config::parse_mem(text).is_err(),
                "expected parse error for: {text}"
            );
        }
    }

    #[test]
    fn parse_zoom_pinch_fingers() {
        // Disabled by default.
        let config = do_parse("zoom { max-zoom 4; }");
        assert_eq!(config.zoom.pinch_fingers, None);

        // Three fingers: the recommended opt-in.
        let config = do_parse("zoom { pinch-fingers 3; }");
        assert_eq!(config.zoom.pinch_fingers, Some(3));

        // Two fingers are allowed as an explicit opt-in.
        let config = do_parse("zoom { pinch-fingers 2; }");
        assert_eq!(config.zoom.pinch_fingers, Some(2));

        // Other zoom fields are unaffected.
        let config = do_parse(
            r#"
            zoom {
                max-zoom 4
                pinch-fingers 3
            }
            "#,
        );
        assert_eq!(config.zoom.max_zoom, 4.);
        assert_eq!(config.zoom.increment_factor, 1.2);
        assert_eq!(config.zoom.deadzone_size, 0.5);
        assert_eq!(config.zoom.pinch_fingers, Some(3));
    }

    #[test]
    fn parse_zoom_debug() {
        // Disabled by default.
        let config = do_parse("");
        assert!(!config.zoom.debug.deadzone);
        assert!(!config.zoom.debug.focal_point);

        // Both flags on.
        let config = do_parse(
            r#"
            zoom {
                debug {
                    deadzone true
                    focal-point true
                }
            }
            "#,
        );
        assert!(config.zoom.debug.deadzone);
        assert!(config.zoom.debug.focal_point);

        // A bare flag means true; an explicit false disables.
        let config = do_parse("zoom { debug { deadzone; focal-point false; }; }");
        assert!(config.zoom.debug.deadzone);
        assert!(!config.zoom.debug.focal_point);

        let config = do_parse("zoom { debug { deadzone false; }; }");
        assert!(!config.zoom.debug.deadzone);
        assert!(!config.zoom.debug.focal_point);

        // A partial debug block leaves the other zoom settings alone.
        let config = do_parse(
            r#"
            zoom {
                max-zoom 4
                deadzone-size 0.25
                debug {
                    focal-point true
                }
            }
            "#,
        );
        assert_eq!(config.zoom.max_zoom, 4.);
        assert_eq!(config.zoom.deadzone_size, 0.25);
        assert!(!config.zoom.debug.deadzone);
        assert!(config.zoom.debug.focal_point);
    }

    #[test]
    fn merge_zoom_debug() {
        use crate::utils::MergeWith;

        // A debug block in an included file merges field-by-field: it must
        // not reset debug flags set earlier, nor other zoom settings.
        let mut zoom = Zoom {
            max_zoom: 4.,
            debug: ZoomDebug {
                deadzone: true,
                focal_point: false,
            },
            ..Zoom::default()
        };

        let part: ZoomPart = knuffel::parse("part.kdl", "debug { focal-point true; }")
            .map_err(miette::Report::new)
            .unwrap();
        zoom.merge_with(&part);

        assert_eq!(zoom.max_zoom, 4.);
        assert!(zoom.debug.deadzone);
        assert!(zoom.debug.focal_point);

        // An explicit false clears a previously set flag.
        let part: ZoomPart = knuffel::parse("part.kdl", "debug { deadzone false; }")
            .map_err(miette::Report::new)
            .unwrap();
        zoom.merge_with(&part);

        assert!(!zoom.debug.deadzone);
        assert!(zoom.debug.focal_point);
    }

    #[test]
    fn parse_zoom_animation() {
        use crate::animations::{Curve, Kind};

        // Default: the overview-style spring.
        let config = do_parse("");
        assert!(!config.animations.zoom.0.off);
        assert!(matches!(
            config.animations.zoom.0.kind,
            Kind::Spring(params)
                if params.damping_ratio == 1. && params.stiffness == 800
        ));

        let config = do_parse(
            r#"
            animations {
                zoom {
                    off
                }
            }
            "#,
        );
        assert!(config.animations.zoom.0.off);

        let config = do_parse(
            r#"
            animations {
                zoom {
                    duration-ms 200
                    curve "ease-out-quad"
                }
            }
            "#,
        );
        assert!(matches!(
            config.animations.zoom.0.kind,
            Kind::Easing(params)
                if params.duration_ms == 200 && params.curve == Curve::EaseOutQuad
        ));

        let config = do_parse(
            r#"
            animations {
                zoom {
                    spring damping-ratio=0.8 stiffness=500 epsilon=0.001
                }
            }
            "#,
        );
        assert!(matches!(
            config.animations.zoom.0.kind,
            Kind::Spring(params)
                if params.damping_ratio == 0.8 && params.stiffness == 500
        ));
    }

    #[test]
    fn parse_screenshot() {
        let config = do_parse(
            r##"
            screenshot {
                toolbar {
                    position "bottom"
                    align "left"
                    margin top=16 bottom=4
                    button-size 48
                    icon-size 32
                    border-width 0
                    accent-color "#00ff00"
                    opacity 0.9
                    drag-opacity 0.25
                }

                tooltips {
                    enabled false
                    font "monospace"
                    font-size 12.5
                }

                selection {
                    border-color "#ff0000"
                    border-width 3
                    dim-opacity 0.8
                    remember-selection false
                }

                brush {
                    width 8
                    color "#0080ff"
                    palette "#112233" "#445566"
                    open-palette-on-click false
                    close-palette-on-select false
                }

                blur {
                    strength "high"
                }

                magnifier {
                    diameter 256
                    pointer-gap 24
                    initial-zoom 4
                    show-from 4
                    max-zoom 16
                    grid false
                    grid-from 8
                    cross-highlight false
                    cross-highlight-color "#ff000080"
                }

                arrow {
                    width 12
                    color "#00ff80"
                }

                rectangle {
                    width 3
                }

                axes {
                    color "#80c7ff80"
                    opacity 0.7
                    width 2
                    dash-length 10
                    dash-gap 5
                    switch-threshold 12
                }
            }
            "##,
        );

        let shot = &config.screenshot;
        assert_eq!(shot.toolbar.position, ScreenshotToolbarPosition::Bottom);
        assert_eq!(shot.toolbar.align, ScreenshotToolbarAlign::Left);
        assert_eq!(shot.toolbar.margin.top, 16.);
        assert_eq!(shot.toolbar.margin.bottom, 4.);
        // Omitted margin sides keep their defaults.
        assert_eq!(shot.toolbar.margin.right, 8.);
        assert_eq!(shot.toolbar.margin.left, 8.);
        assert_eq!(shot.toolbar.button_size, 48.);
        assert_eq!(shot.toolbar.icon_size, 32.);
        assert_eq!(shot.toolbar.border_width, 0.);
        assert_eq!(
            shot.toolbar.accent_color,
            Color::from_rgba8_unpremul(0, 255, 0, 255)
        );
        assert_eq!(shot.toolbar.opacity, 0.9);
        assert_eq!(shot.toolbar.drag_opacity, 0.25);

        assert_eq!(shot.tooltips.enabled, false);
        assert_eq!(shot.tooltips.font, "monospace");
        assert_eq!(shot.tooltips.font_size, 12.5);

        assert_eq!(shot.selection.border_width, 3.);
        assert_eq!(shot.selection.dim_opacity, 0.8);
        assert_eq!(shot.selection.remember_selection, false);

        assert_eq!(shot.brush.width, 8);
        assert_eq!(
            shot.brush.color,
            Color::from_rgba8_unpremul(0, 128, 255, 255)
        );
        assert_eq!(
            shot.brush.palette,
            vec![
                Color::from_rgba8_unpremul(0x11, 0x22, 0x33, 255),
                Color::from_rgba8_unpremul(0x44, 0x55, 0x66, 255),
            ]
        );
        assert_eq!(shot.brush.open_palette_on_click, false);
        assert_eq!(shot.brush.close_palette_on_select, false);

        assert_eq!(shot.blur.strength, BlurStrength::High);

        assert_eq!(shot.magnifier.diameter, 256.);
        assert_eq!(shot.magnifier.pointer_gap, 24.);
        assert_eq!(shot.magnifier.initial_zoom, 4);
        assert_eq!(shot.magnifier.show_from, 4);
        assert_eq!(shot.magnifier.max_zoom, 16);
        assert_eq!(shot.magnifier.grid, false);
        assert_eq!(shot.magnifier.grid_from, 8);
        assert_eq!(shot.magnifier.cross_highlight, false);
        assert_eq!(
            shot.magnifier.cross_highlight_color,
            Color::from_rgba8_unpremul(255, 0, 0, 128)
        );

        // Arrow and rectangle are independent instances of the brush shape.
        assert_eq!(shot.arrow.width, 12);
        assert_eq!(
            shot.arrow.color,
            Color::from_rgba8_unpremul(0, 255, 128, 255)
        );
        assert_eq!(shot.arrow.palette, ScreenshotBrush::default().palette);
        assert_eq!(shot.rectangle.width, 3);
        assert_eq!(shot.rectangle.color, ScreenshotBrush::default().color);
        assert_eq!(shot.brush.width, 8);

        assert_eq!(
            shot.axes.color,
            Color::from_rgba8_unpremul(0x80, 0xc7, 0xff, 0x80)
        );
        assert_eq!(shot.axes.opacity, 0.7);
        assert_eq!(shot.axes.width, 2.);
        assert_eq!(shot.axes.dash_length, 10.);
        assert_eq!(shot.axes.dash_gap, 5.);
        assert_eq!(shot.axes.switch_threshold, 12.);
    }

    #[test]
    fn parse_screenshot_invalid() {
        for text in [
            // Cross-field invariants within one node.
            r#"screenshot { toolbar { icon-size 48; button-size 32; }; }"#,
            r#"screenshot { toolbar { opacity 0.5; drag-opacity 0.75; }; }"#,
            r#"screenshot { magnifier { initial-zoom 8; max-zoom 4; }; }"#,
            r#"screenshot { magnifier { show-from 8; max-zoom 4; }; }"#,
            // Zoom steps are powers of two; thresholds cannot be 1.
            r#"screenshot { magnifier { initial-zoom 3; }; }"#,
            r#"screenshot { magnifier { initial-zoom 64; }; }"#,
            r#"screenshot { magnifier { show-from 1; }; }"#,
            r#"screenshot { magnifier { grid-from 1; }; }"#,
            r#"screenshot { magnifier { max-zoom 1; }; }"#,
            // Strictly positive geometry.
            r#"screenshot { toolbar { button-size 0; }; }"#,
            r#"screenshot { toolbar { icon-size -1; }; }"#,
            r#"screenshot { tooltips { font-size 0; }; }"#,
            r#"screenshot { magnifier { diameter 0; }; }"#,
            // Non-negative geometry.
            r#"screenshot { toolbar { border-width -1; }; }"#,
            r#"screenshot { toolbar { margin top=-1; }; }"#,
            r#"screenshot { magnifier { pointer-gap -1; }; }"#,
            r#"screenshot { selection { border-width -1; }; }"#,
            // Opacities are in [0, 1].
            r#"screenshot { toolbar { opacity 1.5; }; }"#,
            r#"screenshot { selection { dim-opacity 2; }; }"#,
            // Brush width is limited to [1, 128].
            r#"screenshot { brush { width 0; }; }"#,
            r#"screenshot { brush { width 129; }; }"#,
            // Brush colors must be opaque.
            r##"screenshot { brush { color "#ff000080"; }; }"##,
            r##"screenshot { brush { palette "#ff0000" "#00ff0080"; }; }"##,
            // Palette has between 1 and 7 colors.
            r#"screenshot { brush { palette; }; }"#,
            r##"screenshot { brush { palette "#101010" "#202020" "#303030" "#404040" "#505050" "#606060" "#707070" "#808080"; }; }"##,
            // Unknown enum variants are rejected.
            r#"screenshot { toolbar { position "left"; }; }"#,
            r#"screenshot { toolbar { align "justify"; }; }"#,
            r#"screenshot { blur { strength "ultra"; }; }"#,
            // Arrow and rectangle reuse the brush limits.
            r#"screenshot { arrow { width 0; }; }"#,
            r#"screenshot { rectangle { width 129; }; }"#,
            r##"screenshot { rectangle { color "#ff000080"; }; }"##,
            // Axes opacity is in [0, 1]; the geometry fields are strictly positive.
            r#"screenshot { axes { opacity 1.5; }; }"#,
            r#"screenshot { axes { opacity -0.5; }; }"#,
            r#"screenshot { axes { width 0; }; }"#,
            r#"screenshot { axes { width -1; }; }"#,
            r#"screenshot { axes { dash-length 0; }; }"#,
            r#"screenshot { axes { dash-gap -1; }; }"#,
            r#"screenshot { axes { switch-threshold 0; }; }"#,
            r##"screenshot { axes { color "red"; }; }"##,
            // Duplicate sections within one file are rejected.
            "screenshot { blur { strength \"low\"; }; }\nscreenshot {}",
        ] {
            assert!(
                Config::parse_mem(text).is_err(),
                "expected parse error for: {text}"
            );
        }
    }

    #[test]
    fn parse_screenshot_include() {
        // A lower max-zoom in an included file is covered by a higher
        // initial-zoom merged over it, rather than producing an inverted
        // range.
        let dir = std::env::temp_dir().join(format!("niri-screenshot-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("inc.kdl"),
            "screenshot { magnifier { max-zoom 4; }; }",
        )
        .unwrap();

        let config = Config::parse(
            &dir.join("config.kdl"),
            "include \"inc.kdl\"\nscreenshot { magnifier { initial-zoom 8; }; }",
        )
        .config
        .map_err(miette::Report::new)
        .unwrap();

        assert_eq!(config.screenshot.magnifier.initial_zoom, 8);
        assert_eq!(config.screenshot.magnifier.max_zoom, 8);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parse_screenshot_format() {
        for (value, expected) in [
            ("png", niri_ipc::ScreenshotFormat::Png),
            ("webp", niri_ipc::ScreenshotFormat::Webp),
            ("avif", niri_ipc::ScreenshotFormat::Avif),
        ] {
            let config = do_parse(&format!(r#"screenshot-format "{value}""#));
            assert_eq!(config.screenshot_format, expected, "{value}");
        }
    }

    #[test]
    fn parse_screenshot_format_invalid() {
        for text in [
            // Unknown format.
            r#"screenshot-format "jpeg""#,
            r#"screenshot-format "WEBP""#,
            // Wrong argument type.
            "screenshot-format 1",
            // Missing argument.
            "screenshot-format",
            // Extra argument.
            r#"screenshot-format "png" "webp""#,
            // Duplicate node.
            "screenshot-format \"png\"\nscreenshot-format \"webp\"",
        ] {
            assert!(
                Config::parse_mem(text).is_err(),
                "expected parse error for: {text}"
            );
        }
    }
}
