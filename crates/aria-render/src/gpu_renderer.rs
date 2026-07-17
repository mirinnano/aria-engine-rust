//! Concrete wgpu submission for the platform-neutral `RenderFrame` protocol.
//!
//! This module owns no window, filesystem, browser API, or audio device. The
//! host provides an `ImageResolver`; Native and Web can therefore submit the
//! same ordered Core frame through their own asset transport.

use std::collections::BTreeMap;

use aria_core::protocol::{BlendMode, Color, DrawCommand, Rect, RenderFrame, TransitionKind};
use bytemuck::{Pod, Zeroable};
use glyphon::{Attrs, Buffer, Color as GlyphColor, Family, Metrics, Shaping, TextArea, TextBounds};
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
}

#[derive(Debug)]
struct TextPlan {
    text: String,
    bounds: Rect,
    color: Color,
    font_size: f32,
}

/// A small, order-preserving 2D renderer for `RenderFrame`.
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
        frame: &RenderFrame,
        surface: RenderSurfaceSize,
    ) -> ViewportTransform {
        ViewportTransform::fit(
            frame.logical_size,
            surface.width.max(1),
            surface.height.max(1),
            surface.scale_factor(),
            SafeAreaInsets::default(),
        )
    }

    /// Encodes and submits one complete ordered Core frame to `target`.
    pub fn render(
        &mut self,
        frame: &RenderFrame,
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
                    ..
                } if *visible => {
                    let desired_size = requested_size(*destination);
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
                    let bounds =
                        resolved_sprite_bounds(*destination, texture.width, texture.height);
                    quads.push(QuadPlan {
                        vertices: quad_vertices(
                            bounds,
                            Color {
                                red: 255,
                                green: 255,
                                blue: 255,
                                alpha: *opacity,
                            },
                            viewport,
                            surface,
                        ),
                        texture_key,
                        blend: *blend,
                    });
                }
                DrawCommand::Rectangle { bounds, color, .. } => quads.push(QuadPlan {
                    vertices: quad_vertices(*bounds, *color, viewport, surface),
                    texture_key: WHITE_TEXTURE_KEY.to_owned(),
                    blend: BlendMode::Alpha,
                }),
                DrawCommand::Text {
                    text: content,
                    speaker,
                    bounds,
                    color,
                    font_size,
                    ..
                } => text.push(TextPlan {
                    text: speaker.as_ref().map_or_else(
                        || content.clone(),
                        |speaker| format!("{speaker}\n{content}"),
                    ),
                    bounds: *bounds,
                    color: *color,
                    font_size: *font_size,
                }),
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
            });
        }

        if !text.is_empty() && !self.text.has_fonts() {
            return Err(WgpuRendererError::NoBundledFont);
        }

        let mut text_buffers = Vec::with_capacity(text.len());
        for plan in &text {
            let mut buffer = Buffer::new(
                &mut self.text.font_system,
                Metrics::new(
                    (plan.font_size.max(1.0) * viewport.scale).max(1.0),
                    (plan.font_size.max(1.0) * viewport.scale * 1.35).max(1.0),
                ),
            );
            let bounds = physical_rect(plan.bounds, viewport);
            buffer.set_size(Some(bounds.width.max(1.0)), Some(bounds.height.max(1.0)));
            buffer.set_text(
                &plan.text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
                None,
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
                    TextArea {
                        buffer,
                        left: bounds.x,
                        top: bounds.y,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: bounds.x.floor() as i32,
                            top: bounds.y.floor() as i32,
                            right: (bounds.x + bounds.width).ceil() as i32,
                            bottom: (bounds.y + bounds.height).ceil() as i32,
                        },
                        default_color: GlyphColor::rgba(
                            plan.color.red,
                            plan.color.green,
                            plan.color.blue,
                            plan.color.alpha,
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
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransitionOverlay {
    bounds: Rect,
    alpha: u8,
}

fn transition_overlay(
    frame: &RenderFrame,
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
    let physical = physical_rect(bounds, viewport);
    let width = surface.width as f32;
    let height = surface.height as f32;
    let left = physical.x / width * 2.0 - 1.0;
    let right = (physical.x + physical.width) / width * 2.0 - 1.0;
    let top = 1.0 - physical.y / height * 2.0;
    let bottom = 1.0 - (physical.y + physical.height) / height * 2.0;
    let color = [
        color.red as f32 / 255.0,
        color.green as f32 / 255.0,
        color.blue as f32 / 255.0,
        color.alpha as f32 / 255.0,
    ];
    [
        QuadVertex {
            position: [left, top],
            uv: [0.0, 0.0],
            color,
        },
        QuadVertex {
            position: [right, top],
            uv: [1.0, 0.0],
            color,
        },
        QuadVertex {
            position: [right, bottom],
            uv: [1.0, 1.0],
            color,
        },
        QuadVertex {
            position: [left, top],
            uv: [0.0, 0.0],
            color,
        },
        QuadVertex {
            position: [right, bottom],
            uv: [1.0, 1.0],
            color,
        },
        QuadVertex {
            position: [left, bottom],
            uv: [0.0, 1.0],
            color,
        },
    ]
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
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0) var texture_sampler_source: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(texture_sampler_source, texture_sampler, input.uv) * input.color;
}
"#;

#[cfg(test)]
mod tests {
    use aria_core::protocol::LogicalSize;

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
        let frame = RenderFrame {
            frame_number: 1,
            logical_size: LogicalSize {
                width: 1280,
                height: 720,
            },
            clear_color: Color::BLACK,
            commands: Vec::new(),
            transition: None,
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
}
