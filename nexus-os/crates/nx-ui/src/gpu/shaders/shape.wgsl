// Instanced SDF rounded rects. Matches `sdf::sd_round_box`.
// Colors are straight sRGB. Coverage premultiplies in this shader.
// extra.x: 0 fill, 1 shadow. params: border_w, shadow_blur, shadow_dx, shadow_dy.

struct Frame {
    viewport: vec2<f32>,
    pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) radii: vec4<f32>,
    @location(3) color0: vec4<f32>,
    @location(4) color1: vec4<f32>,
    @location(5) shadow: vec4<f32>,
    @location(6) params: vec4<f32>,
    @location(7) extra: vec4<f32>,
    @location(8) border: vec4<f32>,
    @location(9) clip_rect: vec4<f32>,
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
    var pad = 0.0;
    if (extra.x > 0.5 && extra.x < 1.5) {
        pad = params.y * 3.0 + 8.0;
    }
    let x0 = rect.x - pad;
    let y0 = rect.y - pad;
    let w = max(rect.z + pad * 2.0, 1.0);
    let h = max(rect.w + pad * 2.0, 1.0);
    let px = x0 + corner.x * w;
    let py = y0 + corner.y * h;
    let vp = max(frame.viewport, vec2<f32>(1.0, 1.0));
    let ndc = vec2<f32>(px / vp.x * 2.0 - 1.0, 1.0 - py / vp.y * 2.0);
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.world = vec2<f32>(px, py);
    out.rect = rect;
    out.radii = radii;
    out.color0 = color0;
    out.color1 = color1;
    out.shadow = shadow;
    out.params = params;
    out.extra = extra;
    out.border = border;
    out.clip_rect = clip_rect;
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

@fragment
fn fs_main(frag: VsOut) -> @location(0) vec4<f32> {
    let clip_rect = frag.clip_rect;
    if (clip_rect.z > 0.0 && clip_rect.w > 0.0) {
        if (frag.world.x < clip_rect.x || frag.world.y < clip_rect.y || frag.world.x >= clip_rect.x + clip_rect.z || frag.world.y >= clip_rect.y + clip_rect.w) {
            discard;
        }
    }
    let rect = frag.rect;
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half = max(vec2<f32>(rect.z, rect.w) * 0.5, vec2<f32>(0.0, 0.0));
    var shift = vec2<f32>(0.0, 0.0);
    if (frag.extra.x > 0.5 && frag.extra.x < 1.5) {
        shift = vec2<f32>(frag.params.z, frag.params.w);
    }
    let p = frag.world - center - shift;
    let radius = radius_at(p, frag.radii);
    let smoothing = frag.extra.w;
    let sd = sd_round_box(p, half, radius, smoothing);
    let opacity = frag.extra.z;
    if (frag.extra.x > 0.5 && frag.extra.x < 1.5) {
        let sigma = max(frag.params.y, 0.5);
        let dist = max(sd, 0.0);
        let alpha = exp(-0.5 * dist * dist / (sigma * sigma)) * frag.shadow.a * opacity;
        return vec4<f32>(frag.shadow.rgb * alpha, alpha);
    }
    let dir = vec2<f32>(cos(frag.extra.y), sin(frag.extra.y));
    let denom = max(half, vec2<f32>(1.0, 1.0));
    let gt = clamp(dot(p / denom, dir) * 0.5 + 0.5, 0.0, 1.0);
    let rgb = mix(frag.color0.rgb, frag.color1.rgb, gt);
    let alpha = mix(frag.color0.a, frag.color1.a, gt);
    let cov = cover(sd);
    let bw = frag.params.x;
    if (bw > 0.25) {
        let inner_half = max(half - vec2<f32>(bw, bw), vec2<f32>(0.0, 0.0));
        let inner_r = max(radius - bw, 0.0);
        let inner = cover(sd_round_box(p, inner_half, inner_r, smoothing));
        let fill_a = inner * alpha;
        let border_a = max(cov - inner, 0.0) * frag.border.a;
        let a = (fill_a + border_a) * opacity;
        let out_rgb = rgb * fill_a + frag.border.rgb * border_a;
        return vec4<f32>(out_rgb * opacity, a);
    }
    let a = cov * alpha * opacity;
    return vec4<f32>(rgb * a, a);
}
