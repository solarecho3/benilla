// Night-sky stars (`Stars.m2`): each patch's BLP in authored gamma (`WorldAssets::texture` is
// non-sRGB), times the star-curve fade on `base_color` alpha. Blend-2 cards cover premultiplied;
// blend 3/4 glow cards add (`StarExt.additive.x`, `(ONE, ONE)` in specialize). Depth is the far
// pin in `sky_vertex.wgsl`.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::VertexOutput,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> star_add: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    let c = pbr_input.material.base_color; // texel RGB (gamma) × white, alpha × the curve
    let a = c.a;
    let rgb = c.rgb * a;
    if star_add.x >= 0.5 {
        // Glow: colour already weighted; alpha 0 so a covering blend cannot stain the dome.
        return vec4<f32>(rgb, 0.0);
    }
    return vec4<f32>(rgb, a);
}
