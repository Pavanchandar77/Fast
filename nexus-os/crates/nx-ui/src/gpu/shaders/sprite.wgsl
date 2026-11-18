struct Frame {
    viewport: vec2<f32>,
    pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv_rect: vec4<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vi];
    let px = rect.x + corner.x * max(rect.z, 0.0);
    let py = rect.y + corner.y * max(rect.w, 0.0);
    let vp = max(frame.viewport, vec2<f32>(1.0, 1.0));
    let ndc = vec2<f32>(px / vp.x * 2.0 - 1.0, 1.0 - py / vp.y * 2.0);
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = mix(uv_rect.xy, uv_rect.zw, corner);
    out.color = color;
    return out;
}

@fragment
fn fs_main(frag: VsOut) -> @location(0) vec4<f32> {
    return textureSample(atlas, atlas_samp, frag.uv) * frag.color;
}
