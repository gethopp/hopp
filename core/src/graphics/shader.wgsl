struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_coords: vec2<f32>,
};

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) texture_coords: vec2<f32>,
};

struct CoordsUniform {
    transform: mat4x4<f32>,
};


@group(1) @binding(0)
var<uniform> coords: CoordsUniform;

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.texture_coords = model.texture_coords;
    out.clip_position = coords.transform * vec4<f32>(model.position, 0.0, 1.0);
    return out;
}

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.texture_coords);
}

@vertex
fn vs_lines_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.texture_coords = model.texture_coords;
    out.clip_position = vec4<f32>(model.position, 0.0, 1.0);
    return out;
}

@vertex
fn vs_click_animation_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.texture_coords = model.texture_coords;
    out.clip_position = coords.transform * vec4<f32>(model.position, 0.0, 1.0);
    return out;
}

@group(2) @binding(0)
var<uniform> radius: f32;

@fragment
fn fs_click_animation_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_diffuse, s_diffuse, in.texture_coords);
    let centered_coords = in.texture_coords - vec2<f32>(0.5, 0.5);
    let dist = length(centered_coords);

    let radius_start = 0.1;
    if radius == radius_start {
        let alpha = 1.0 - smoothstep(radius, radius + 0.1, dist);
        return vec4<f32>(color.rgb, color.a * alpha);
    } else {
        let ring_width = 0.01;
        let edge = 0.02;
        let outer = smoothstep(radius - ring_width - edge, radius - ring_width + edge, dist);
        let inner = 1.0 - smoothstep(radius + ring_width - edge, radius + ring_width + edge, dist);
        let alpha = inner * outer;
        return vec4<f32>(color.rgb, color.a * alpha);
    }
}