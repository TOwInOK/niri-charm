use std::str::FromStr as _;

use knuffel::errors::DecodeError;

use crate::utils::{Flag, MergeWith};
use crate::{Color, FloatOrInt};

/// Screenshot UI settings.
///
/// Configured through the `screenshot {}` section. These values describe the
/// appearance and starting parameters of the screenshot editor; the current
/// tool, color, width and zoom of an open session live in the editor itself.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScreenshotConfig {
    pub toolbar: ScreenshotToolbar,
    pub tooltips: ScreenshotTooltips,
    pub selection: ScreenshotSelection,
    pub brush: ScreenshotBrush,
    /// Arrow tool brush settings; an independent instance of the same shape
    /// as `brush`, defaulting to the same values.
    pub arrow: ScreenshotBrush,
    /// Rectangle tool brush settings; an independent instance of the same
    /// shape as `brush`, defaulting to the same values.
    pub rectangle: ScreenshotBrush,
    pub blur: ScreenshotBlur,
    pub magnifier: ScreenshotMagnifier,
    pub axes: ScreenshotAxes,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ScreenshotConfigPart {
    #[knuffel(child)]
    pub toolbar: Option<ScreenshotToolbarPart>,
    #[knuffel(child)]
    pub tooltips: Option<ScreenshotTooltipsPart>,
    #[knuffel(child)]
    pub selection: Option<ScreenshotSelectionPart>,
    #[knuffel(child)]
    pub brush: Option<ScreenshotBrushPart>,
    #[knuffel(child)]
    pub arrow: Option<ScreenshotBrushPart>,
    #[knuffel(child)]
    pub rectangle: Option<ScreenshotBrushPart>,
    #[knuffel(child)]
    pub blur: Option<ScreenshotBlurPart>,
    #[knuffel(child)]
    pub magnifier: Option<ScreenshotMagnifierPart>,
    #[knuffel(child)]
    pub axes: Option<ScreenshotAxesPart>,
}

impl ScreenshotConfigPart {
    /// Checks same-node violations of cross-field invariants.
    ///
    /// Cross-file merges cannot be checked here and are resolved by clamping
    /// in the `MergeWith` impls instead.
    pub fn validate<S>(
        &self,
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) where
        S: knuffel::traits::ErrorSpan,
    {
        if let Some(toolbar) = &self.toolbar {
            if let (Some(icon), Some(button)) = (toolbar.icon_size, toolbar.button_size) {
                if icon.0 > button.0 {
                    ctx.emit_error(DecodeError::unexpected(
                        &node.node_name,
                        "node",
                        "icon-size must be <= button-size",
                    ));
                }
            }

            if let (Some(drag), Some(opacity)) = (toolbar.drag_opacity, toolbar.opacity) {
                if drag.0 > opacity.0 {
                    ctx.emit_error(DecodeError::unexpected(
                        &node.node_name,
                        "node",
                        "drag-opacity must be <= opacity",
                    ));
                }
            }
        }

        if let Some(magnifier) = &self.magnifier {
            if let Some(max) = magnifier.max_zoom {
                for (name, zoom) in [
                    ("initial-zoom", magnifier.initial_zoom.map(|z| z.factor())),
                    ("show-from", magnifier.show_from.map(|z| z.factor())),
                ] {
                    if let Some(zoom) = zoom {
                        if zoom > max.factor() {
                            ctx.emit_error(DecodeError::unexpected(
                                &node.node_name,
                                "node",
                                format!("{name} must be <= max-zoom"),
                            ));
                        }
                    }
                }
            }
        }
    }
}

impl MergeWith<ScreenshotConfigPart> for ScreenshotConfig {
    fn merge_with(&mut self, part: &ScreenshotConfigPart) {
        merge!(
            (self, part),
            toolbar,
            tooltips,
            selection,
            brush,
            arrow,
            rectangle,
            blur,
            magnifier,
            axes
        );
    }
}

/// The screen edge the toolbar sticks to.
#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotToolbarPosition {
    #[default]
    Top,
    Bottom,
}

/// Toolbar alignment along its screen edge.
#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotToolbarAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// Independent minimum offsets from the four screen edges, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotMargin {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Default for ScreenshotMargin {
    fn default() -> Self {
        Self {
            top: 8.,
            right: 8.,
            bottom: 8.,
            left: 8.,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenshotMarginPart {
    #[knuffel(property)]
    pub top: Option<NonNegative>,
    #[knuffel(property)]
    pub right: Option<NonNegative>,
    #[knuffel(property)]
    pub bottom: Option<NonNegative>,
    #[knuffel(property)]
    pub left: Option<NonNegative>,
}

impl MergeWith<ScreenshotMarginPart> for ScreenshotMargin {
    fn merge_with(&mut self, part: &ScreenshotMarginPart) {
        merge!((self, part), top, right, bottom, left);
    }
}

/// Toolbar appearance and geometry. Sizes are in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotToolbar {
    pub position: ScreenshotToolbarPosition,
    pub align: ScreenshotToolbarAlign,
    pub margin: ScreenshotMargin,
    pub button_size: f64,
    pub icon_size: f64,
    pub background_color: Color,
    pub border_color: Color,
    pub border_width: f64,
    pub accent_color: Color,
    pub icon_color: Color,
    pub disabled_icon_color: Color,
    pub hover_color: Color,
    pub pressed_color: Color,
    /// Opacity of the whole panel in [0, 1].
    pub opacity: f64,
    /// Absolute opacity of the panel during gestures, in [0, 1].
    ///
    /// Not a multiplier of `opacity`. Also `<= opacity`.
    pub drag_opacity: f64,
}

impl Default for ScreenshotToolbar {
    fn default() -> Self {
        Self {
            position: ScreenshotToolbarPosition::default(),
            align: ScreenshotToolbarAlign::default(),
            margin: ScreenshotMargin::default(),
            button_size: 40.,
            icon_size: 24.,
            background_color: Color::from_rgba8_unpremul(0x12, 0x12, 0x1a, 0xff),
            border_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0x29),
            border_width: 1.,
            accent_color: Color::from_rgba8_unpremul(0x80, 0xc7, 0xff, 0xff),
            icon_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0xeb),
            disabled_icon_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0x4d),
            hover_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0x1f),
            pressed_color: Color::from_rgba8_unpremul(0x00, 0x00, 0x00, 0x59),
            opacity: 1.,
            drag_opacity: 0.5,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ScreenshotToolbarPart {
    #[knuffel(child, unwrap(argument))]
    pub position: Option<ScreenshotToolbarPosition>,
    #[knuffel(child, unwrap(argument))]
    pub align: Option<ScreenshotToolbarAlign>,
    #[knuffel(child)]
    pub margin: Option<ScreenshotMarginPart>,
    #[knuffel(child, unwrap(argument))]
    pub button_size: Option<Positive>,
    #[knuffel(child, unwrap(argument))]
    pub icon_size: Option<Positive>,
    #[knuffel(child)]
    pub background_color: Option<Color>,
    #[knuffel(child)]
    pub border_color: Option<Color>,
    #[knuffel(child, unwrap(argument))]
    pub border_width: Option<NonNegative>,
    #[knuffel(child)]
    pub accent_color: Option<Color>,
    #[knuffel(child)]
    pub icon_color: Option<Color>,
    #[knuffel(child)]
    pub disabled_icon_color: Option<Color>,
    #[knuffel(child)]
    pub hover_color: Option<Color>,
    #[knuffel(child)]
    pub pressed_color: Option<Color>,
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub drag_opacity: Option<FloatOrInt<0, 1>>,
}

impl MergeWith<ScreenshotToolbarPart> for ScreenshotToolbar {
    fn merge_with(&mut self, part: &ScreenshotToolbarPart) {
        merge!(
            (self, part),
            margin,
            button_size,
            icon_size,
            border_width,
            opacity,
            drag_opacity
        );
        merge_clone!(
            (self, part),
            position,
            align,
            background_color,
            border_color,
            accent_color,
            icon_color,
            disabled_icon_color,
            hover_color,
            pressed_color
        );

        // The button must cover the icon, and the normal opacity must cover
        // the drag opacity: merging a higher icon-size or drag-opacity over a
        // lower button-size or opacity raises the upper bound instead of
        // producing an inverted range. Same-node violations are rejected at
        // decode time.
        self.button_size = self.button_size.max(self.icon_size);
        self.opacity = self.opacity.max(self.drag_opacity);
    }
}

/// Hover/focus tooltip settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotTooltips {
    pub enabled: bool,
    pub font: String,
    pub font_size: f64,
}

impl Default for ScreenshotTooltips {
    fn default() -> Self {
        Self {
            enabled: true,
            font: String::from("Sans"),
            font_size: 11.,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ScreenshotTooltipsPart {
    #[knuffel(child)]
    pub enabled: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub font: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub font_size: Option<Positive>,
}

impl MergeWith<ScreenshotTooltipsPart> for ScreenshotTooltips {
    fn merge_with(&mut self, part: &ScreenshotTooltipsPart) {
        merge!((self, part), enabled, font_size);
        merge_clone!((self, part), font);
    }
}

/// Selection border and dimming of the frozen frame outside the selection.
///
/// Widths are in pixels of the captured frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotSelection {
    pub border_color: Color,
    pub border_width: f64,
    pub dim_color: Color,
    /// Multiplier applied to `dim_color`'s alpha, in [0, 1].
    pub dim_opacity: f64,
    /// Whether the next session restores the previous selection.
    pub remember_selection: bool,
}

impl Default for ScreenshotSelection {
    fn default() -> Self {
        Self {
            border_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0xff),
            border_width: 2.,
            dim_color: Color::from_rgba8_unpremul(0x00, 0x00, 0x00, 0xff),
            dim_opacity: 0.5,
            remember_selection: true,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenshotSelectionPart {
    #[knuffel(child)]
    pub border_color: Option<Color>,
    #[knuffel(child, unwrap(argument))]
    pub border_width: Option<NonNegative>,
    #[knuffel(child)]
    pub dim_color: Option<Color>,
    #[knuffel(child, unwrap(argument))]
    pub dim_opacity: Option<FloatOrInt<0, 1>>,
    #[knuffel(child)]
    pub remember_selection: Option<Flag>,
}

impl MergeWith<ScreenshotSelectionPart> for ScreenshotSelection {
    fn merge_with(&mut self, part: &ScreenshotSelectionPart) {
        merge!((self, part), border_width, dim_opacity, remember_selection);
        merge_clone!((self, part), border_color, dim_color);
    }
}

/// Brush appearance and palette. Widths are in pixels of the captured frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotBrush {
    /// Width of new strokes in pixels, in [1, 128].
    pub width: u32,
    /// Opaque color of new strokes.
    pub color: Color,
    /// Palette of 1 to 7 opaque colors; an entry's number is its 1-based
    /// position in the list.
    pub palette: Vec<Color>,
    /// Whether activating the brush button toggles the palette.
    pub open_palette_on_click: bool,
    pub close_palette_on_select: bool,
}

impl Default for ScreenshotBrush {
    fn default() -> Self {
        Self {
            width: 4,
            color: Color::from_rgba8_unpremul(0xff, 0x00, 0x00, 0xff),
            palette: vec![
                Color::from_rgba8_unpremul(0xff, 0x00, 0x00, 0xff),
                Color::from_rgba8_unpremul(0xff, 0x80, 0x00, 0xff),
                Color::from_rgba8_unpremul(0xff, 0xe0, 0x00, 0xff),
                Color::from_rgba8_unpremul(0x00, 0xc0, 0x40, 0xff),
                Color::from_rgba8_unpremul(0x00, 0x80, 0xff, 0xff),
                Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0xff),
                Color::from_rgba8_unpremul(0x00, 0x00, 0x00, 0xff),
            ],
            open_palette_on_click: true,
            close_palette_on_select: true,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct ScreenshotBrushPart {
    #[knuffel(child, unwrap(argument))]
    pub width: Option<BrushWidth>,
    #[knuffel(child, unwrap(argument))]
    pub color: Option<OpaqueColor>,
    #[knuffel(child)]
    pub palette: Option<BrushPalette>,
    #[knuffel(child)]
    pub open_palette_on_click: Option<Flag>,
    #[knuffel(child)]
    pub close_palette_on_select: Option<Flag>,
}

impl MergeWith<ScreenshotBrushPart> for ScreenshotBrush {
    fn merge_with(&mut self, part: &ScreenshotBrushPart) {
        merge!(
            (self, part),
            width,
            color,
            palette,
            open_palette_on_click,
            close_palette_on_select
        );
    }
}

/// Blur tool strength preset.
#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BlurStrength {
    Low,
    #[default]
    Medium,
    High,
}

/// Blur tool settings. The strength preset is stored per blur operation.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenshotBlur {
    pub strength: BlurStrength,
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenshotBlurPart {
    #[knuffel(child, unwrap(argument))]
    pub strength: Option<BlurStrength>,
}

impl MergeWith<ScreenshotBlurPart> for ScreenshotBlur {
    fn merge_with(&mut self, part: &ScreenshotBlurPart) {
        merge_clone!((self, part), strength);
    }
}

/// Magnifier settings. Zoom levels are powers of two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotMagnifier {
    /// Lens diameter in logical pixels; strictly positive.
    pub diameter: f64,
    /// Inward border width in logical pixels; zero disables the border.
    pub border_width: f64,
    pub border_color: Color,
    pub grid_color: Color,
    /// Offset between the pointer and the lens in logical pixels.
    pub pointer_gap: f64,
    /// Zoom level at every session start, one of 1, 2, 4, 8, 16, 32.
    pub initial_zoom: u32,
    /// The lens appears at this zoom level and above: one of 2, 4, 8, 16, 32.
    pub show_from: u32,
    /// Upper zoom bound: one of 2, 4, 8, 16, 32.
    pub max_zoom: u32,
    /// Whether to draw the pixel grid.
    pub grid: bool,
    /// Zoom level at which the grid appears: one of 2, 4, 8, 16, 32.
    ///
    /// May exceed `max_zoom`, in which case the grid never appears until the
    /// maximum is raised.
    pub grid_from: u32,
    /// Whether to highlight the row and column of the center pixel.
    pub cross_highlight: bool,
    pub cross_highlight_color: Color,
}

impl Default for ScreenshotMagnifier {
    fn default() -> Self {
        Self {
            diameter: 192.,
            border_width: 2.,
            border_color: Color::from_rgba8_unpremul(255, 255, 255, 255),
            grid_color: Color::from_rgba8_unpremul(0, 0, 0, 140),
            pointer_gap: 16.,
            initial_zoom: 1,
            show_from: 2,
            max_zoom: 32,
            grid: true,
            grid_from: 4,
            cross_highlight: true,
            cross_highlight_color: Color::from_rgba8_unpremul(0xa6, 0xd9, 0xf2, 0x59),
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenshotMagnifierPart {
    #[knuffel(child, unwrap(argument))]
    pub diameter: Option<Positive>,
    #[knuffel(child, unwrap(argument))]
    pub border_width: Option<NonNegative>,
    #[knuffel(child)]
    pub border_color: Option<Color>,
    #[knuffel(child)]
    pub grid_color: Option<Color>,
    #[knuffel(child, unwrap(argument))]
    pub pointer_gap: Option<NonNegative>,
    #[knuffel(child, unwrap(argument))]
    pub initial_zoom: Option<MagnifierZoomStep<0>>,
    #[knuffel(child, unwrap(argument))]
    pub show_from: Option<MagnifierZoomStep<1>>,
    #[knuffel(child, unwrap(argument))]
    pub max_zoom: Option<MagnifierZoomStep<1>>,
    #[knuffel(child)]
    pub grid: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub grid_from: Option<MagnifierZoomStep<1>>,
    #[knuffel(child)]
    pub cross_highlight: Option<Flag>,
    #[knuffel(child)]
    pub cross_highlight_color: Option<Color>,
}

impl MergeWith<ScreenshotMagnifierPart> for ScreenshotMagnifier {
    fn merge_with(&mut self, part: &ScreenshotMagnifierPart) {
        merge!(
            (self, part),
            diameter,
            border_width,
            pointer_gap,
            initial_zoom,
            show_from,
            max_zoom,
            grid,
            grid_from,
            cross_highlight
        );
        merge_clone!(
            (self, part),
            cross_highlight_color,
            border_color,
            grid_color
        );

        // The maximum must cover the starting and visibility thresholds: a
        // config that merges a higher initial-zoom or show-from over a lower
        // max-zoom raises the maximum instead of producing an inverted range.
        // Same-node violations are rejected at decode time.
        self.max_zoom = self.max_zoom.max(self.initial_zoom).max(self.show_from);
    }
}

/// Axis guides shown while a drawing tool constrains a stroke.
///
/// Lengths are in pixels of the captured frame; the guide is drawn as a
/// dashed line through the drag origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenshotAxes {
    /// Color of the guide lines. Alpha compositing comes from `opacity`, but
    /// the stored color may itself carry alpha.
    pub color: Color,
    /// Multiplier applied to `color`'s alpha, in [0, 1].
    pub opacity: f64,
    /// Guide line width in pixels; strictly positive.
    pub width: f64,
    /// Length of a dash in pixels; strictly positive.
    pub dash_length: f64,
    /// Gap between dashes in pixels; strictly positive.
    pub dash_gap: f64,
    /// Pointer distance in pixels that switches a stroke between unconstrained
    /// and axis-constrained; strictly positive.
    pub switch_threshold: f64,
}

impl Default for ScreenshotAxes {
    fn default() -> Self {
        Self {
            color: Color::from_rgba8_unpremul(0x80, 0xc7, 0xff, 0xff),
            opacity: 0.4,
            width: 1.,
            dash_length: 6.,
            dash_gap: 4.,
            switch_threshold: 8.,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenshotAxesPart {
    #[knuffel(child, unwrap(argument))]
    pub color: Option<HexColor>,
    #[knuffel(child, unwrap(argument))]
    pub opacity: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub width: Option<Positive>,
    #[knuffel(child, unwrap(argument))]
    pub dash_length: Option<Positive>,
    #[knuffel(child, unwrap(argument))]
    pub dash_gap: Option<Positive>,
    #[knuffel(child, unwrap(argument))]
    pub switch_threshold: Option<Positive>,
}

impl MergeWith<ScreenshotAxesPart> for ScreenshotAxes {
    fn merge_with(&mut self, part: &ScreenshotAxesPart) {
        merge!(
            (self, part),
            color,
            opacity,
            width,
            dash_length,
            dash_gap,
            switch_threshold
        );
    }
}

/// A finite number strictly greater than 0.
///
/// Used for sizes that must stay non-degenerate, such as button size or font
/// size, so unlike [`FloatOrInt`] the lower bound is exclusive.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Positive(pub f64);

impl MergeWith<Positive> for f64 {
    fn merge_with(&mut self, part: &Positive) {
        *self = part.0;
    }
}

/// A finite number greater than or equal to 0.
///
/// Used for offsets and widths that may legitimately vanish, such as margins
/// and border widths.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct NonNegative(pub f64);

impl MergeWith<NonNegative> for f64 {
    fn merge_with(&mut self, part: &NonNegative) {
        *self = part.0;
    }
}

fn decode_number<S: knuffel::traits::ErrorSpan>(
    val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
    ctx: &mut knuffel::decode::Context<S>,
) -> Option<f64> {
    match &**val {
        knuffel::ast::Literal::Int(value) => match i32::try_from(value) {
            Ok(v) => Some(f64::from(v)),
            Err(e) => {
                ctx.emit_error(DecodeError::conversion(val, e));
                None
            }
        },
        knuffel::ast::Literal::Decimal(value) => match f64::try_from(value) {
            Ok(v) => Some(v),
            Err(e) => {
                ctx.emit_error(DecodeError::conversion(val, e));
                None
            }
        },
        _ => {
            ctx.emit_error(DecodeError::unsupported(
                val,
                "Unsupported value, only numbers are recognized",
            ));
            None
        }
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for Positive {
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
        let Some(value) = decode_number(val, ctx) else {
            return Ok(Self::default());
        };

        if value.is_finite() && value > 0. {
            Ok(Positive(value))
        } else {
            ctx.emit_error(DecodeError::conversion(val, "value must be greater than 0"));
            Ok(Self::default())
        }
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for NonNegative {
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
        let Some(value) = decode_number(val, ctx) else {
            return Ok(Self::default());
        };

        if value.is_finite() && value >= 0. {
            Ok(NonNegative(value))
        } else {
            ctx.emit_error(DecodeError::conversion(
                val,
                "value must be greater than or equal to 0",
            ));
            Ok(Self::default())
        }
    }
}

/// Brush stroke width: an integer number of captured-frame pixels in [1, 128].
///
/// Integer-only to match the `screenshot-context` action contract, where widths are
/// counted in whole pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct BrushWidth(pub u32);

impl MergeWith<BrushWidth> for u32 {
    fn merge_with(&mut self, part: &BrushWidth) {
        *self = part.0;
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for BrushWidth {
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
        match &**val {
            knuffel::ast::Literal::Int(value) => match u32::try_from(value) {
                Ok(v) if (1..=128).contains(&v) => Ok(BrushWidth(v)),
                Ok(_) => {
                    ctx.emit_error(DecodeError::conversion(
                        val,
                        "value must be between 1 and 128",
                    ));
                    Ok(Self::default())
                }
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    Ok(Self::default())
                }
            },
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only integers are recognized",
                ));
                Ok(Self::default())
            }
        }
    }
}

/// A magnifier zoom step: a power of two between `1 << MIN_EXP` and 32.
///
/// `MIN_EXP` is the minimum allowed binary exponent: `MagnifierZoomStep<0>`
/// accepts 1, 2, 4, 8, 16, 32 and `MagnifierZoomStep<1>` accepts 2, 4, 8, 16,
/// 32.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MagnifierZoomStep<const MIN_EXP: u8>(pub u32);

impl<const MIN_EXP: u8> MagnifierZoomStep<MIN_EXP> {
    /// The zoom factor.
    pub fn factor(self) -> u32 {
        self.0
    }
}

impl<const MIN_EXP: u8> MergeWith<MagnifierZoomStep<MIN_EXP>> for u32 {
    fn merge_with(&mut self, part: &MagnifierZoomStep<MIN_EXP>) {
        *self = part.0;
    }
}

impl<S: knuffel::traits::ErrorSpan, const MIN_EXP: u8> knuffel::DecodeScalar<S>
    for MagnifierZoomStep<MIN_EXP>
{
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
        let Some(value) = decode_number(val, ctx) else {
            return Ok(Self::default());
        };

        let min = f64::from(1u32 << MIN_EXP);
        if value.fract() == 0. && value >= min && value <= 32. && (value as u32).is_power_of_two() {
            Ok(MagnifierZoomStep(value as u32))
        } else {
            ctx.emit_error(DecodeError::conversion(
                val,
                format!(
                    "value must be a power of two between {} and 32",
                    1u32 << MIN_EXP
                ),
            ));
            Ok(Self::default())
        }
    }
}

/// An opaque color: a string color value with alpha equal to 1.
///
/// Brush strokes have no alpha in this design, so semi-transparent values are
/// rejected rather than silently composited.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct OpaqueColor(pub Color);

impl MergeWith<OpaqueColor> for Color {
    fn merge_with(&mut self, part: &OpaqueColor) {
        *self = part.0;
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for OpaqueColor {
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
        match &**val {
            knuffel::ast::Literal::String(s) => {
                let color = Color::from_str(s).map_err(|e| DecodeError::conversion(val, e))?;
                if color.a == 1. {
                    Ok(OpaqueColor(color))
                } else {
                    ctx.emit_error(DecodeError::conversion(
                        val,
                        "brush colors must be fully opaque",
                    ));
                    Ok(Self::default())
                }
            }
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only strings are recognized",
                ));
                Ok(Self::default())
            }
        }
    }
}

/// A color written in `#`-hex form, alpha allowed.
///
/// Guides and highlights accept translucent colors but only in hex notation:
/// named CSS colors are rejected so screenshot config colors stay consistent
/// with the brush palette and the `#RRGGBB[AA]` IPC form.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct HexColor(pub Color);

impl MergeWith<HexColor> for Color {
    fn merge_with(&mut self, part: &HexColor) {
        *self = part.0;
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for HexColor {
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
        match &**val {
            knuffel::ast::Literal::String(s) if s.starts_with('#') => {
                let color = Color::from_str(s).map_err(|e| DecodeError::conversion(val, e))?;
                Ok(HexColor(color))
            }
            knuffel::ast::Literal::String(_) => {
                ctx.emit_error(DecodeError::conversion(
                    val,
                    "color must be in #RRGGBB or #RRGGBBAA form",
                ));
                Ok(Self::default())
            }
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only strings are recognized",
                ));
                Ok(Self::default())
            }
        }
    }
}

/// The brush palette argument list: between 1 and 7 opaque colors.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushPalette(pub Vec<Color>);

impl MergeWith<BrushPalette> for Vec<Color> {
    fn merge_with(&mut self, part: &BrushPalette) {
        self.clone_from(&part.0);
    }
}

impl<S> knuffel::Decode<S> for BrushPalette
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        // Check for unexpected type name.
        if let Some(type_name) = &node.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        let colors = node
            .arguments
            .iter()
            .map(|val| {
                let color: OpaqueColor = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                Ok(color.0)
            })
            .collect::<Result<Vec<Color>, DecodeError<S>>>()?;

        if colors.is_empty() {
            return Err(DecodeError::missing(node, "at least one color is required"));
        }
        if colors.len() > 7 {
            return Err(DecodeError::unexpected(
                node,
                "node",
                "at most 7 palette colors are supported",
            ));
        }

        // Check for unexpected properties and children.
        for name in node.properties.keys() {
            ctx.emit_error(DecodeError::unexpected(
                name,
                "property",
                format!("unexpected property `{}`", name.escape_default()),
            ));
        }
        for child in node.children.as_ref().map(|lst| &lst[..]).unwrap_or(&[]) {
            ctx.emit_error(DecodeError::unexpected(
                child,
                "node",
                format!("unexpected node `{}`", child.node_name.escape_default()),
            ));
        }

        Ok(Self(colors))
    }
}
