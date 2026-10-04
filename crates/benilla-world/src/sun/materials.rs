//! The celestial and star [`ExtendedMaterial`]s, drawn by `celestial.wgsl` and `star.wgsl`.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::sky_order::{sky_pipeline_state, SKY_VERTEX_SHADER};

/// Every disc and glare, blended in gamma space like the reference. A disc takes the horizon clip
/// and fade (`0x6d1960`) under a premultiplied blend; a glare skips it and adds gamma bytes, the
/// reference's SRC_ALPHA, ONE flare blend (`0x7e5a16`), which saturates the sun's core to white.
pub type CelestialMaterial = ExtendedMaterial<StandardMaterial, CelestialExt>;

/// The shader's controls. `fade`: `.x` the horizon ramp slope in sin-elevation
/// ([`DISC_HORIZON_FADE`]); `.y` a brightness on RGB, 1.0; `.z` 0 for a disc, 1 for a glare,
/// which never routes `0x6d1960`; `.w` the disc colour's alpha byte above the band, 1.0 from the
/// 0xFF broadcast and 0 for moon02, whose colour has no writer (`0xce98a4`). Under weather `.w`
/// takes the alpha seed `floor(255·(1−bcc))/255` (`0x6d2c74`), `bcc = min(1, density·4)`. The
/// reference's per-vertex fade is interpolated across [`CelestialExt::span`].
#[derive(Asset, AsBindGroup, Clone, TypePath, Default)]
pub struct CelestialExt {
    #[uniform(100)]
    pub(super) fade: Vec4,
    /// The disc quad's bottom (`.x`) and top (`.y`) edges in sin-elevation, written each frame.
    #[uniform(101)]
    pub(super) span: Vec4,
}

impl MaterialExtension for CelestialExt {
    /// The shared sky vertex stage, the far-depth pin ([`crate::sky_order`]).
    fn vertex_shader() -> ShaderRef {
        SKY_VERTEX_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://benilla_world/shaders/celestial.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        sky_pipeline_state(descriptor);
        Ok(())
    }
}

/// The discs' horizon ramp slope: `0x6d1960`'s `clamp(2.5·height, 0, 1)` at radius 12 is
/// `clamp(30·dir.y, 0, 1)`, a soft edge over the bottom ~1.9° of elevation; at or below 0 it clips.
pub(super) const DISC_HORIZON_FADE: f32 = 30.0;

/// The star patches. Blend-2 cards cover premultiplied in gamma; blend 3/4 glow cards add
/// `(ONE, ONE)` like [`benilla_assets::materials::WowModelExt`], so a nebula does not stain the dome.
pub type StarMaterial = ExtendedMaterial<StandardMaterial, StarExt>;

/// Pipeline key: additive glow (`Stars.m2` blend 3/4) versus premultiplied cover (blend 2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StarPipelineKey {
    additive: bool,
}

impl From<&StarExt> for StarPipelineKey {
    fn from(e: &StarExt) -> Self {
        Self {
            additive: e.additive.x > 0.5,
        }
    }
}

/// `.x` 1: M2 blend 3/4 glow, `(ONE, ONE)` in `specialize`; `.x` 0: premultiplied cover.
/// The star-curve fade rides `StandardMaterial::base_color` alpha, sampled each frame.
#[derive(Asset, AsBindGroup, Clone, TypePath, Default)]
#[bind_group_data(StarPipelineKey)]
pub struct StarExt {
    #[uniform(100)]
    pub(super) additive: Vec4,
}

impl MaterialExtension for StarExt {
    /// The shared sky vertex stage, the far-depth pin ([`crate::sky_order`]).
    fn vertex_shader() -> ShaderRef {
        SKY_VERTEX_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://benilla_world/shaders/star.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        sky_pipeline_state(descriptor);
        if key.bind_group_data.additive {
            // Gamma-space add, matching WowModel glow cards. `AlphaMode::Add` linearises and
            // fattens the nebula's soft edge.
            if let Some(target) = descriptor
                .fragment
                .as_mut()
                .and_then(|f| f.targets.get_mut(0))
                .and_then(|t| t.as_mut())
            {
                target.blend = Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod star_shader_tests {
    use super::{StarExt, StarPipelineKey};

    #[test]
    fn star_fragment_keeps_texel_rgb_in_gamma() {
        let src = include_str!("../shaders/star.wgsl");
        assert!(
            src.contains("c.rgb * a") && !src.contains("vec3<f32>(a)"),
            "star.wgsl must emit texel RGB, not alpha-as-white — Pretty Night nebula cards smeared \
             Stars.blp as white streaks when RGB was discarded"
        );
    }

    #[test]
    fn additive_star_ext_keys_the_glow_pipeline() {
        assert!(!StarPipelineKey::from(&StarExt::default()).additive);
        let glow = StarExt {
            additive: bevy::prelude::Vec4::new(1.0, 0.0, 0.0, 0.0),
        };
        assert!(StarPipelineKey::from(&glow).additive);
    }
}
