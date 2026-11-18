struct FrameUniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    is_dark: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

struct MaterialUniforms {
    base_color: vec4<f32>,
    emissive: vec3<f32>,
    metallic: f32,
    roughness: f32,
    has_texture: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> frame: FrameUniforms;

@group(1) @binding(0)
var<uniform> mat: MaterialUniforms;
@group(1) @binding(1)
var base_tex: texture_2d<f32>;
@group(1) @binding(2)
var base_sampler: sampler;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let wp = frame.model * vec4<f32>(in.pos, 1.0);
    out.world_pos = wp.xyz;
    out.world_normal = normalize((frame.model * vec4<f32>(in.normal, 0.0)).xyz);
    out.clip_pos = frame.mvp * vec4<f32>(in.pos, 1.0);
    out.uv = in.uv;
    return out;
}

fn studio_env(r: vec3<f32>) -> vec3<f32> {
    var col = mix(
        vec3<f32>(0.34, 0.36, 0.40),
        vec3<f32>(0.80, 0.84, 0.90),
        clamp(r.y * 0.5 + 0.5, 0.0, 1.0),
    );
    let top_box = smoothstep(0.55, 0.88, r.y);
    col += vec3<f32>(0.68, 0.70, 0.74) * top_box;

    let key_box = smoothstep(0.62, 0.94, dot(r, normalize(vec3<f32>(-0.55, 0.65, 0.52))));
    col += vec3<f32>(0.90, 0.91, 0.94) * key_box;

    let rim_strip = smoothstep(0.68, 0.96, dot(r, normalize(vec3<f32>(0.75, 0.32, -0.55))));
    col += vec3<f32>(0.40, 0.48, 0.58) * rim_strip;
    return col;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var albedo = mat.base_color.rgb;
    var alpha = mat.base_color.a;

    if (mat.has_texture > 0.5) {
        let tex_sample = textureSample(base_tex, base_sampler, in.uv);
        albedo *= tex_sample.rgb;
        alpha *= tex_sample.a;
    }

    let n = normalize(in.world_normal);
    let cam_pos = vec3<f32>(0.0, 0.05, 2.75);
    let v = normalize(cam_pos - in.world_pos);
    let r = reflect(-v, n);

    let key_dir = normalize(vec3<f32>(-0.55, 0.72, 0.52));
    let fill_dir = normalize(vec3<f32>(0.78, 0.15, 0.55));
    let rim_dir = normalize(vec3<f32>(0.15, 0.45, -0.88));

    let key_diff = max(dot(n, key_dir), 0.0);
    let fill_diff = max(dot(n, fill_dir), 0.0) * 0.28;
    let rim_diff = pow(1.0 - max(dot(n, v), 0.0), 2.6) * max(dot(n, rim_dir), 0.0) * 0.30;
    let hemi = 0.26 + 0.14 * n.y;

    let f0 = mix(vec3<f32>(0.04), albedo, mat.metallic);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(n, v), 0.0), 4.0);

    let env = studio_env(r);
    let spec_power = mix(64.0, 12.0, clamp(mat.roughness, 0.06, 1.0));
    let half_v = normalize(key_dir + v);
    let spec = pow(max(dot(n, half_v), 0.0), spec_power) * mix(0.45, 0.08, mat.roughness);

    let diffuse = albedo * (1.0 - mat.metallic) * (hemi + key_diff * 0.54 + fill_diff);
    var color = diffuse + env * fresnel * mix(0.42, 0.10, mat.roughness) + vec3<f32>(spec) + vec3<f32>(rim_diff) + mat.emissive;

    // Balanced tone mapping that preserves 3D curvature shading on white AirPods
    color = color / (color + vec3<f32>(0.32)) * 1.04;
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));

    return vec4<f32>(color, alpha);
}
