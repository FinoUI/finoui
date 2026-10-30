//! Shared wgpu device, pipelines, glyph atlas, and per-window surface rendering.
//! 共享 wgpu Device、Pipeline、Glyph atlas 与每窗口 Surface 渲染。

use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
    sync::Arc,
};

use bytemuck::{Pod, Zeroable};
use skrifa::{
    MetadataProvider,
    outline::{DrawSettings, OutlinePen},
    prelude::{FontRef, GlyphId, LocationRef, Size},
};
use thiserror::Error;

use wgpu::util::DeviceExt as _;
use winit::window::Window;

use super::{
    geom::Rect,
    paint::{Color, DisplayList, PaintCommand},
    window::{SurfaceState, WindowContext},
};
use crate::text::TextEngine;

const ATLAS_SIZE: u32 = 1024;
const ATLAS_PADDING: u32 = 2;

/// Native rendering failure without platform-specific panic text.
/// 不含平台特定 panic 文本的原生渲染故障。
#[derive(Debug, Error)]
pub enum RenderError {
    /// No suitable GPU adapter is available.
    /// 没有可用的 GPU 适配器。
    #[error("no suitable GPU adapter: {0}")]
    Adapter(String),
    /// Device creation failed.
    /// Device 创建失败。
    #[error("GPU device creation failed: {0}")]
    Device(String),
    /// Surface creation or configuration failed.
    /// Surface 创建或配置失败。
    #[error("GPU surface failure: {0}")]
    Surface(String),
    /// The window has no installed surface.
    /// 窗口没有已安装 Surface。
    #[error("window {0} has no surface")]
    MissingSurface(u64),
    /// The window identity is not registered.
    /// 窗口身份未注册。
    #[error("window {0} is not registered")]
    MissingWindow(u64),
    /// A glyph outline could not be resolved.
    /// 无法解析 glyph 轮廓。
    #[error("glyph rasterization failed: {0}")]
    Glyph(String),
}

/// Result of attempting to render one window.
/// 尝试渲染单个窗口的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderStatus {
    /// A frame was rendered and presented.
    /// 已渲染并呈现一帧。
    Rendered,
    /// The surface was outdated and must be reconfigured.
    /// Surface 已过期，必须重新配置。
    Reconfigured,
    /// The surface was lost and must be recreated.
    /// Surface 已丢失，必须重建。
    SurfaceLost,
    /// The frame was skipped because the surface was occluded or timed out.
    /// 因 Surface 被遮挡或超时而跳过该帧。
    Skipped,
}

/// Resource counters exposed to diagnostics.
/// 暴露给诊断的资源计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Glyphs rasterized into the atlas.
    /// 光栅化到 atlas 的 glyph 数。
    pub rasterized_glyphs: u64,
    /// Atlas resets caused by capacity pressure.
    /// 因容量压力重置 atlas 的次数。
    pub atlas_resets: u64,
    /// Rendered frames.
    /// 已渲染帧数。
    pub rendered_frames: u64,
    /// Last vertex count.
    /// 最近一次顶点数。
    pub vertices: u64,
}

/// One textured vertex.
/// 一个带纹理的顶点。
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 16,
            shader_location: 2,
        },
    ];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GlyphKey {
    font_ptr: usize,
    glyph_id: u32,
    size_quarters: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct AtlasGlyph {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    left: f32,
    top: f32,
}

/// CPU-side R8 glyph atlas shared by every window.
/// 由所有窗口共享的 CPU 侧 R8 glyph atlas。
struct GlyphAtlas {
    data: Vec<u8>,
    next_x: u32,
    next_y: u32,
    row_height: u32,
    cache: BTreeMap<GlyphKey, AtlasGlyph>,
    dirty: bool,
}

impl GlyphAtlas {
    fn new() -> Self {
        let mut atlas = Self {
            data: vec![0; (ATLAS_SIZE * ATLAS_SIZE) as usize],
            next_x: 1,
            next_y: 1,
            row_height: 1,
            cache: BTreeMap::new(),
            dirty: true,
        };
        // A single white texel lets solid quads share the glyph pipeline.
        // 一个白色 texel 让实心矩形复用 glyph pipeline。
        atlas.data[0] = 255;
        atlas
    }

    fn reset(&mut self) {
        self.data.fill(0);
        self.data[0] = 255;
        self.next_x = 1;
        self.next_y = 1;
        self.row_height = 1;
        self.cache.clear();
        self.dirty = true;
    }

    fn white_uv() -> [f32; 2] {
        [0.5 / ATLAS_SIZE as f32, 0.5 / ATLAS_SIZE as f32]
    }

    fn ensure(
        &mut self,
        key: GlyphKey,
        glyph: &crate::text::PositionedGlyph,
    ) -> Option<(AtlasGlyph, bool)> {
        if let Some(existing) = self.cache.get(&key) {
            return Some((*existing, false));
        }
        let raster = rasterize_glyph(glyph, key.size_quarters as f32 / 4.0)?;
        let width = raster.width + ATLAS_PADDING * 2;
        let height = raster.height + ATLAS_PADDING * 2;
        if self.next_x + width >= ATLAS_SIZE {
            self.next_x = 1;
            self.next_y += self.row_height;
            self.row_height = 1;
        }
        if self.next_y + height >= ATLAS_SIZE {
            self.reset();
        }
        let x = self.next_x;
        let y = self.next_y;
        for row in 0..raster.height {
            let source = (row * raster.width) as usize;
            let destination = ((y + ATLAS_PADDING + row) * ATLAS_SIZE + x + ATLAS_PADDING) as usize;
            self.data[destination..destination + raster.width as usize]
                .copy_from_slice(&raster.data[source..source + raster.width as usize]);
        }
        self.next_x += width;
        self.row_height = self.row_height.max(height);
        self.dirty = true;
        let atlas_glyph = AtlasGlyph {
            x: x + ATLAS_PADDING,
            y: y + ATLAS_PADDING,
            width: raster.width,
            height: raster.height,
            left: raster.left,
            top: raster.top,
        };
        self.cache.insert(key, atlas_glyph);
        Some((atlas_glyph, true))
    }
}

struct RasterGlyph {
    width: u32,
    height: u32,
    left: f32,
    top: f32,
    data: Vec<u8>,
}

struct TinyPen {
    builder: tiny_skia::PathBuilder,
}

impl TinyPen {
    fn new() -> Self {
        Self {
            builder: tiny_skia::PathBuilder::new(),
        }
    }
}

impl OutlinePen for TinyPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, -y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, -y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.builder.quad_to(cx0, -cy0, x, -y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.builder.cubic_to(cx0, -cy0, cx1, -cy1, x, -y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

fn rasterize_glyph(glyph: &crate::text::PositionedGlyph, size: f32) -> Option<RasterGlyph> {
    let font = FontRef::from_index(glyph.font.data.as_ref(), glyph.font_index).ok()?;
    let outlines = font.outline_glyphs();
    let outline = outlines.get(GlyphId::new(glyph.glyph_id))?;
    let mut pen = TinyPen::new();
    outline
        .draw(
            DrawSettings::unhinted(Size::new(size), LocationRef::default()),
            &mut pen,
        )
        .ok()?;
    let path = pen.builder.finish()?;
    let bounds = path.bounds();
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return None;
    }
    let width = bounds.width().ceil() as u32;
    let height = bounds.height().ceil() as u32;
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(width + ATLAS_PADDING * 2, height + ATLAS_PADDING * 2)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    let transform = tiny_skia::Transform::from_translate(
        -bounds.x() + ATLAS_PADDING as f32,
        -bounds.y() + ATLAS_PADDING as f32,
    );
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
    let mut data = vec![0; (width * height) as usize];
    let pixels = pixmap.data();
    let stride = pixmap.width() as usize * 4;
    for row in 0..height as usize {
        for column in 0..width as usize {
            let source = row * stride + column * 4 + 3;
            data[row * width as usize + column] = pixels[source];
        }
    }
    Some(RasterGlyph {
        width,
        height,
        left: bounds.x() - ATLAS_PADDING as f32,
        top: bounds.y() - ATLAS_PADDING as f32,
        data,
    })
}

/// Shared application-level GPU and text resources.
/// 应用级共享 GPU 与文本资源。
pub struct SharedRenderContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    atlas_texture: wgpu::Texture,
    #[allow(dead_code, reason = "kept alive for the shared glyph bind group")]
    atlas_view: wgpu::TextureView,
    atlas_bind_group: wgpu::BindGroup,
    atlas: GlyphAtlas,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u64,
    text_engine: Rc<std::cell::RefCell<TextEngine>>,
    stats: RenderStats,
}

impl SharedRenderContext {
    /// Creates the shared context with the default wgpu instance.
    /// 使用默认 wgpu Instance 创建共享上下文。
    pub fn new() -> Result<Self, RenderError> {
        Self::with_instance(wgpu::Instance::default())
    }

    /// Creates the shared context around an explicit wgpu instance.
    /// 围绕显式 wgpu Instance 创建共享上下文。
    pub fn with_instance(instance: wgpu::Instance) -> Result<Self, RenderError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|error| RenderError::Adapter(error.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fino-native-device"),
            ..wgpu::DeviceDescriptor::default()
        }))
        .map_err(|error| RenderError::Device(error.to_string()))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fino-native-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fino-native-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fino-native-glyph-atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fino-native-glyph-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fino-native-glyph-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fino-native-vertices"),
            contents: bytemuck::cast_slice(&[Vertex {
                position: [0.0, 0.0],
                uv: [0.0, 0.0],
                color: [0.0, 0.0, 0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            shader,
            bind_group_layout,
            atlas_texture,
            atlas_view,
            atlas_bind_group,
            atlas: GlyphAtlas::new(),
            pipelines: HashMap::new(),
            vertex_buffer,
            vertex_capacity: std::mem::size_of::<Vertex>() as u64,
            text_engine: Rc::new(std::cell::RefCell::new(TextEngine::new())),
            stats: RenderStats::default(),
        })
    }

    /// Returns the shared text engine handle.
    /// 返回共享文本引擎句柄。
    pub fn text_engine(&self) -> Rc<std::cell::RefCell<TextEngine>> {
        Rc::clone(&self.text_engine)
    }

    /// Returns shared resource counters.
    /// 返回共享资源计数。
    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Creates a surface from a winit window without configuring it yet.
    /// 从 winit 窗口创建 Surface，但暂不配置。
    pub fn create_surface(
        &self,
        window: Arc<Window>,
    ) -> Result<wgpu::Surface<'static>, RenderError> {
        self.instance
            .create_surface(window)
            .map_err(|error| RenderError::Surface(error.to_string()))
    }

    /// Attaches and configures a surface for one window.
    /// 为一个窗口挂载并配置 Surface。
    pub fn attach_surface(
        &self,
        context: &mut WindowContext,
        window: Arc<Window>,
    ) -> Result<(), RenderError> {
        let surface = self.create_surface(window)?;
        self.install_surface(context, surface)
    }

    /// Installs an already created surface and configures it for the current viewport.
    /// 安装已创建 Surface，并按当前视口配置。
    pub fn install_surface(
        &self,
        context: &mut WindowContext,
        surface: wgpu::Surface<'static>,
    ) -> Result<(), RenderError> {
        let capabilities = surface.get_capabilities(&self.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| RenderError::Surface("surface exposes no formats".to_owned()))?;
        let viewport = context.viewport();
        let dpr = context.dpr();
        let width = ((viewport.width * dpr).round() as u32).max(1);
        let height = ((viewport.height * dpr).round() as u32).max(1);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: capabilities
                .present_modes
                .first()
                .copied()
                .unwrap_or(wgpu::PresentMode::Fifo),
            alpha_mode: capabilities
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&self.device, &config);
        context.install_surface(SurfaceState { surface, config });
        Ok(())
    }

    /// Reconfigures an existing window surface after a resize.
    /// 在缩放后重新配置现有窗口 Surface。
    pub fn reconfigure_surface(&self, context: &mut WindowContext) -> Result<(), RenderError> {
        let viewport = context.viewport();
        let dpr = context.dpr();
        let width = ((viewport.width * dpr).round() as u32).max(1);
        let height = ((viewport.height * dpr).round() as u32).max(1);
        let Some(state) = context.surface_mut() else {
            return Err(RenderError::MissingSurface(context.id().value()));
        };
        if state.config.width == width && state.config.height == height {
            return Ok(());
        }
        state.config.width = width;
        state.config.height = height;
        state.surface.configure(&self.device, &state.config);
        Ok(())
    }

    /// Forces the existing surface to be reconfigured after an outdated swapchain.
    /// 在交换链过期后强制重新配置现有 Surface。
    fn force_reconfigure_surface(&self, context: &mut WindowContext) -> Result<(), RenderError> {
        let viewport = context.viewport();
        let dpr = context.dpr();
        let width = ((viewport.width * dpr).round() as u32).max(1);
        let height = ((viewport.height * dpr).round() as u32).max(1);
        let Some(state) = context.surface_mut() else {
            return Err(RenderError::MissingSurface(context.id().value()));
        };
        state.config.width = width;
        state.config.height = height;
        state.surface.configure(&self.device, &state.config);
        Ok(())
    }

    /// Renders one window when it has a surface; DOM and layout survive surface loss.
    /// 在窗口有 Surface 时渲染；Surface 丢失后 DOM 与布局仍保留。
    pub fn render(&mut self, context: &mut WindowContext) -> Result<RenderStatus, RenderError> {
        if context.surface().is_none() {
            return Ok(RenderStatus::Skipped);
        }
        self.reconfigure_surface(context)?;
        let display_list = context.paint();
        let dpr = context.dpr();
        let (width, height, format) = {
            let state = context.surface().expect("surface checked above");
            (state.config.width, state.config.height, state.config.format)
        };
        let current = context
            .surface()
            .expect("surface checked above")
            .surface
            .get_current_texture();
        let frame = match current {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.force_reconfigure_surface(context)?;
                return Ok(RenderStatus::Reconfigured);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                context.surface_lost();
                return Ok(RenderStatus::SurfaceLost);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(RenderStatus::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(RenderError::Surface(
                    "surface validation failed while acquiring a frame".to_owned(),
                ));
            }
        };

        let vertices = self.build_vertices(&display_list, dpr, width, height);
        if self.atlas.dirty {
            self.upload_atlas();
        }
        if !vertices.is_empty() {
            let bytes = bytemuck::cast_slice(&vertices);
            if bytes.len() as u64 > self.vertex_capacity {
                self.vertex_capacity = bytes.len() as u64;
                self.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("fino-native-vertices"),
                    size: self.vertex_capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
        }
        self.ensure_pipeline(format);
        let pipeline = self.pipelines.get(&format).expect("pipeline inserted");
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fino-native-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fino-native-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.055,
                            g: 0.06,
                            b: 0.075,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !vertices.is_empty() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        context.finish_frame();
        self.stats.rendered_frames += 1;
        self.stats.vertices = vertices.len() as u64;
        Ok(RenderStatus::Rendered)
    }

    /// Renders a display list into an RGBA8 texture and returns unpadded pixels.
    /// 将显示列表渲染到 RGBA8 纹理并返回无行填充的像素。
    pub fn render_to_rgba(
        &mut self,
        display_list: &DisplayList,
        width: u32,
        height: u32,
        dpr: f32,
    ) -> Result<Vec<u8>, RenderError> {
        let width = width.max(1);
        let height = height.max(1);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        self.ensure_pipeline(format);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fino-native-offscreen-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let vertices = self.build_vertices(display_list, dpr, width, height);
        if self.atlas.dirty {
            self.upload_atlas();
        }
        if !vertices.is_empty() {
            let bytes = bytemuck::cast_slice(&vertices);
            if bytes.len() as u64 > self.vertex_capacity {
                self.vertex_capacity = bytes.len() as u64;
                self.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("fino-native-vertices"),
                    size: self.vertex_capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
        }
        let pipeline = self.pipelines.get(&format).expect("pipeline inserted");
        let padded_bytes_per_row = (width * 4).div_ceil(256) * 256;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fino-native-readback"),
            size: u64::from(padded_bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fino-native-offscreen-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fino-native-offscreen-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.055,
                            g: 0.06,
                            b: 0.075,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !vertices.is_empty() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| RenderError::Surface(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| RenderError::Surface(error.to_string()))?
            .map_err(|error| RenderError::Surface(error.to_string()))?;
        let mapped = readback.get_mapped_range(..);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height as usize {
            let start = row * padded_bytes_per_row as usize;
            pixels.extend_from_slice(&mapped[start..start + width as usize * 4]);
        }
        drop(mapped);
        readback.unmap();
        Ok(pixels)
    }

    fn ensure_pipeline(&mut self, format: wgpu::TextureFormat) {
        if !self.pipelines.contains_key(&format) {
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("fino-native-pipeline-layout"),
                        bind_group_layouts: &[Some(&self.bind_group_layout)],
                        immediate_size: 0,
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("fino-native-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[Vertex::layout()],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                });
            self.pipelines.insert(format, pipeline);
        }
    }

    fn upload_atlas(&mut self) {
        self.queue.write_texture(
            self.atlas_texture.as_image_copy(),
            &self.atlas.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_SIZE),
                rows_per_image: Some(ATLAS_SIZE),
            },
            wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
        );
        self.atlas.dirty = false;
    }

    fn build_vertices(
        &mut self,
        display_list: &DisplayList,
        dpr: f32,
        width: u32,
        height: u32,
    ) -> Vec<Vertex> {
        let mut vertices = Vec::new();
        let white = GlyphAtlas::white_uv();
        for command in display_list.commands() {
            match command {
                PaintCommand::Rect { bounds, color, .. } => {
                    let physical = Rect::new(
                        bounds.x * dpr,
                        bounds.y * dpr,
                        bounds.width * dpr,
                        bounds.height * dpr,
                    );
                    push_pixel_quad(&mut vertices, physical, width, height, white, white, *color);
                }
                PaintCommand::Text {
                    bounds,
                    layout,
                    color,
                    ..
                } => {
                    let color = *color;
                    for glyph in layout.glyphs() {
                        let size_quarters = (glyph.font_size * dpr * 4.0).round().max(1.0) as u32;
                        let key = GlyphKey {
                            font_ptr: glyph.font.data.as_ref().as_ptr() as usize,
                            glyph_id: glyph.glyph_id,
                            size_quarters,
                        };
                        if let Some((atlas, rasterized)) = self.atlas.ensure(key, glyph) {
                            let x = (bounds.x + glyph.x) * dpr + atlas.left;
                            let y = (bounds.y + glyph.y) * dpr + atlas.top;
                            let (u0, v0) = (
                                atlas.x as f32 / ATLAS_SIZE as f32,
                                atlas.y as f32 / ATLAS_SIZE as f32,
                            );
                            let (u1, v1) = (
                                (atlas.x + atlas.width) as f32 / ATLAS_SIZE as f32,
                                (atlas.y + atlas.height) as f32 / ATLAS_SIZE as f32,
                            );
                            push_pixel_quad(
                                &mut vertices,
                                Rect::new(x, y, atlas.width as f32, atlas.height as f32),
                                width,
                                height,
                                [u0, v0],
                                [u1, v1],
                                color,
                            );
                            if rasterized {
                                self.stats.rasterized_glyphs += 1;
                            }
                        }
                    }
                }
            }
        }
        vertices
    }
}

fn push_pixel_quad(
    vertices: &mut Vec<Vertex>,
    bounds: Rect,
    width: u32,
    height: u32,
    uv0: [f32; 2],
    uv1: [f32; 2],
    color: Color,
) {
    let x0 = bounds.x / width as f32 * 2.0 - 1.0;
    let y0 = 1.0 - bounds.y / height as f32 * 2.0;
    let x1 = bounds.right() / width as f32 * 2.0 - 1.0;
    let y1 = 1.0 - bounds.bottom() / height as f32 * 2.0;
    let color = color.to_array();
    let corners = [
        ([x0, y0], [uv0[0], uv0[1]]),
        ([x1, y0], [uv1[0], uv0[1]]),
        ([x1, y1], [uv1[0], uv1[1]]),
        ([x0, y0], [uv0[0], uv0[1]]),
        ([x1, y1], [uv1[0], uv1[1]]),
        ([x0, y1], [uv0[0], uv1[1]]),
    ];
    vertices.extend(corners.map(|(position, uv)| Vertex {
        position,
        uv,
        color,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom_api::{Attribute, DomBackend, Tag};
    use crate::native::{dom::NativeDom, geom::Size, id::WindowId};

    /// Exercises the real wgpu pipeline, glyph atlas, and text rasterization offscreen.
    /// 离屏验证真实 wgpu Pipeline、glyph atlas 与文本光栅化。
    #[test]
    #[ignore = "requires a GPU adapter; run with --ignored"]
    fn offscreen_gpu_render_draws_counter_pixels() {
        let mut dom = NativeDom::new_offscreen(
            WindowId::new(1),
            Rc::new(std::cell::RefCell::new(TextEngine::new())),
            Size::new(320.0, 200.0),
            1.0,
        );
        let button = dom.create_element(Tag::Button);
        dom.set_attribute(&button, Attribute::AriaLabel, Some("Increment counter"));
        let prefix = dom.create_text("Count: ");
        let value = dom.create_text("0");
        dom.insert_before(&button, &prefix, None);
        dom.insert_before(&button, &value, None);
        dom.layout(super::super::layout::WindowConstraints::new(
            WindowId::new(1),
            Size::new(320.0, 200.0),
        ));
        let list = dom.paint(WindowId::new(1), Size::new(320.0, 200.0));

        let mut shared = SharedRenderContext::new().expect("GPU adapter and device");
        let pixels = shared
            .render_to_rgba(&list, 320, 200, 1.0)
            .expect("offscreen render");
        assert_eq!(pixels.len(), 320 * 200 * 4);
        let painted = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 32 || pixel[1] > 32 || pixel[2] > 32)
            .count();
        assert!(painted > 1_000, "painted pixels: {painted}");
        assert!(shared.stats().rasterized_glyphs > 0);
    }
}
