//! Rectangles pleins : fonds, sélections, curseurs.
//!
//! Un seul pipeline dessine tous les rectangles d'une frame en un appel, un rectangle par
//! instance ; le tampon d'instances grandit au besoin et n'est jamais réalloué sinon.

use std::ops::Range;

use crate::color::Color;
use crate::geometry::Rect;

/// Rectangle plein d'une couleur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Position et taille, en pixels physiques.
    pub rect: Rect,
    /// Couleur (l'opacité est respectée).
    pub color: Color,
}

impl Quad {
    /// Rectangle `rect` de couleur `color`.
    pub const fn new(rect: Rect, color: Color) -> Self {
        Self { rect, color }
    }
}

/// Données d'une instance, telles que lues par le shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    color: [f32; 4],
}

/// Taille de l'image cible, pour passer des pixels aux coordonnées normalisées.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    screen: [f32; 2],
    _padding: [f32; 2],
}

const SHADER: &str = r"
struct Uniforms {
    screen: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOutput {
    let corner = vec2<f32>(f32(vertex & 1u), f32((vertex >> 1u) & 1u));
    let pixel = rect.xy + corner * rect.zw;
    let ndc = vec2<f32>(pixel.x / uniforms.screen.x * 2.0 - 1.0, 1.0 - pixel.y / uniforms.screen.y * 2.0);
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
";

/// Pipeline et tampons des rectangles.
pub(crate) struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: usize,
    /// Cible en sRGB : les couleurs sont converties en linéaire avant d'être écrites.
    srgb: bool,
}

impl QuadPipeline {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cupride-quads"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cupride-quads-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cupride-quads"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cupride-quads"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cupride-quads"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cupride-quads"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });
        let capacity = 64;
        Self {
            pipeline,
            uniforms,
            bind_group,
            instances: Self::instance_buffer(device, capacity),
            capacity,
            srgb: format.is_srgb(),
        }
    }

    fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cupride-quads-instances"),
            size: (capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Envoie les rectangles au GPU : ceux de `under` (sous le texte) puis ceux de `over`
    /// (par-dessus). Renvoie les plages d'instances à passer à [`QuadPipeline::draw`].
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (width, height): (u32, u32),
        under: &[Quad],
        over: &[Quad],
    ) -> (Range<u32>, Range<u32>) {
        let srgb = self.srgb;
        let convert = |quads: &[Quad]| -> Vec<Instance> {
            quads
                .iter()
                .filter(|q| q.rect.width > 0.0 && q.rect.height > 0.0 && q.color.a > 0)
                .map(|quad| {
                    let r = quad.rect;
                    Instance {
                        rect: [r.x, r.y, r.width, r.height],
                        color: if srgb {
                            quad.color.to_linear()
                        } else {
                            let c = quad.color;
                            [c.r, c.g, c.b, c.a].map(|v| f32::from(v) / 255.0)
                        },
                    }
                })
                .collect()
        };
        let mut instances = convert(under);
        let split = instances.len() as u32;
        instances.extend(convert(over));
        let total = instances.len() as u32;
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                screen: [width.max(1) as f32, height.max(1) as f32],
                _padding: [0.0; 2],
            }),
        );
        if instances.len() > self.capacity {
            self.capacity = instances.len().next_power_of_two();
            self.instances = Self::instance_buffer(device, self.capacity);
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }
        (0..split, split..total)
    }

    /// Dessine les instances `range` envoyées par le dernier [`QuadPipeline::prepare`].
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, range);
    }
}
