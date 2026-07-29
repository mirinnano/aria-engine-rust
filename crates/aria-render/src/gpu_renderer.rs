//! Concrete wgpu submission for the platform-neutral scene protocol.
//!
//! This module owns no window, filesystem, browser API, or audio device. The
//! host provides an `ImageResolver`; Native and Web can therefore submit the
//! same ordered Core frame through their own asset transport.

use std::collections::BTreeMap;

use aria_core::protocol::{
    BlendMode, Color, DrawCommand, DrawStyle, GradientStyle, Rect, SceneFrame, ScreenEffect,
    SpriteFit, TextAlign, TextDecoration, TransitionKind,
};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Color as GlyphColor, Family, Metrics, Shaping, TextArea, TextBounds,
    cosmic_text::Align,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::gpu_text::{BundledFont, BundledFontError, GpuTextLayer};
use crate::{SafeAreaInsets, ViewportTransform};

const WHITE_TEXTURE_KEY: &str = "__aria_internal_white";

/// Decoded RGBA image data supplied by a platform-specific asset transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RasterImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, WgpuRendererError> {
        let expected = usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| WgpuRendererError::InvalidImageSize { width, height })?;
        if width == 0 || height == 0 || rgba.len() != expected {
            return Err(WgpuRendererError::InvalidImageSize { width, height });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}

/// Resolves a logical sprite path to RGBA pixels without exposing platform
/// handles to `aria-core`.
pub trait ImageResolver {
    fn load_image(
        &mut self,
        logical_path: &str,
        desired_size: Option<(u32, u32)>,
    ) -> Result<RasterImage, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSurfaceSize {
    pub width: u32,
    pub height: u32,
    /// Winit/browser DPI scale. It affects input hit targets but not the
    /// logical game coordinate system.
    pub scale_factor_milli: u32,
}

impl RenderSurfaceSize {
    #[must_use]
    pub fn new(width: u32, height: u32, scale_factor: f64) -> Self {
        let scale_factor_milli = (scale_factor.clamp(0.1, 16.0) * 1_000.0).round() as u32;
        Self {
            width,
            height,
            scale_factor_milli,
        }
    }

    #[must_use]
    pub fn scale_factor(self) -> f32 {
        self.scale_factor_milli.max(1) as f32 / 1_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSubmission {
    pub quads: usize,
    pub text_areas: usize,
    pub texture_uploads: usize,
}

#[derive(Debug, Error)]
pub enum WgpuRendererError {
    #[error("image dimensions or RGBA byte length are invalid for {width}x{height}")]
    InvalidImageSize { width: u32, height: u32 },
    #[error("cannot load sprite asset '{asset}': {message}")]
    Asset { asset: String, message: String },
    #[error("this frame contains text but the project declares no bundled runtime.fonts assets")]
    NoBundledFont,
    #[error(transparent)]
    BundledFont(#[from] BundledFontError),
    #[error("glyph preparation failed: {0}")]
    TextPrepare(String),
    #[error("glyph render failed: {0}")]
    TextRender(String),
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// Physical-pixel extent, corner radius, and soft-edge width used by the
    /// fragment SDF. Keeping these per-vertex makes every draw command a
    /// complete value object with no host-side shape reconstruction.
    sdf: [f32; 4],
    border_width: f32,
    shape_enabled: f32,
}

#[derive(Debug)]
struct GpuTexture {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct QuadPlan {
    vertices: [QuadVertex; 6],
    texture_key: String,
    blend: BlendMode,
    clip: Option<Rect>,
}

#[derive(Debug)]
struct TextPlan {
    text: String,
    bounds: Rect,
    color: Color,
    font_size: f32,
    style: DrawStyle,
}

/// A small, order-preserving 2D renderer for `SceneFrame`.
///
/// Each draw command remains in Core-provided z/id order. The implementation
/// intentionally favors correctness and a clear adapter boundary over an
/// early instance-buffer optimization; batching can replace the per-quad
/// buffer allocation without changing its public contract.
#[derive(Debug)]
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture_layout: wgpu::BindGroupLayout,
    alpha_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    multiply_pipeline: wgpu::RenderPipeline,
    textures: BTreeMap<String, GpuTexture>,
    text: GpuTextLayer,
    format: wgpu::TextureFormat,
}

impl WgpuRenderer {
    #[must_use]
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::new_with_fonts(device, queue, format, Vec::new())
            .expect("an empty bundled-font configuration is always valid")
    }

    /// Creates a renderer whose text system may shape only the supplied font
    /// bytes. This is the Native Player's Windows/Linux text contract.
    pub fn new_with_fonts(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        fonts: Vec<BundledFont>,
    ) -> Result<Self, WgpuRendererError> {
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria-2d-texture-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aria-2d-shader"),
            source: wgpu::ShaderSource::Wgsl(QUAD_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aria-2d-pipeline-layout"),
            bind_group_layouts: &[Some(&texture_layout)],
            immediate_size: 0,
        });
        let alpha_pipeline = create_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            "aria-2d-alpha",
            wgpu::BlendState::ALPHA_BLENDING,
        );
        let add_pipeline = create_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            "aria-2d-add",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
        );
        let multiply_pipeline = create_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            "aria-2d-multiply",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            },
        );
        let text = GpuTextLayer::new(&device, &queue, format, fonts)?;
        let white = create_texture(
            &device,
            &queue,
            &texture_layout,
            WHITE_TEXTURE_KEY,
            &RasterImage::new(1, 1, vec![255, 255, 255, 255])
                .expect("the built-in white texture is valid"),
        );
        let mut textures = BTreeMap::new();
        textures.insert(WHITE_TEXTURE_KEY.to_owned(), white);
        Ok(Self {
            device,
            queue,
            texture_layout,
            alpha_pipeline,
            add_pipeline,
            multiply_pipeline,
            textures,
            text,
            format,
        })
    }

    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Device backing this renderer. Hosts use it only to configure a surface
    /// that was created outside the renderer; Core-facing rendering remains
    /// fully platform-neutral.
    #[must_use]
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Presents a surface texture after [`Self::render`] submitted its work.
    pub fn present(&self, surface_texture: wgpu::SurfaceTexture) {
        self.queue.present(surface_texture);
    }

    /// Drops device-local texture state after surface/device recreation.
    /// Hosts recreate the renderer with their new device and then submit the
    /// current deterministic frame, causing the resolver to repopulate it.
    pub fn clear_asset_cache(&mut self) {
        self.textures.retain(|key, _| key == WHITE_TEXTURE_KEY);
        self.text.trim();
    }

    #[must_use]
    pub fn viewport_transform(
        &self,
        frame: &SceneFrame,
        surface: RenderSurfaceSize,
    ) -> ViewportTransform {
        ViewportTransform::fit(
            frame.logical_size,
            surface.width.max(1),
            surface.height.max(1),
            surface.scale_factor(),
            SafeAreaInsets {
                top: frame.viewport.safe_area.top * frame.viewport.scale_factor,
                right: frame.viewport.safe_area.right * frame.viewport.scale_factor,
                bottom: frame.viewport.safe_area.bottom * frame.viewport.scale_factor,
                left: frame.viewport.safe_area.left * frame.viewport.scale_factor,
            },
        )
    }

    /// Encodes and submits one complete ordered Core frame to `target`.
    pub fn render(
        &mut self,
        frame: &SceneFrame,
        target: &wgpu::TextureView,
        surface: RenderSurfaceSize,
        resolver: &mut dyn ImageResolver,
    ) -> Result<RenderSubmission, WgpuRendererError> {
        if surface.width == 0 || surface.height == 0 {
            return Ok(RenderSubmission {
                quads: 0,
                text_areas: 0,
                texture_uploads: 0,
            });
        }
        let viewport = self.viewport_transform(frame, surface);
        let mut texture_uploads = 0;
        let mut quads = Vec::new();
        let mut text = Vec::new();
        for command in &frame.commands {
            match command {
                DrawCommand::Sprite {
                    asset,
                    destination,
                    opacity,
                    visible,
                    blend,
                    scale,
                    rotation_degrees,
                    tint,
                    fit,
                    style,
                    ..
                } if *visible => {
                    let desired_size = if *fit == SpriteFit::Fill {
                        requested_size(*destination)
                    } else {
                        None
                    };
                    let texture_key = texture_key(asset, desired_size);
                    if !self.textures.contains_key(&texture_key) {
                        let image =
                            resolver
                                .load_image(asset, desired_size)
                                .map_err(|message| WgpuRendererError::Asset {
                                    asset: asset.clone(),
                                    message,
                                })?;
                        let texture = create_texture(
                            &self.device,
                            &self.queue,
                            &self.texture_layout,
                            &texture_key,
                            &image,
                        );
                        self.textures.insert(texture_key.clone(), texture);
                        texture_uploads += 1;
                    }
                    let texture = self
                        .textures
                        .get(&texture_key)
                        .expect("a texture was inserted or already existed");
                    let mut bounds =
                        fitted_sprite_bounds(*destination, texture.width, texture.height, *fit);
                    if (*scale - 1.0).abs() > f32::EPSILON {
                        let center_x = bounds.x + bounds.width * 0.5;
                        let center_y = bounds.y + bounds.height * 0.5;
                        bounds.width *= *scale;
                        bounds.height *= *scale;
                        bounds.x = center_x - bounds.width * 0.5;
                        bounds.y = center_y - bounds.height * 0.5;
                    }
                    let tint = Color {
                        red: ((u16::from(tint.red) * u16::from(style_opacity(style))) / 255) as u8,
                        green: ((u16::from(tint.green) * u16::from(style_opacity(style))) / 255)
                            as u8,
                        blue: ((u16::from(tint.blue) * u16::from(style_opacity(style))) / 255)
                            as u8,
                        alpha: (((u32::from(tint.alpha) * u32::from(*opacity))
                            * u32::from(style_opacity(style)))
                            / (255 * 255)) as u8,
                    };
                    quads.push(QuadPlan {
                        vertices: quad_vertices_styled(
                            bounds,
                            [tint; 4],
                            viewport,
                            surface,
                            *rotation_degrees,
                            style.corner_radius,
                            0.0,
                            0.0,
                            style.corner_radius > 0.0,
                        ),
                        texture_key,
                        blend: *blend,
                        clip: style
                            .clip
                            .or((*fit == SpriteFit::Cover).then_some(*destination))
                            .map(|clip| physical_rect(clip, viewport)),
                    });
                }
                DrawCommand::Rectangle {
                    bounds,
                    color,
                    corner_radius,
                    style,
                    ..
                } => {
                    if let Some(shadow) = style.shadow {
                        let blur = shadow.blur.max(0.75);
                        quads.push(QuadPlan {
                            vertices: quad_vertices_styled(
                                Rect {
                                    x: bounds.x + shadow.offset_x - blur,
                                    y: bounds.y + shadow.offset_y - blur,
                                    width: bounds.width + blur * 2.0,
                                    height: bounds.height + blur * 2.0,
                                },
                                [apply_style_opacity(shadow.color, style.opacity); 4],
                                viewport,
                                surface,
                                0.0,
                                (*corner_radius).max(style.corner_radius) + blur,
                                0.0,
                                blur,
                                true,
                            ),
                            texture_key: WHITE_TEXTURE_KEY.to_owned(),
                            blend: BlendMode::Alpha,
                            clip: style.clip.map(|clip| physical_rect(clip, viewport)),
                        });
                    }
                    let fill = gradient_colors(*bounds, *color, style.gradient, style.opacity);
                    quads.push(QuadPlan {
                        vertices: quad_vertices_styled(
                            *bounds,
                            fill,
                            viewport,
                            surface,
                            0.0,
                            (*corner_radius).max(style.corner_radius),
                            0.0,
                            0.0,
                            true,
                        ),
                        texture_key: WHITE_TEXTURE_KEY.to_owned(),
                        blend: BlendMode::Alpha,
                        clip: style.clip.map(|clip| physical_rect(clip, viewport)),
                    });
                    if let Some(border) = style.border {
                        let width = f32::from(border.width.max(1));
                        for border_bounds in [
                            Rect {
                                height: width,
                                ..*bounds
                            },
                            Rect {
                                y: bounds.y + bounds.height - width,
                                height: width,
                                ..*bounds
                            },
                            Rect { width, ..*bounds },
                            Rect {
                                x: bounds.x + bounds.width - width,
                                width,
                                ..*bounds
                            },
                        ] {
                            quads.push(QuadPlan {
                                vertices: quad_vertices_styled(
                                    border_bounds,
                                    [apply_style_opacity(border.color, style.opacity); 4],
                                    viewport,
                                    surface,
                                    0.0,
                                    (*corner_radius).max(style.corner_radius),
                                    width,
                                    0.0,
                                    true,
                                ),
                                texture_key: WHITE_TEXTURE_KEY.to_owned(),
                                blend: BlendMode::Alpha,
                                clip: style.clip.map(|clip| physical_rect(clip, viewport)),
                            });
                        }
                    }
                }
                DrawCommand::Text {
                    text: content,
                    speaker,
                    bounds,
                    color,
                    font_size,
                    style,
                    ..
                } => append_text_plans(
                    &mut text,
                    speaker.as_ref().map_or_else(
                        || content.clone(),
                        |speaker| format!("{speaker}\n{content}"),
                    ),
                    *bounds,
                    *color,
                    *font_size,
                    *style,
                ),
                DrawCommand::Sprite { .. } => {}
            }
        }
        if let Some(transition) = &frame.transition
            && let Some(overlay) =
                transition_overlay(frame, transition.kind.clone(), transition.progress)
        {
            quads.push(QuadPlan {
                vertices: quad_vertices(
                    overlay.bounds,
                    Color {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: overlay.alpha,
                    },
                    viewport,
                    surface,
                ),
                texture_key: WHITE_TEXTURE_KEY.to_owned(),
                blend: BlendMode::Alpha,
                clip: None,
            });
        }
        for effect in &frame.effects {
            let (color, alpha) = match effect {
                ScreenEffect::Tint {
                    color,
                    opacity,
                    progress,
                } => (
                    *color,
                    ((*opacity as f32) * (1.0 - *progress)).round() as u8,
                ),
                ScreenEffect::Flash {
                    color,
                    opacity,
                    progress,
                } => (
                    *color,
                    ((*opacity as f32) * (1.0 - *progress)).round() as u8,
                ),
                ScreenEffect::Shake { .. } => continue,
            };
            quads.push(QuadPlan {
                vertices: quad_vertices(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: frame.logical_size.width as f32,
                        height: frame.logical_size.height as f32,
                    },
                    Color { alpha, ..color },
                    viewport,
                    surface,
                ),
                texture_key: WHITE_TEXTURE_KEY.to_owned(),
                blend: BlendMode::Alpha,
                clip: None,
            });
        }

        let (shake_x, shake_y) = shake_offset(&frame.effects);
        if shake_x.abs() > f32::EPSILON || shake_y.abs() > f32::EPSILON {
            let ndc_x = shake_x * 2.0 / frame.logical_size.width.max(1) as f32;
            let ndc_y = -shake_y * 2.0 / frame.logical_size.height.max(1) as f32;
            for quad in &mut quads {
                for vertex in &mut quad.vertices {
                    vertex.position[0] += ndc_x;
                    vertex.position[1] += ndc_y;
                }
            }
            for plan in &mut text {
                plan.bounds.x += shake_x;
                plan.bounds.y += shake_y;
            }
        }

        if !text.is_empty() && !self.text.has_fonts() {
            return Err(WgpuRendererError::NoBundledFont);
        }

        let mut text_buffers = Vec::with_capacity(text.len());
        for plan in &text {
            let line_height = if plan.style.line_height > 0.0 {
                plan.style.line_height
            } else {
                plan.font_size * 1.35
            };
            let mut buffer = Buffer::new(
                &mut self.text.font_system,
                Metrics::new(
                    (plan.font_size.max(1.0) * viewport.scale).max(1.0),
                    (line_height.max(1.0) * viewport.scale).max(1.0),
                ),
            );
            let bounds = physical_rect(plan.bounds, viewport);
            buffer.set_size(Some(bounds.width.max(1.0)), Some(bounds.height.max(1.0)));
            let tracking_em = plan.style.letter_spacing / plan.font_size.max(1.0);
            buffer.set_text(
                &plan.text,
                &Attrs::new()
                    .family(Family::SansSerif)
                    .letter_spacing(tracking_em),
                Shaping::Advanced,
                Some(text_align(plan.style.text_align)),
            );
            buffer.shape_until_scroll(&mut self.text.font_system, false);
            text_buffers.push(buffer);
        }
        self.text
            .prepare(
                &self.device,
                &self.queue,
                surface.width,
                surface.height,
                text_buffers.iter().zip(&text).map(|(buffer, plan)| {
                    let bounds = physical_rect(plan.bounds, viewport);
                    let clipped = plan
                        .style
                        .clip
                        .map(|clip| intersect_rect(bounds, physical_rect(clip, viewport)))
                        .unwrap_or(bounds);
                    let color = apply_style_opacity(plan.color, plan.style.opacity);
                    TextArea {
                        buffer,
                        left: bounds.x,
                        top: bounds.y,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: clipped.x.floor() as i32,
                            top: clipped.y.floor() as i32,
                            right: (clipped.x + clipped.width).ceil() as i32,
                            bottom: (clipped.y + clipped.height).ceil() as i32,
                        },
                        default_color: GlyphColor::rgba(
                            color.red,
                            color.green,
                            color.blue,
                            color.alpha,
                        ),
                        custom_glyphs: &[],
                    }
                }),
            )
            .map_err(|error| WgpuRendererError::TextPrepare(error.to_string()))?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aria-2d-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aria-2d-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(to_wgpu_color(frame.clear_color)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for plan in &quads {
                if !set_scissor_for_clip(&mut pass, plan.clip, surface) {
                    continue;
                }
                pass.set_pipeline(match plan.blend {
                    BlendMode::Alpha => &self.alpha_pipeline,
                    BlendMode::Add => &self.add_pipeline,
                    BlendMode::Multiply => &self.multiply_pipeline,
                });
                let texture = self
                    .textures
                    .get(&plan.texture_key)
                    .expect("frame plans only reference resident textures");
                pass.set_bind_group(0, &texture.bind_group, &[]);
                let vertex_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("aria-2d-quad"),
                            contents: bytemuck::cast_slice(&plan.vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.draw(0..6, 0..1);
            }
            pass.set_scissor_rect(0, 0, surface.width, surface.height);
            self.text
                .render(&mut pass)
                .map_err(|error| WgpuRendererError::TextRender(error.to_string()))?;
        }
        self.queue.submit(Some(encoder.finish()));
        self.text.trim();
        Ok(RenderSubmission {
            quads: quads.len(),
            text_areas: text.len(),
            texture_uploads,
        })
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    label: &'static str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32,
        5 => Float32,
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    image: &RasterImage,
) -> GpuTexture {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &image.rgba,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("aria-2d-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    GpuTexture {
        _texture: texture,
        bind_group,
        width: image.width,
        height: image.height,
    }
}

fn requested_size(bounds: Rect) -> Option<(u32, u32)> {
    (bounds.width > 0.0 && bounds.height > 0.0)
        .then(|| (bounds.width.ceil() as u32, bounds.height.ceil() as u32))
}

fn texture_key(asset: &str, desired: Option<(u32, u32)>) -> String {
    desired.map_or_else(
        || format!("{asset}@intrinsic"),
        |(width, height)| format!("{asset}@{width}x{height}"),
    )
}

fn resolved_sprite_bounds(bounds: Rect, width: u32, height: u32) -> Rect {
    Rect {
        x: bounds.x,
        y: bounds.y,
        width: if bounds.width > 0.0 {
            bounds.width
        } else {
            width as f32
        },
        height: if bounds.height > 0.0 {
            bounds.height
        } else {
            height as f32
        },
    }
}

fn fitted_sprite_bounds(bounds: Rect, width: u32, height: u32, fit: SpriteFit) -> Rect {
    let destination = resolved_sprite_bounds(bounds, width, height);
    if matches!(fit, SpriteFit::Fill)
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || width == 0
        || height == 0
    {
        return destination;
    }
    let source_aspect = width as f32 / height as f32;
    let destination_aspect = destination.width / destination.height.max(f32::EPSILON);
    let scale = match fit {
        SpriteFit::Contain => {
            if source_aspect > destination_aspect {
                destination.width / width as f32
            } else {
                destination.height / height as f32
            }
        }
        SpriteFit::Cover => {
            if source_aspect > destination_aspect {
                destination.height / height as f32
            } else {
                destination.width / width as f32
            }
        }
        SpriteFit::Fill => unreachable!("fill returned above"),
    };
    let fitted_width = width as f32 * scale;
    let fitted_height = height as f32 * scale;
    Rect {
        x: destination.x + (destination.width - fitted_width) * 0.5,
        y: destination.y + (destination.height - fitted_height) * 0.5,
        width: fitted_width,
        height: fitted_height,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransitionOverlay {
    bounds: Rect,
    alpha: u8,
}

fn transition_overlay(
    frame: &SceneFrame,
    kind: TransitionKind,
    progress: f32,
) -> Option<TransitionOverlay> {
    let remaining = (1.0 - progress.clamp(0.0, 1.0)).max(0.0);
    if remaining <= f32::EPSILON {
        return None;
    }
    let width = frame.logical_size.width as f32;
    let height = frame.logical_size.height as f32;
    Some(match kind {
        TransitionKind::WipeLeft => TransitionOverlay {
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: width * remaining,
                height,
            },
            alpha: 255,
        },
        TransitionKind::WipeRight => TransitionOverlay {
            bounds: Rect {
                x: width * progress.clamp(0.0, 1.0),
                y: 0.0,
                width: width * remaining,
                height,
            },
            alpha: 255,
        },
        // A mask-specific texture will replace this fade fallback once the
        // declarative mask asset is represented in the renderer protocol.
        TransitionKind::FadeThroughBlack => {
            const DARK_HOLD_RATIO: f32 = 180.0 / 640.0;
            let alpha = if progress <= DARK_HOLD_RATIO {
                255
            } else {
                (((1.0 - (progress - DARK_HOLD_RATIO) / (1.0 - DARK_HOLD_RATIO)).clamp(0.0, 1.0))
                    * 255.0)
                    .round() as u8
            };
            TransitionOverlay {
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                },
                alpha,
            }
        }
        TransitionKind::Fade | TransitionKind::CrossFade | TransitionKind::Mask(_) => {
            TransitionOverlay {
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                },
                alpha: (remaining * 255.0).round() as u8,
            }
        }
    })
}

fn quad_vertices(
    bounds: Rect,
    color: Color,
    viewport: ViewportTransform,
    surface: RenderSurfaceSize,
) -> [QuadVertex; 6] {
    quad_vertices_styled(
        bounds, [color; 4], viewport, surface, 0.0, 0.0, 0.0, 0.0, false,
    )
}

#[allow(clippy::too_many_arguments)]
fn quad_vertices_styled(
    bounds: Rect,
    colors: [Color; 4],
    viewport: ViewportTransform,
    surface: RenderSurfaceSize,
    rotation_degrees: f32,
    corner_radius: f32,
    border_width: f32,
    softness: f32,
    shape_enabled: bool,
) -> [QuadVertex; 6] {
    let physical = physical_rect(bounds, viewport);
    let width = surface.width as f32;
    let height = surface.height as f32;
    let left = physical.x / width * 2.0 - 1.0;
    let right = (physical.x + physical.width) / width * 2.0 - 1.0;
    let top = 1.0 - physical.y / height * 2.0;
    let bottom = 1.0 - (physical.y + physical.height) / height * 2.0;
    let mut positions = [
        [left, top],
        [right, top],
        [right, bottom],
        [left, top],
        [right, bottom],
        [left, bottom],
    ];
    if rotation_degrees.abs() > f32::EPSILON {
        let center = [(left + right) * 0.5, (top + bottom) * 0.5];
        let radians = rotation_degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        for position in &mut positions {
            let x = position[0] - center[0];
            let y = position[1] - center[1];
            position[0] = center[0] + x * cos - y * sin;
            position[1] = center[1] + x * sin + y * cos;
        }
    }
    let sdf = [
        physical.width.max(1.0),
        physical.height.max(1.0),
        (corner_radius.max(0.0) * viewport.scale).min(physical.width.min(physical.height) * 0.5),
        softness.max(0.0) * viewport.scale,
    ];
    let to_float = |color: Color| {
        [
            color.red as f32 / 255.0,
            color.green as f32 / 255.0,
            color.blue as f32 / 255.0,
            color.alpha as f32 / 255.0,
        ]
    };
    let vertex = |position, uv, color| QuadVertex {
        position,
        uv,
        color: to_float(color),
        sdf,
        border_width: border_width.max(0.0) * viewport.scale,
        shape_enabled: f32::from(shape_enabled),
    };
    [
        vertex(positions[0], [0.0, 0.0], colors[0]),
        vertex(positions[1], [1.0, 0.0], colors[1]),
        vertex(positions[2], [1.0, 1.0], colors[2]),
        vertex(positions[3], [0.0, 0.0], colors[0]),
        vertex(positions[4], [1.0, 1.0], colors[2]),
        vertex(positions[5], [0.0, 1.0], colors[3]),
    ]
}

fn gradient_colors(
    bounds: Rect,
    fallback: Color,
    gradient: Option<GradientStyle>,
    opacity: u8,
) -> [Color; 4] {
    let Some(gradient) = gradient else {
        return [apply_style_opacity(fallback, opacity); 4];
    };
    let radians = gradient.angle_degrees.to_radians();
    let direction = [radians.cos(), radians.sin()];
    let denominator =
        (direction[0].abs() * bounds.width + direction[1].abs() * bounds.height).max(1.0);
    let sample = |x: f32, y: f32| {
        let projection = (x * direction[0] + y * direction[1]) / denominator + 0.5;
        blend_color(
            gradient.start,
            gradient.end,
            projection.clamp(0.0, 1.0),
            opacity,
        )
    };
    let half_width = bounds.width * 0.5;
    let half_height = bounds.height * 0.5;
    [
        sample(-half_width, -half_height),
        sample(half_width, -half_height),
        sample(half_width, half_height),
        sample(-half_width, half_height),
    ]
}

fn blend_color(start: Color, end: Color, progress: f32, opacity: u8) -> Color {
    let blend = |left: u8, right: u8| {
        (f32::from(left) + (f32::from(right) - f32::from(left)) * progress).round() as u8
    };
    apply_style_opacity(
        Color {
            red: blend(start.red, end.red),
            green: blend(start.green, end.green),
            blue: blend(start.blue, end.blue),
            alpha: blend(start.alpha, end.alpha),
        },
        opacity,
    )
}

fn apply_style_opacity(mut color: Color, opacity: u8) -> Color {
    color.alpha = ((u16::from(color.alpha) * u16::from(opacity)) / 255) as u8;
    color
}

fn append_text_plans(
    plans: &mut Vec<TextPlan>,
    text: String,
    bounds: Rect,
    color: Color,
    font_size: f32,
    style: DrawStyle,
) {
    match style.text_decoration {
        TextDecoration::Shadow {
            color: shadow,
            offset_x,
            offset_y,
        } => plans.push(TextPlan {
            text: text.clone(),
            bounds: Rect {
                x: bounds.x + f32::from(offset_x),
                y: bounds.y + f32::from(offset_y),
                ..bounds
            },
            color: shadow,
            font_size,
            style,
        }),
        TextDecoration::Outline {
            color: outline,
            width,
        } => {
            let width = i8::try_from(width.min(i8::MAX as u8)).unwrap_or(i8::MAX);
            for (x, y) in [(-width, 0), (width, 0), (0, -width), (0, width)] {
                plans.push(TextPlan {
                    text: text.clone(),
                    bounds: Rect {
                        x: bounds.x + f32::from(x),
                        y: bounds.y + f32::from(y),
                        ..bounds
                    },
                    color: outline,
                    font_size,
                    style,
                });
            }
        }
        TextDecoration::None => {}
    }
    plans.push(TextPlan {
        text,
        bounds,
        color,
        font_size,
        style,
    });
}

fn text_align(align: TextAlign) -> Align {
    match align {
        TextAlign::Start => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::End => Align::End,
    }
}

fn intersect_rect(left: Rect, right: Rect) -> Rect {
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let right_edge = (left.x + left.width).min(right.x + right.width);
    let bottom_edge = (left.y + left.height).min(right.y + right.height);
    Rect {
        x,
        y,
        width: (right_edge - x).max(0.0),
        height: (bottom_edge - y).max(0.0),
    }
}

fn set_scissor_for_clip(
    pass: &mut wgpu::RenderPass<'_>,
    clip: Option<Rect>,
    surface: RenderSurfaceSize,
) -> bool {
    let clip = clip.unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: surface.width as f32,
        height: surface.height as f32,
    });
    let x = clip.x.max(0.0).floor() as u32;
    let y = clip.y.max(0.0).floor() as u32;
    let right = (clip.x + clip.width)
        .min(surface.width as f32)
        .ceil()
        .max(0.0) as u32;
    let bottom = (clip.y + clip.height)
        .min(surface.height as f32)
        .ceil()
        .max(0.0) as u32;
    if right <= x || bottom <= y {
        return false;
    }
    pass.set_scissor_rect(x, y, right - x, bottom - y);
    true
}

fn style_opacity(style: &DrawStyle) -> u8 {
    style.opacity
}

fn shake_offset(effects: &[ScreenEffect]) -> (f32, f32) {
    effects
        .iter()
        .filter_map(|effect| {
            let ScreenEffect::Shake {
                amplitude,
                progress,
            } = effect
            else {
                return None;
            };
            let fade = (1.0 - progress.clamp(0.0, 1.0)).max(0.0);
            let phase = progress.clamp(0.0, 1.0) * std::f32::consts::TAU * 3.0;
            Some((
                amplitude * phase.sin() * fade,
                amplitude * (phase * 1.37).cos() * fade,
            ))
        })
        .fold((0.0, 0.0), |(x, y), (dx, dy)| (x + dx, y + dy))
}

fn physical_rect(bounds: Rect, viewport: ViewportTransform) -> Rect {
    Rect {
        x: viewport.offset_x + bounds.x * viewport.scale,
        y: viewport.offset_y + bounds.y * viewport.scale,
        width: bounds.width * viewport.scale,
        height: bounds.height * viewport.scale,
    }
}

fn to_wgpu_color(color: Color) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(color.red) / 255.0,
        g: f64::from(color.green) / 255.0,
        b: f64::from(color.blue) / 255.0,
        a: f64::from(color.alpha) / 255.0,
    }
}

const QUAD_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) sdf: vec4<f32>,
    @location(4) border_width: f32,
    @location(5) shape_enabled: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) sdf: vec4<f32>,
    @location(3) border_width: f32,
    @location(4) shape_enabled: f32,
}

@group(0) @binding(0) var texture_sampler_source: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    output.sdf = input.sdf;
    output.border_width = input.border_width;
    output.shape_enabled = input.shape_enabled;
    return output;
}

fn rounded_box_distance(uv: vec2<f32>, extent: vec2<f32>, radius: f32) -> f32 {
    let point = (uv - vec2<f32>(0.5, 0.5)) * extent;
    let inner = extent * 0.5 - vec2<f32>(radius, radius);
    let corner = abs(point) - inner;
    return length(max(corner, vec2<f32>(0.0, 0.0)))
        + min(max(corner.x, corner.y), 0.0) - radius;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var output = textureSample(texture_sampler_source, texture_sampler, input.uv) * input.color;
    if input.shape_enabled > 0.5 {
        let distance = rounded_box_distance(input.uv, input.sdf.xy, input.sdf.z);
        let anti_alias = max(0.75, input.sdf.w);
        let coverage = 1.0 - smoothstep(-anti_alias, anti_alias, distance);
        output.a = output.a * coverage;
    }
    return output;
}
"#;

#[cfg(test)]
mod tests {
    use aria_core::protocol::{LogicalSize, SceneFrame};

    use super::*;

    #[test]
    fn sprite_without_explicit_size_uses_decoded_intrinsic_dimensions() {
        let bounds = resolved_sprite_bounds(
            Rect {
                x: 10.0,
                y: 20.0,
                width: 0.0,
                height: 0.0,
            },
            360,
            620,
        );
        assert_eq!((bounds.width, bounds.height), (360.0, 620.0));
    }

    #[test]
    fn letterboxed_geometry_maps_logical_bounds_to_ndc() {
        let frame = SceneFrame {
            frame_number: 1,
            logical_size: LogicalSize {
                width: 1280,
                height: 720,
            },
            viewport: Default::default(),
            clear_color: Color::BLACK,
            commands: Vec::new(),
            transition: None,
            effects: Vec::new(),
        };
        let surface = RenderSurfaceSize::new(2560, 1080, 1.0);
        let viewport = ViewportTransform::fit(
            frame.logical_size,
            surface.width,
            surface.height,
            surface.scale_factor(),
            SafeAreaInsets::default(),
        );
        let vertices = quad_vertices(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
            },
            Color::WHITE,
            viewport,
            surface,
        );
        assert!(vertices[0].position[0] > -1.0);
        assert_eq!(vertices[0].position[1], 1.0);
        assert_eq!(vertices[2].position[1], -1.0);
    }

    #[test]
    fn fade_through_black_holds_dark_before_revealing_the_new_scene() {
        let frame = SceneFrame {
            frame_number: 1,
            logical_size: LogicalSize {
                width: 1280,
                height: 720,
            },
            viewport: Default::default(),
            clear_color: Color::BLACK,
            commands: Vec::new(),
            transition: None,
            effects: Vec::new(),
        };
        let kind = TransitionKind::FadeThroughBlack;
        assert_eq!(
            transition_overlay(&frame, kind.clone(), 0.0).unwrap().alpha,
            255
        );
        assert_eq!(
            transition_overlay(&frame, kind.clone(), 0.25)
                .unwrap()
                .alpha,
            255
        );
        let reveal = transition_overlay(&frame, kind.clone(), 0.7).unwrap().alpha;
        assert!(reveal < 255 && reveal > 0);
        assert!(transition_overlay(&frame, kind, 1.0).is_none());
    }
}
