use std::cell::RefCell;

use anyhow::Result;
use niri_config::screenshot::ScreenshotAxes;
use pangocairo::cairo::{self, ImageSurface};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::utils::{Logical, Physical, Point, Rectangle, Size, Transform};

use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};

/// Thin reusable strips avoid uploading an output-sized image on pointer motion.
pub struct Guides {
    size: Size<i32, Physical>,
    scale: f64,
    revision: u64,
    thickness: i32,
    horizontal: TextureBuffer<GlesTexture>,
    vertical: TextureBuffer<GlesTexture>,
    brush: Option<(u32, TextureBuffer<GlesTexture>)>,
}

fn upload(
    renderer: &mut GlesRenderer,
    mut surface: ImageSurface,
    scale: f64,
) -> Result<TextureBuffer<GlesTexture>> {
    let size = (surface.width(), surface.height());
    let data = surface.data()?;
    Ok(TextureBuffer::from_memory(
        renderer,
        &data,
        Fourcc::Argb8888,
        size,
        false,
        scale,
        Transform::Normal,
        Vec::new(),
    )?)
}

impl Guides {
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        cache: &RefCell<Option<Self>>,
        renderer: &mut GlesRenderer,
        settings: &ScreenshotAxes,
        revision: u64,
        size: Size<i32, Physical>,
        scale: f64,
        center: Option<Point<i32, Physical>>,
        pen: Point<i32, Physical>,
        brush_width: Option<u32>,
        push: &mut dyn FnMut(PrimaryGpuTextureRenderElement),
    ) -> Result<()> {
        let mut cache = cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|c| c.size != size || c.scale != scale || c.revision != revision)
        {
            // Cap raster dimensions to the output even for very large configured widths.
            let thickness = (settings.width * scale)
                .ceil()
                .clamp(1., f64::from(size.w.min(size.h))) as i32;
            let strip = |horizontal: bool| -> Result<ImageSurface> {
                let (w, h) = if horizontal {
                    (size.w, thickness)
                } else {
                    (thickness, size.h)
                };
                let surface = ImageSurface::create(cairo::Format::ARgb32, w, h)?;
                let cr = cairo::Context::new(&surface)?;
                let c = settings.color;
                cr.set_source_rgba(
                    f64::from(c.r),
                    f64::from(c.g),
                    f64::from(c.b),
                    f64::from(c.a) * settings.opacity,
                );
                cr.set_line_width(settings.width * scale);
                cr.set_dash(
                    &[settings.dash_length * scale, settings.dash_gap * scale],
                    0.,
                );
                if horizontal {
                    cr.move_to(0., f64::from(h) / 2.);
                    cr.line_to(f64::from(w), f64::from(h) / 2.);
                } else {
                    cr.move_to(f64::from(w) / 2., 0.);
                    cr.line_to(f64::from(w) / 2., f64::from(h));
                }
                cr.stroke()?;
                Ok(surface)
            };
            *cache = Some(Self {
                size,
                scale,
                revision,
                thickness,
                horizontal: upload(renderer, strip(true)?, scale)?,
                vertical: upload(renderer, strip(false)?, scale)?,
                brush: None,
            });
        }
        let cache = cache.as_mut().unwrap();
        if let Some(width) = brush_width {
            if cache.brush.as_ref().is_none_or(|(w, _)| *w != width) {
                let d = width as i32 + 4;
                let surface = ImageSurface::create(cairo::Format::ARgb32, d, d)?;
                let cr = cairo::Context::new(&surface)?;
                let middle = f64::from(d) / 2.;
                cr.arc(
                    middle,
                    middle,
                    f64::from(width) / 2.,
                    0.,
                    std::f64::consts::TAU,
                );
                cr.set_source_rgba(0., 0., 0., 0.8);
                cr.set_line_width(3.);
                cr.stroke_preserve()?;
                cr.set_source_rgba(1., 1., 1., 0.9);
                cr.set_line_width(1.);
                cr.stroke()?;
                drop(cr);
                cache.brush = Some((width, upload(renderer, surface, scale)?));
            }
            let (_, buffer) = cache.brush.as_ref().unwrap();
            let d = f64::from(width + 4);
            push(PrimaryGpuTextureRenderElement(
                TextureRenderElement::from_texture_buffer(
                    buffer.clone(),
                    Point::<f64, Physical>::from((
                        f64::from(pen.x) - d / 2.,
                        f64::from(pen.y) - d / 2.,
                    ))
                    .to_logical(scale),
                    1.,
                    None,
                    None,
                    Kind::Unspecified,
                ),
            ));
        }
        if let Some(center) = center {
            let half = f64::from(cache.thickness) / 2.;
            let y = (f64::from(center.y) - half).clamp(0., f64::from(size.h - cache.thickness));
            let x = (f64::from(center.x) - half).clamp(0., f64::from(size.w - cache.thickness));
            push(PrimaryGpuTextureRenderElement(
                TextureRenderElement::from_texture_buffer(
                    cache.horizontal.clone(),
                    Point::<f64, Physical>::from((0., y)).to_logical(scale),
                    1.,
                    None,
                    None,
                    Kind::Unspecified,
                ),
            ));
            // Split at the horizontal strip so the intersection is not double-blended.
            for (start, end) in [(0., y), (y + f64::from(cache.thickness), f64::from(size.h))] {
                if end <= start {
                    continue;
                }
                let source = Rectangle::<f64, Logical>::new(
                    (0., start / scale).into(),
                    (f64::from(cache.thickness) / scale, (end - start) / scale).into(),
                );
                push(PrimaryGpuTextureRenderElement(
                    TextureRenderElement::from_texture_buffer(
                        cache.vertical.clone(),
                        Point::<f64, Physical>::from((x, start)).to_logical(scale),
                        1.,
                        Some(source),
                        Some(source.size),
                        Kind::Unspecified,
                    ),
                ));
            }
        }
        Ok(())
    }
}
