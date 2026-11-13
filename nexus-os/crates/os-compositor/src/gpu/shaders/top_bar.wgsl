// top_bar.wgsl: 28px macOS-refined Top Menu Bar procedural shader.
// Renders workspace pills, active state highlights, 1px specular separator, and live telemetry.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct TopBarUniforms {
    viewport_size: vec2<f32>,
    active_workspace: u32,
    num_workspaces: u32,
    cpu_percent: f32,
    ram_gb: f32,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> u: TopBarUniforms;

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32((in_vertex_index << 1u) & 2u);
    let y = f32(in_vertex_index & 2u);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// Rounded box SDF
fn rounded_box_sdf(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - (half_size - vec2<f32>(r, r));
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let px = in.uv * u.viewport_size;

    // Base translucent dark backdrop: rgba(16, 18, 22, 0.94)
    var col = vec3<f32>(0.063, 0.071, 0.086);
    var alpha = 0.94;

    // 1px specular bottom border
    if (px.y >= u.viewport_size.y - 1.0) {
        col = vec3<f32>(0.24, 0.27, 0.32);
        alpha = 0.98;
    }

    // Left side: Workspaces pills
    let pill_start_x = 14.0;
    let pill_w = 26.0;
    let pill_h = 18.0;
    let pill_spacing = 6.0;

    for (var i = 0u; i < u.num_workspaces; i = i + 1u) {
        let cx = pill_start_x + f32(i) * (pill_w + pill_spacing) + pill_w * 0.5;
        let cy = 14.0;
        let p = px - vec2<f32>(cx, cy);
        let d = rounded_box_sdf(p, vec2<f32>(pill_w * 0.5, pill_h * 0.5), 5.0);

        if (d <= 0.0) {
            let fw = max(fwidth(d), 0.75);
            let mask = 1.0 - smoothstep(-fw * 0.5, fw * 0.5, d);
            if (i + 1u == u.active_workspace) {
                // Active workspace: macOS system blue pill with top specular rim
                let active_color = vec3<f32>(0.23, 0.51, 0.96);
                let rim = select(0.0, 0.15, p.y < -pill_h * 0.2);
                col = mix(col, active_color + vec3<f32>(rim), mask * 0.95);
            } else {
                // Inactive workspace pill
                let inactive_color = vec3<f32>(0.14, 0.16, 0.20);
                col = mix(col, inactive_color, mask * 0.80);
            }
        }
    }

    // Right side: Telemetry badge
    let telem_w = 120.0;
    let telem_h = 18.0;
    let telem_cx = u.viewport_size.x - 14.0 - telem_w * 0.5;
    let telem_cy = 14.0;
    let tp = px - vec2<f32>(telem_cx, telem_cy);
    let td = rounded_box_sdf(tp, vec2<f32>(telem_w * 0.5, telem_h * 0.5), 5.0);

    if (td <= 0.0) {
        let mask = 1.0 - smoothstep(-0.5, 0.5, td);
        col = mix(col, vec3<f32>(0.11, 0.13, 0.17), mask * 0.85);

        // CPU gauge mini-bar
        let bar_start = tp.x + telem_w * 0.5 - 10.0;
        if (abs(tp.y) < 3.0 && bar_start >= 0.0 && bar_start <= 40.0) {
            let fill = clamp(u.cpu_percent / 100.0, 0.05, 1.0) * 40.0;
            if (bar_start <= fill) {
                let bar_color = select(vec3<f32>(0.2, 0.8, 0.4), vec3<f32>(0.9, 0.25, 0.25), u.cpu_percent > 80.0);
                col = mix(col, bar_color, 0.9);
            }
        }
    }

    return vec4<f32>(col * alpha, alpha);
}
