// Separable 9-tap gaussian. Weights are packed into vec4s because a uniform
// array of f32 is 16-byte aligned and would not match the Rust kernel.

struct Blur {
    dir_texel: vec4<f32>,
    w0: vec4<f32>,
    w1: vec4<f32>,
    w2: vec4<f32>,
}

@group(0) @binding(0) var<uniform> blur: Blur;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var src_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = pos[vi];
    var out: VsOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, -p.y) * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(frag: VsOut) -> @location(0) vec4<f32> {
    let step = blur.dir_texel.xy;
    let uv = frag.uv;
    var acc = textureSample(src_tex, src_samp, uv + step * -4.0) * blur.w0.x;
    acc = acc + textureSample(src_tex, src_samp, uv + step * -3.0) * blur.w0.y;
    acc = acc + textureSample(src_tex, src_samp, uv + step * -2.0) * blur.w0.z;
    acc = acc + textureSample(src_tex, src_samp, uv + step * -1.0) * blur.w0.w;
    acc = acc + textureSample(src_tex, src_samp, uv) * blur.w1.x;
    acc = acc + textureSample(src_tex, src_samp, uv + step * 1.0) * blur.w1.y;
    acc = acc + textureSample(src_tex, src_samp, uv + step * 2.0) * blur.w1.z;
    acc = acc + textureSample(src_tex, src_samp, uv + step * 3.0) * blur.w1.w;
    acc = acc + textureSample(src_tex, src_samp, uv + step * 4.0) * blur.w2.x;
    return acc;
}
