// command_bar.wgsl: 680x420 macOS Spotlight / Command Center floating panel shader.
// Renders continuous squircle backdrop, 1px inner specular rim, search bar, and action rows.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct CommandBarUniforms {
    viewport_size: vec2<f32>,
    selected_index: u32,
    anim_progress: f32,
    _pad: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> u: CommandBarUniforms;

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32((in_vertex_index << 1u) & 2u);
    let y = f32(in_vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// L4-norm super-ellipse SDF
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

// Rounded box SDF
fn rounded_box_sdf(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - (half_size - vec2<f32>(r, r));
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let px = in.uv * u.viewport_size;
    let center = u.viewport_size * 0.5;
    let p = px - center;
    let half_size = u.viewport_size * 0.5 - vec2<f32>(12.0, 12.0);

    let dist = squircle_sdf(p, half_size, 16.0);

    // Outer shadow / glow
    let shadow_dist = max(dist, 0.0);
    let shadow = exp(-shadow_dist * 0.15) * 0.45;

    if (dist > 0.0) {
        return vec4<f32>(vec3<f32>(0.0), shadow);
    }

    // Inside main squircle: Dark translucent acrylic
    var col = vec3<f32>(0.086, 0.094, 0.114);
    var alpha = 0.95;

    // 1px inner specular rim
    let inner_dist = abs(dist + 1.0);
    let specular = 1.0 - smoothstep(0.0, 1.0, inner_dist);
    let top_rim = select(0.0, 0.25, p.y < 0.0);
    col += vec3<f32>(0.35, 0.38, 0.45) * specular * (0.4 + top_rim);

    // Search input field box
    let search_cy = 40.0;
    let search_p = px - vec2<f32>(center.x, search_cy);
    let search_half = vec2<f32>(half_size.x - 16.0, 20.0);
    let search_d = rounded_box_sdf(search_p, search_half, 8.0);
    if (search_d <= 0.0) {
        col = mix(col, vec3<f32>(0.13, 0.15, 0.19), 0.85);
        let icon_p = search_p + vec2<f32>(search_half.x - 20.0, 0.0);
        if (length(icon_p) < 4.5) {
            col = vec3<f32>(0.38, 0.65, 0.98);
        }
    }

    // Separator line under search box at y = 72.0
    if (abs(px.y - 72.0) <= 0.75 && abs(p.x) < half_size.x - 12.0) {
        col = vec3<f32>(0.20, 0.22, 0.27);
    }

    // Action item rows
    let row_start_y = 96.0;
    let row_h = 44.0;
    let row_spacing = 6.0;

    for (var i = 0u; i < 5u; i = i + 1u) {
        let row_cy = row_start_y + f32(i) * (row_h + row_spacing) + row_h * 0.5;
        let row_p = px - vec2<f32>(center.x, row_cy);
        let row_half = vec2<f32>(half_size.x - 16.0, row_h * 0.5);
        let row_d = rounded_box_sdf(row_p, row_half, 8.0);

        if (row_d <= 0.0) {
            if (i == u.selected_index) {
                let sel_color = vec3<f32>(0.23, 0.51, 0.96);
                let rim = select(0.0, 0.15, row_p.y < -row_half.y * 0.3);
                col = mix(col, sel_color + vec3<f32>(rim), 0.90);
            } else {
                col = mix(col, vec3<f32>(0.11, 0.13, 0.17), 0.50);
            }
        }
    }

    return vec4<f32>(col * alpha, alpha);
}
