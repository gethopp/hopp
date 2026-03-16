// YUV-to-RGB conversion shader with center-crop UV transform.
//
// Adapted from the livekit yuv_shader.wgsl example with added
// center-crop logic for aspect-ratio-aware rendering.
//
// Uses 3 separate R8Unorm textures for Y, U, V planes (I420 format).
// BT.709 color space conversion (standard for HD video).

struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Full-screen triangle: 3 vertices, no vertex buffer needed.
@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VSOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0)
    );
    let p = pos[vid];
    var out: VSOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = 0.5 * (p + vec2<f32>(1.0, 1.0));
    return out;
}

@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var y_tex: texture_2d<f32>;
@group(0) @binding(2) var u_tex: texture_2d<f32>;
@group(0) @binding(3) var v_tex: texture_2d<f32>;

struct Params {
    src_w: u32,
    src_h: u32,
    y_tex_w: u32,
    uv_tex_w: u32,
    tile_aspect_num: u32,
    tile_aspect_den: u32,
    tile_w: f32,
    tile_h: f32,
    corner_radius: f32,
    stretch_to_fill: u32,
};
@group(0) @binding(4) var<uniform> params: Params;

// Signed distance to a rounded rectangle centered at the origin.
// `half_size` is half the rectangle dimensions, `radius` is the corner radius.
// Returns negative inside, positive outside.
fn rounded_rect_sdf(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - radius;
}

// BT.601 YUV to RGB conversion (limited / studio-swing range).
// HD webcam feeds from WebRTC use BT.709; using BT.601 here would
// desaturate colours noticeably.
fn yuv_to_rgb(y: f32, u: f32, v: f32) -> vec3<f32> {
    let c = y - (16.0 / 255.0);
    let d = u - 0.5;
    let e = v - 0.5;
    let r = 1.164 * c + 1.596 * e;
    let g = 1.164 * c - 0.392 * d - 0.813 * e;
    let b = 1.164 * c + 2.017 * d;
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in_: VSOut) -> @location(0) vec4<f32> {
    let src_w = f32(params.src_w);
    let src_h = f32(params.src_h);
    let y_tex_w = f32(params.y_tex_w);
    let uv_tex_w = f32(params.uv_tex_w);

    // Flip vertically (wgpu texture origin is top-left, UV origin bottom-left)
    let flipped = vec2<f32>(in_.uv.x, 1.0 - in_.uv.y);

    // Center-crop: compute UVs that center-crop source to fill tile.
    // When stretch_to_fill is set, skip the crop and use raw UVs (stretches to fill).
    var crop_uv = flipped;
    if (params.stretch_to_fill == 0u) {
        let src_aspect = src_w / src_h;
        let tile_aspect_val = f32(params.tile_aspect_num) / f32(params.tile_aspect_den);
        if (src_aspect > tile_aspect_val) {
            // Source wider than tile: crop sides
            let scale = tile_aspect_val / src_aspect;
            crop_uv.x = (crop_uv.x - 0.5) * scale + 0.5;
        } else {
            // Source taller than tile: crop top/bottom
            let scale = src_aspect / tile_aspect_val;
            crop_uv.y = (crop_uv.y - 0.5) * scale + 0.5;
        }
    }

    // Scale X to avoid sampling padded columns (256-byte alignment)
    let uv_y = vec2<f32>(crop_uv.x * (src_w / y_tex_w), crop_uv.y);
    let uv_uv = vec2<f32>(crop_uv.x * ((src_w * 0.5) / uv_tex_w), crop_uv.y);

    let y = textureSample(y_tex, samp, uv_y).r;
    let u = textureSample(u_tex, samp, uv_uv).r;
    let v = textureSample(v_tex, samp, uv_uv).r;

    let rgb = yuv_to_rgb(y, u, v);

    // Optional rounded-corner mask. When radius is <= 0, skip masking entirely.
    // We intentionally use two paths:
    // - Camera tiles (stretch_to_fill == 0): use CPU-provided tile_w/tile_h.
    // - Screenshare   (stretch_to_fill != 0): derive tile size from UV derivatives
    //   so resize cannot produce stale-size clipping artifacts.
    var alpha = 1.0;
    if (params.corner_radius > 0.0) {
        var mask_tile_size = vec2<f32>(params.tile_w, params.tile_h);
        if (params.stretch_to_fill != 0u) {
            let duv_dx = max(abs(dpdx(in_.uv.x)), 1e-6);
            let duv_dy = max(abs(dpdy(in_.uv.y)), 1e-6);
            mask_tile_size = vec2<f32>(1.0 / duv_dx, 1.0 / duv_dy);
        }

        let pixel = in_.uv * mask_tile_size;
        let center = mask_tile_size * 0.5;
        let half_size = center;
        let dist = rounded_rect_sdf(pixel - center, half_size, params.corner_radius);
        // Anti-aliased edge: smoothstep over ~1px
        alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    }

    return vec4<f32>(rgb * alpha, alpha);
}
