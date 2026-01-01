struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(position, 0.0, 1.0);
    out.tex_coords = tex_coords;
    return out;
}

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<u32> {
    let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    return vec4<u32>(
        u32(color.r * 255.0),
        u32(color.g * 255.0),
        u32(color.b * 255.0),
        u32(color.a * 255.0)
    );
}
