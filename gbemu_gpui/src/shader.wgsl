
struct VertexInput {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>
}

alias PaletteUniform = array<vec4<f32>, 4>;

@vertex
fn screen_vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(input.position, 1.0);
    return out;
}


@group(0) @binding(0)
var<uniform> palette: PaletteUniform;
@group(0) @binding(1)
var screen: texture_2d<u32>;

@fragment
fn screen_fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let coord = vec2<u32>(in.clip_position.xy);

    let colour = textureLoad(screen, coord, 0).r;

    let output = palette[colour];

    return output;
}
