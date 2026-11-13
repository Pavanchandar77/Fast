// squircle.wgsl: Single-pass L4-norm super-ellipse SDF with Quartz specular inner highlight
// Anti-aliased via fwidth() and smoothstep() for 120Hz continuous-curvature rendering.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    rect_center: vec2<f32>,
    rect_size: vec2<f32>,
    corner_radius: f32,
    border_width: f32,
    _pad0: vec2<f32>,
    border_color: vec4<f32>,
    specular_color: vec4<f32>,
    glow_color: vec4<f32>,
    viewport_size: vec2<f32>,
    specular_intensity: f32,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

// Fullscreen quad generated procedurally from vertex index: 0, 1, 2
@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32((in_vertex_index << 1u) & 2u);
    let y = f32(in_vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// L4-norm super-ellipse SDF: (|x|^4 + |y|^4)^0.25 - r
fn squircle_sdf(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let d = abs(p) - (half_size - vec2<f32>(r, r));
    if (d.x > 0.0 && d.y > 0.0) {
        let dx4 = d.x * d.x * d.x * d.x;
        let dy4 = d.y * d.y * d.y * d.y;
        return pow(dx4 + dy4, 0.25) - r;
    } else {
        return max(d.x, d.y) - r;
    }
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel_coord = in.uv * u.viewport_size;
    let p = pixel_coord - u.rect_center;
    let half_size = u.rect_size * 0.5;

    // Evaluate continuous curvature SDF
    let dist = squircle_sdf(p, half_size, u.corner_radius);

    // Screen-space derivative anti-aliasing
    let fw = max(fwidth(dist), 0.75);
    let outer_alpha = 1.0 - smoothstep(-fw * 0.5, fw * 0.5, dist);
    let inner_dist = dist + u.border_width;
    let inner_mask = smoothstep(-fw * 0.5, fw * 0.5, inner_dist);

    // Border stroke mask
    let stroke = outer_alpha * inner_mask;

    // Immediately discard / return pure transparent outside the 1px border stroke
    // Ensures desktop and underlying windows are 100% visible through the overlay
    if (stroke <= 0.001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // 1px specular inner highlight on top edge and upper fillet
    let specular_dist = abs(dist + 0.75);
    let specular_mask = 1.0 - smoothstep(0.0, 1.0, specular_dist);
    let normal_y = select(0.0, -p.y / max(abs(p.y), 1.0), p.y < 0.0);
    let specular_factor = max(normal_y, 0.25) * u.specular_intensity;

    // Subtle outer glow
    let glow_dist = max(dist, 0.0);
    let glow = exp(-glow_dist * 0.2) * select(1.0, 0.0, dist < 0.0);

    // Composite final color with pre-multiplied alpha
    var col = u.border_color.rgb * stroke * u.border_color.a;
    col += u.specular_color.rgb * (specular_mask * stroke * specular_factor * u.specular_color.a);
    col += u.glow_color.rgb * (glow * u.glow_color.a);

    let total_alpha = clamp(stroke * u.border_color.a + glow * u.glow_color.a, 0.0, 1.0);

    return vec4<f32>(col, total_alpha);
}
