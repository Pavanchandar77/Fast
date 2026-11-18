// Vibrancy composite. Samples the blurred backdrop, then saturation, tint, and
// stable film grain. `hash21` matches `material::film_grain`.

struct Frame {
    viewport: vec2<f32>,
    pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var blur_tex: texture_2d<f32>;
@group(0) @binding(2) var blur_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) radii: vec4<f32>,
    @location(3) color0: vec4<f32>,
    @location(4) params: vec4<f32>,
    @location(5) extra: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) radii: vec4<f32>,
    @location(2) color0: vec4<f32>,
    @location(3) color1: vec4<f32>,
    @location(4) shadow: vec4<f32>,
    @location(5) params: vec4<f32>,
    @location(6) extra: vec4<f32>,
    @location(7) border: vec4<f32>,
    @location(8) clip_rect: vec4<f32>,
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
    let px = rect.x + corner.x * max(rect.z, 1.0);
    let py = rect.y + corner.y * max(rect.w, 1.0);
    let vp = max(frame.viewport, vec2<f32>(1.0, 1.0));
    let ndc = vec2<f32>(px / vp.x * 2.0 - 1.0, 1.0 - py / vp.y * 2.0);
    var out: VsOut;
    let consumed = (color1.x + shadow.x + border.x + clip_rect.x) * 0.0;
    out.clip = vec4<f32>(ndc, consumed, 1.0);
    out.world = vec2<f32>(px, py);
    out.rect = rect;
    out.radii = radii;
    out.color0 = color0;
    out.params = params;
    out.extra = extra;
    return out;
}

fn sd_round_box(p: vec2<f32>, half: vec2<f32>, radius: f32, smoothing: f32) -> f32 {
    let limit = max(min(half.x, half.y), 0.0);
    let r = clamp(radius, 0.0, limit);
    let q = abs(p) - half + vec2<f32>(r, r);
    let a = max(q, vec2<f32>(0.0, 0.0));
    let n = 2.0 + 2.5 * clamp(smoothing, 0.0, 1.0);
    let corner = pow(pow(a.x, n) + pow(a.y, n), 1.0 / n);
    return min(max(q.x, q.y), 0.0) + corner - r;
}

fn radius_at(p: vec2<f32>, radii: vec4<f32>) -> f32 {
    let top = select(radii.x, radii.y, p.x >= 0.0);
    let bot = select(radii.w, radii.z, p.x >= 0.0);
    return select(top, bot, p.y >= 0.0);
}

fn cover(sd: f32) -> f32 {
    let aa = max(fwidth(sd), 0.75);
    let t = clamp((sd + aa) / (2.0 * aa), 0.0, 1.0);
    let h = t * t * (3.0 - 2.0 * t);
    return 1.0 - h;
}

fn hash21(p: vec2<f32>) -> f32 {
    let n = sin(p.x * 127.1 + p.y * 311.7) * 43758.5453;
    return fract(n) - 0.5;
}

@fragment
fn fs_main(frag: VsOut) -> @location(0) vec4<f32> {
    let rect = frag.rect;
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half = max(vec2<f32>(rect.z, rect.w) * 0.5, vec2<f32>(0.0, 0.0));
    let p = frag.world - center;
    let sd = sd_round_box(p, half, radius_at(p, frag.radii), frag.extra.w);
    let cov = cover(sd);
    let vp = max(frame.viewport, vec2<f32>(1.0, 1.0));
    let sample = textureSample(blur_tex, blur_samp, frag.world / vp);
    let luma = dot(sample.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let colored = mix(vec3<f32>(luma), sample.rgb, frag.params.y);
    let tinted = mix(colored, frag.color0.rgb, frag.color0.a);
    let grain = hash21(frag.world) * frag.params.x;
    let rgb = tinted + vec3<f32>(grain, grain, grain);
    let a = cov * frag.extra.z;
    return vec4<f32>(rgb * a, a);
}
