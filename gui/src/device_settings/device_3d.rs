//! Hardware-accelerated glTF 2.0 (`.glb`) PBR 3D showcase widget (`wgpu` / `iced::widget::shader`).
//!
//! Renders embedded `.glb` models (AirPods Pro buds & MagSafe case, Soundcore Q30 in black & gold)
//! with hardware `Depth24Plus` Z-buffering, embedded textures, studio softbox PBR lighting,
//! and interactive drag-to-spin with spring momentum handoff.

use std::{
    f32::consts::TAU,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use cosmic::{
    Element,
    iced::{
        Length, Point, Rectangle, mouse, wgpu,
        widget::shader::{self, Pipeline, Primitive, Shader, Viewport},
        window,
    },
};

use crate::spring::{Spring, SpringParams};

const AIRPODS_GLB: &[u8] = include_bytes!("../../resources/models/airpods_pro_2.glb");
const Q30_GLB: &[u8] = include_bytes!("../../resources/models/soundcore_q30.glb");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device3DModel {
    AirPodsBuds,
    AirPodsCase,
    SoundcoreQ30,
}

impl Device3DModel {
    const fn slot_index(self) -> usize {
        match self {
            Self::AirPodsBuds => 0,
            Self::AirPodsCase => 1,
            Self::SoundcoreQ30 => 2,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct FrameUniforms {
    mvp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    is_dark: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MaterialUniforms {
    base_color: [f32; 4],
    emissive: [f32; 3],
    metallic: f32,
    roughness: f32,
    has_texture: f32,
    _pad: [f32; 2],
}

fn as_bytes<T: Copy>(val: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((val as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

fn slice_as_bytes<T: Copy>(slice: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), std::mem::size_of_val(slice)) }
}

// --- Minimal 4x4 Column-Major Matrix Math ---

fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0_f32; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            out[c][r] = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

fn mat4_transform_pos(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let x = m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0];
    let y = m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1];
    let z = m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2];
    [x, y, z]
}

fn mat4_transform_dir(m: [[f32; 4]; 4], n: [f32; 3]) -> [f32; 3] {
    let x = m[0][0] * n[0] + m[1][0] * n[1] + m[2][0] * n[2];
    let y = m[0][1] * n[0] + m[1][1] * n[1] + m[2][1] * n[2];
    let z = m[0][2] * n[0] + m[1][2] * n[1] + m[2][2] * n[2];
    let len = (x * x + y * y + z * z).sqrt().max(1e-6);
    [x / len, y / len, z / len]
}

fn mat4_translate(x: f32, y: f32, z: f32) -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [x, y, z, 1.0],
    ]
}

fn mat4_rotate_y(a: f32) -> [[f32; 4]; 4] {
    let (s, c) = a.sin_cos();
    [
        [c, 0.0, -s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [s, 0.0, c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn mat4_rotate_x(a: f32) -> [[f32; 4]; 4] {
    let (s, c) = a.sin_cos();
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, c, s, 0.0],
        [0.0, -s, c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn mat4_perspective(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y_rad * 0.5).tan();
    let nf = 1.0 / (near - far);
    [
        [f / aspect.max(0.1), 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far * nf, -1.0],
        [0.0, 0.0, near * far * nf, 0.0],
    ]
}

// --- CPU-side Parsed glTF Model Data ---

struct CpuPrimitive {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    base_color: [f32; 4],
    emissive: [f32; 3],
    metallic: f32,
    roughness: f32,
    texture_idx: Option<usize>,
}

struct CpuTexture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct CpuModel {
    primitives: Vec<CpuPrimitive>,
    textures: Vec<CpuTexture>,
}

fn decode_gltf_image(data: &gltf::image::Data) -> CpuTexture {
    use gltf::image::Format;
    let rgba = match data.format {
        Format::R8G8B8A8 => data.pixels.clone(),
        Format::R8G8B8 => {
            let mut out = Vec::with_capacity((data.width * data.height * 4) as usize);
            for chunk in data.pixels.chunks_exact(3) {
                out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            out
        }
        Format::R8 => {
            let mut out = Vec::with_capacity((data.width * data.height * 4) as usize);
            for &g in &data.pixels {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        Format::R8G8 => {
            let mut out = Vec::with_capacity((data.width * data.height * 4) as usize);
            for chunk in data.pixels.chunks_exact(2) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        _ => {
            vec![255; (data.width * data.height * 4) as usize]
        }
    };
    CpuTexture {
        width: data.width,
        height: data.height,
        rgba,
    }
}

fn load_glb_model(bytes: &[u8], root_node_idx: Option<usize>, is_q30: bool) -> CpuModel {
    let (doc, buffers, images) = gltf::import_slice(bytes).expect("valid embedded GLB");
    let textures: Vec<CpuTexture> = images.iter().map(decode_gltf_image).collect();
    let mut primitives = Vec::new();

    fn visit_node(
        node: gltf::Node<'_>,
        parent_mat: [[f32; 4]; 4],
        active: bool,
        target_root: Option<usize>,
        buffers: &[gltf::buffer::Data],
        is_q30: bool,
        out: &mut Vec<CpuPrimitive>,
    ) {
        // Skip node 35 (`ovvBCJtLtXvnMKD`): floating black dummy buds above the open case
        if !is_q30 && node.index() == 35 {
            return;
        }
        let local = node.transform().matrix();
        let world = mat4_mul(parent_mat, local);
        let now_active = active || target_root == Some(node.index());

        if now_active && let Some(mesh) = node.mesh() {
            let node_name = node.name().unwrap_or_default().to_ascii_lowercase();
            let mesh_name = mesh.name().unwrap_or_default().to_ascii_lowercase();

            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = prim.reader(|b| Some(&buffers[b.index()]));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let pos_vec: Vec<[f32; 3]> = positions.collect();
                let norm_vec: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map(|n| n.collect())
                    .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; pos_vec.len()]);
                let uv_vec: Vec<[f32; 2]> = reader
                    .read_tex_coords(0)
                    .map(|t| t.into_f32().collect())
                    .unwrap_or_else(|| vec![[0.0, 0.0]; pos_vec.len()]);

                let vertices: Vec<Vertex> = pos_vec
                    .iter()
                    .zip(norm_vec.iter())
                    .zip(uv_vec.iter())
                    .map(|((&p, &n), &uv)| Vertex {
                        pos: mat4_transform_pos(world, p),
                        normal: mat4_transform_dir(world, n),
                        uv,
                    })
                    .collect();

                let indices: Vec<u32> = reader
                    .read_indices()
                    .map(|i| i.into_u32().collect())
                    .unwrap_or_else(|| (0..vertices.len() as u32).collect());

                let mat = prim.material();
                let pbr = mat.pbr_metallic_roughness();
                let mut base_color = pbr.base_color_factor();
                let mut metallic = pbr.metallic_factor();
                let mut roughness = pbr.roughness_factor();
                let emissive = mat.emissive_factor();
                let texture_idx = pbr
                    .base_color_texture()
                    .map(|t| t.texture().source().index());

                // Style the Soundcore Q30 in Black + Champagne Gold accents as requested
                if is_q30 {
                    let is_gold_accent = node_name.contains("cylinder")
                        || mesh_name.contains("cylinder")
                        || node_name.contains("handle")
                        || mesh_name.contains("handle");

                    if is_gold_accent {
                        base_color = [0.86, 0.66, 0.28, 1.0];
                        metallic = 0.52;
                        roughness = 0.28;
                    } else {
                        base_color = [0.075, 0.08, 0.09, 1.0];
                        metallic = 0.05;
                        roughness = 0.42;
                    }
                } else {
                    // glTF 2.0 defaults omitted metallicFactor to 1.0, which makes white
                    // polycarbonate / textured parts look pitch-black or overly reflective.
                    if texture_idx.is_some() || base_color[0] > 0.55 {
                        metallic = 0.04;
                        roughness = roughness.clamp(0.20, 0.45);
                    }
                }

                out.push(CpuPrimitive {
                    vertices,
                    indices,
                    base_color,
                    emissive,
                    metallic,
                    roughness,
                    texture_idx,
                });
            }
        }

        for child in node.children() {
            visit_node(child, world, now_active, target_root, buffers, is_q30, out);
        }
    }

    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let start_active = root_node_idx.is_none();
    for scene in doc.scenes() {
        for node in scene.nodes() {
            visit_node(
                node,
                identity,
                start_active,
                root_node_idx,
                &buffers,
                is_q30,
                &mut primitives,
            );
        }
    }

    // Center at origin and normalize scale to fit nicely in the camera frustum
    let mut min_p = [f32::MAX; 3];
    let mut max_p = [f32::MIN; 3];
    for prim in &primitives {
        for v in &prim.vertices {
            for k in 0..3 {
                min_p[k] = min_p[k].min(v.pos[k]);
                max_p[k] = max_p[k].max(v.pos[k]);
            }
        }
    }

    if min_p[0] < max_p[0] {
        let center = [
            (min_p[0] + max_p[0]) * 0.5,
            (min_p[1] + max_p[1]) * 0.5,
            (min_p[2] + max_p[2]) * 0.5,
        ];
        let extent = (max_p[0] - min_p[0])
            .max(max_p[1] - min_p[1])
            .max(max_p[2] - min_p[2])
            .max(1e-5);
        let target_extent = if is_q30 { 1.48 } else { 1.42 };
        let scale = target_extent / extent;

        for prim in &mut primitives {
            for v in &mut prim.vertices {
                v.pos = [
                    (v.pos[0] - center[0]) * scale,
                    (v.pos[1] - center[1]) * scale,
                    (v.pos[2] - center[2]) * scale,
                ];
            }
        }
    }

    CpuModel {
        primitives,
        textures,
    }
}

fn cached_model(model: Device3DModel) -> &'static Arc<CpuModel> {
    static BUDS: OnceLock<Arc<CpuModel>> = OnceLock::new();
    static CASE: OnceLock<Arc<CpuModel>> = OnceLock::new();
    static Q30: OnceLock<Arc<CpuModel>> = OnceLock::new();

    match model {
        Device3DModel::AirPodsBuds => {
            BUDS.get_or_init(|| Arc::new(load_glb_model(AIRPODS_GLB, Some(3), false)))
        }
        Device3DModel::AirPodsCase => {
            CASE.get_or_init(|| Arc::new(load_glb_model(AIRPODS_GLB, Some(26), false)))
        }
        Device3DModel::SoundcoreQ30 => {
            Q30.get_or_init(|| Arc::new(load_glb_model(Q30_GLB, None, true)))
        }
    }
}

// --- GPU Pipeline & Uploaded Mesh Resources ---

struct GpuMeshPart {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    material_bind_group: wgpu::BindGroup,
}

struct GpuModel {
    parts: Vec<GpuMeshPart>,
}

struct DepthTarget {
    width: u32,
    height: u32,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct FrameSlot {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    phys_bounds: [f32; 4],
}

pub struct Device3DPipeline {
    pipeline: wgpu::RenderPipeline,
    slots: [Mutex<FrameSlot>; 3],
    models: [GpuModel; 3],
    depth: Mutex<Option<DepthTarget>>,
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &CpuTexture,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("device_3d_tex"),
        size: wgpu::Extent3d {
            width: tex.width.max(1),
            height: tex.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &tex.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * tex.width.max(1)),
            rows_per_image: Some(tex.height.max(1)),
        },
        wgpu::Extent3d {
            width: tex.width.max(1),
            height: tex.height.max(1),
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn upload_gpu_model(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mat_bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    fallback_view: &wgpu::TextureView,
    cpu: &CpuModel,
) -> GpuModel {
    use wgpu::util::DeviceExt;

    let tex_views: Vec<wgpu::TextureView> = cpu
        .textures
        .iter()
        .map(|t| upload_texture(device, queue, t))
        .collect();

    let mut parts = Vec::with_capacity(cpu.primitives.len());
    for prim in &cpu.primitives {
        if prim.vertices.is_empty() || prim.indices.is_empty() {
            continue;
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("device_3d_vb"),
            contents: slice_as_bytes(&prim.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("device_3d_ib"),
            contents: slice_as_bytes(&prim.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let mat_uniform = MaterialUniforms {
            base_color: prim.base_color,
            emissive: prim.emissive,
            metallic: prim.metallic,
            roughness: prim.roughness,
            has_texture: if prim.texture_idx.is_some() { 1.0 } else { 0.0 },
            _pad: [0.0; 2],
        };
        let mat_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("device_3d_mat"),
            contents: as_bytes(&mat_uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let tex_view = prim
            .texture_idx
            .and_then(|idx| tex_views.get(idx))
            .unwrap_or(fallback_view);

        let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("device_3d_mat_bg"),
            layout: mat_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: mat_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        parts.push(GpuMeshPart {
            vertex_buffer,
            index_buffer,
            index_count: prim.indices.len() as u32,
            material_bind_group,
        });
    }

    GpuModel { parts }
}

impl Pipeline for Device3DPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("device_3d_glb_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("device_3d.wgsl").into()),
        });

        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("device_3d_frame_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let mat_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("device_3d_mat_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("device_3d_pipeline_layout"),
            bind_group_layouts: &[&frame_bgl, &mat_bgl],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("device_3d_glb_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
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
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let make_slot = |label: &str| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<FrameUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &frame_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            Mutex::new(FrameSlot {
                buffer,
                bind_group,
                phys_bounds: [0.0, 0.0, 1.0, 1.0],
            })
        };

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("device_3d_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let fallback_view = upload_texture(
            device,
            queue,
            &CpuTexture {
                width: 1,
                height: 1,
                rgba: vec![255, 255, 255, 255],
            },
        );

        let models = [
            upload_gpu_model(
                device,
                queue,
                &mat_bgl,
                &sampler,
                &fallback_view,
                cached_model(Device3DModel::AirPodsBuds),
            ),
            upload_gpu_model(
                device,
                queue,
                &mat_bgl,
                &sampler,
                &fallback_view,
                cached_model(Device3DModel::AirPodsCase),
            ),
            upload_gpu_model(
                device,
                queue,
                &mat_bgl,
                &sampler,
                &fallback_view,
                cached_model(Device3DModel::SoundcoreQ30),
            ),
        ];

        Self {
            pipeline,
            slots: [
                make_slot("device_3d_buds"),
                make_slot("device_3d_case"),
                make_slot("device_3d_q30"),
            ],
            models,
            depth: Mutex::new(None),
        }
    }
}

#[derive(Debug)]
struct Device3DPrimitive {
    model: Device3DModel,
    yaw: f32,
    pitch: f32,
    bob_y: f32,
    is_dark: bool,
}

impl Primitive for Device3DPrimitive {
    type Pipeline = Device3DPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let scale = viewport.scale_factor() as f32;
        let phys_bounds = [
            bounds.x * scale,
            bounds.y * scale,
            (bounds.width * scale).max(1.0),
            (bounds.height * scale).max(1.0),
        ];
        let phys_size = viewport.physical_size();

        // Ensure hardware depth texture matches current target size
        {
            let mut depth_guard = pipeline.depth.lock().unwrap();
            let needs_realloc = depth_guard
                .as_ref()
                .is_none_or(|d| d.width != phys_size.width || d.height != phys_size.height);
            if needs_realloc && phys_size.width > 0 && phys_size.height > 0 {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("device_3d_depth"),
                    size: wgpu::Extent3d {
                        width: phys_size.width,
                        height: phys_size.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth24Plus,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                *depth_guard = Some(DepthTarget {
                    width: phys_size.width,
                    height: phys_size.height,
                    _texture: texture,
                    view,
                });
            }
        }

        let aspect = bounds.width / bounds.height.max(1.0);
        let model_mat = mat4_mul(
            mat4_translate(0.0, self.bob_y, 0.0),
            mat4_mul(mat4_rotate_x(self.pitch), mat4_rotate_y(self.yaw)),
        );
        let view_mat = mat4_translate(0.0, -0.02, -2.65);
        let proj_mat = mat4_perspective(34.0_f32.to_radians(), aspect, 0.1, 20.0);
        let mvp = mat4_mul(proj_mat, mat4_mul(view_mat, model_mat));

        let uniforms = FrameUniforms {
            mvp,
            model: model_mat,
            is_dark: if self.is_dark { 1.0 } else { 0.0 },
            _pad: [0.0; 3],
        };

        let mut slot = pipeline.slots[self.model.slot_index()].lock().unwrap();
        slot.phys_bounds = phys_bounds;
        queue.write_buffer(&slot.buffer, 0, as_bytes(&uniforms));
    }

    fn draw(&self, _pipeline: &Self::Pipeline, _render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        // Use render() so we can attach a hardware Depth24Plus Z-buffer
        false
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let depth_guard = pipeline.depth.lock().unwrap();
        let Some(depth) = depth_guard.as_ref() else {
            return;
        };
        let slot = pipeline.slots[self.model.slot_index()].lock().unwrap();
        let gpu_model = &pipeline.models[self.model.slot_index()];

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("device_3d_glb_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        pass.set_viewport(
            slot.phys_bounds[0],
            slot.phys_bounds[1],
            slot.phys_bounds[2],
            slot.phys_bounds[3],
            0.0,
            1.0,
        );
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width.max(1),
            clip_bounds.height.max(1),
        );
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &slot.bind_group, &[]);

        for part in &gpu_model.parts {
            pass.set_bind_group(1, &part.material_bind_group, &[]);
            pass.set_vertex_buffer(0, part.vertex_buffer.slice(..));
            pass.set_index_buffer(part.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..part.index_count, 0, 0..1);
        }
    }
}

struct Device3DProgram {
    model: Device3DModel,
}

struct Device3DState {
    yaw: f32,
    yaw_velocity: f32,
    pitch_spring: Spring,
    last_tick: Option<Instant>,
    dragging: bool,
    last_cursor: Point,
    last_move_time: Option<Instant>,
}

impl Default for Device3DState {
    fn default() -> Self {
        Self {
            yaw: 0.35,
            yaw_velocity: 0.62,
            pitch_spring: Spring::new(0.10, SpringParams::SNAPPY),
            last_tick: None,
            dragging: false,
            last_cursor: Point::ORIGIN,
            last_move_time: None,
        }
    }
}

impl<M> shader::Program<M> for Device3DProgram {
    type State = Device3DState;
    type Primitive = Device3DPrimitive;

    fn update(
        &self,
        state: &mut Self::State,
        event: &cosmic::iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<M>> {
        const IDLE_SPIN_SPEED: f32 = 0.62;

        match event {
            cosmic::iced::Event::Window(window::Event::RedrawRequested(now)) => {
                let dt = state
                    .last_tick
                    .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32())
                    .clamp(0.0, 0.05);
                state.last_tick = Some(*now);

                if !state.dragging {
                    let blend = 1.0 - (-2.8 * dt).exp();
                    state.yaw_velocity += (IDLE_SPIN_SPEED - state.yaw_velocity) * blend;
                    state.yaw = (state.yaw + state.yaw_velocity * dt) % TAU;
                }
                state.pitch_spring.step(dt);

                return Some(shader::Action::request_redraw());
            }
            cosmic::iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    state.dragging = true;
                    state.last_cursor = pos;
                    state.last_move_time = Some(Instant::now());
                    state.yaw_velocity = 0.0;
                    return Some(shader::Action::capture());
                }
            }
            cosmic::iced::Event::Mouse(mouse::Event::CursorMoved { position })
                if state.dragging =>
            {
                let dx = position.x - state.last_cursor.x;
                let dy = position.y - state.last_cursor.y;
                state.yaw = (state.yaw + dx * 0.022) % TAU;

                let next_pitch = (state.pitch_spring.target + dy * 0.012).clamp(-0.50, 0.55);
                state.pitch_spring.snap_to(next_pitch);

                let now = Instant::now();
                if let Some(prev) = state.last_move_time {
                    let dt = now.duration_since(prev).as_secs_f32();
                    if dt > 0.001 {
                        let inst_v = (dx * 0.022) / dt;
                        state.yaw_velocity =
                            (state.yaw_velocity * 0.4 + inst_v * 0.6).clamp(-12.0, 12.0);
                    }
                }
                state.last_cursor = *position;
                state.last_move_time = Some(now);
                return Some(shader::Action::capture());
            }
            cosmic::iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging =>
            {
                state.dragging = false;
                state.pitch_spring.set_target(0.10);
                return Some(shader::Action::capture());
            }
            _ => {}
        }

        if state.last_tick.is_none() {
            state.last_tick = Some(Instant::now());
            return Some(shader::Action::request_redraw());
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        Device3DPrimitive {
            model: self.model,
            yaw: state.yaw,
            pitch: state.pitch_spring.value,
            bob_y: (state.yaw * 2.0).sin() * 0.016,
            is_dark: false,
        }
    }
}

pub fn view_3d_device<'a, M: 'a>(model: Device3DModel, width: f32, height: f32) -> Element<'a, M> {
    Shader::new(Device3DProgram { model })
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}
