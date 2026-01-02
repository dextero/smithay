struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

struct GlobalUniforms {
    projection: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: GlobalUniforms;

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pos = globals.projection * vec4<f32>(model.position, 0.0, 1.0);
    out.clip_position = pos;
    out.tex_coords = model.tex_coords;
    return out;
}

@group(1) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(1) @binding(1)
var s_diffuse: sampler;

struct RenderUniforms {
    color: vec4<f32>,
    alpha: f32,
    has_texture: u32,
    _padding: vec2<u32>,
};

@group(2) @binding(0)
var<uniform> uniforms: RenderUniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (uniforms.has_texture != 0u) {
        let tex_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
        return tex_color * uniforms.alpha;
    } else {
        return uniforms.color;
    }
}
