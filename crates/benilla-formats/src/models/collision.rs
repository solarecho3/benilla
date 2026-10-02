//! Collision geometry: the M2 collision hull and the WMO group gathers for walking and the camera.

use std::collections::HashMap;
use std::io::Cursor;

use anyhow::{Context, Result};
use benilla_m2::parse_m2;
use benilla_wmo::{parse_wmo, ParsedWmo};

use crate::Chain;

use super::model_path;

/// A collision triangle list in raw model-local coordinates, the space of
/// [`super::RenderSubmesh::positions`]; empty when the source has no collision geometry.
#[derive(Debug, Clone, Default)]
pub struct CollisionMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl CollisionMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Drop a canopy-volume hull so it does not block walking. Elwynn oaks (`ElwynnTreeCanopy01`)
    /// author a 14×16 yd (vanilla) / 32×42 yd (patch-D) triangle soup whose walk-height radius is
    /// ~7.5 yd, so empty grass around the trunk is solid; the trunk is a separate doodad
    /// (`ElwynnTreeMid01`, ~1.4 yd). Fences stay: they are long on one ground axis and thin on the
    /// other. Empty when this hull is a canopy volume.
    pub fn without_canopy_volume(self) -> Self {
        if canopy_volume(&self.positions) {
            Self::default()
        } else {
            self
        }
    }
}

/// Both ground-plane extents at or above this (yd) means a canopy volume, not a prop. Stumps sit
/// at ~4.5 yd; vanilla `ElwynnTreeCanopy01` at 14 yd.
const CANOPY_HULL_EXTENT: f32 = 8.0;

fn canopy_volume(positions: &[[f32; 3]]) -> bool {
    if positions.is_empty() {
        return false;
    }
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in positions {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    (max_x - min_x).abs() >= CANOPY_HULL_EXTENT && (max_y - min_y).abs() >= CANOPY_HULL_EXTENT
}

/// An M2 doodad's collision hull, the header's `bounding_vertices` (`C3Vector`) and
/// `bounding_triangles` (`u16` indices): a coarse solid far smaller than the render mesh, so the
/// trunk blocks and the canopy does not. A doodad with no hull does not collide. Canopy-volume
/// hulls are dropped, see [`CollisionMesh::without_canopy_volume`].
pub fn load_m2_collision_hull(chain: &mut Chain, raw_path: &str) -> Result<CollisionMesh> {
    let path = model_path(raw_path);
    let bytes = chain
        .read_file(&path)
        .with_context(|| format!("reading M2 {path}"))?;
    parse_m2_collision_hull(&bytes)
        .map(CollisionMesh::without_canopy_volume)
        .with_context(|| format!("parsing M2 {path}"))
}

/// [`load_m2_collision_hull`] from file bytes already in hand; empty when there is no hull.
pub fn parse_m2_collision_hull(bytes: &[u8]) -> Result<CollisionMesh> {
    let format =
        parse_m2(&mut Cursor::new(bytes)).map_err(|e| anyhow::anyhow!("parsing M2 hull: {e}"))?;
    let rd = &format.model().raw_data;
    let positions: Vec<[f32; 3]> = rd
        .bounding_vertices
        .as_chunks::<12>()
        .0
        .iter()
        .map(|c| {
            [
                f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                f32::from_le_bytes([c[8], c[9], c[10], c[11]]),
            ]
        })
        .collect();
    let n = positions.len() as u32;
    let indices: Vec<u32> = rd
        .bounding_triangles
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u32::from(u16::from_le_bytes([c[0], c[1]])))
        .collect();
    // Every 1.12 hull has whole triangles and in-range indices; anything else counts as no hull.
    if indices.is_empty() || !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i >= n) {
        return Ok(CollisionMesh::default());
    }
    Ok(CollisionMesh { positions, indices })
}

/// MOPY `DETAIL`, the face bit the walking gather skips. The reference builds a reject mask from a
/// per-query class word: walking, `0x10_0111` (`0x6315f0`), gives `0x84`, tested at the box-query
/// leaf `0x6bca50`; camera and line of sight, `0x10_0171` (`0x50e570`), give `0x82`, tested at
/// the segment-query leaf `0x6bc700`. The same code builds both masks, and `0x80` is the query's
/// transient visited bit, so walking skips `DETAIL` and the camera `NOCAMCOLLIDE`.
const MOPY_DETAIL: u8 = 0x04;
/// MOPY `NOCAMCOLLIDE`, the face bit the camera and line-of-sight gather skips.
const MOPY_NOCAMCOLLIDE: u8 = 0x02;

/// A WMO's walking-collidable triangles across every group file, in raw WMO-local coordinates: a
/// face collides unless it is `DETAIL` ([`MOPY_DETAIL`]). Wider than the vmangos vmap rule
/// `COLLISION || (RENDER && !DETAIL)`: faces with flags `0x00` or only `0x40` collide too. Every
/// group is gathered, with no whole-group skip by MOGP flags.
pub fn load_wmo_collision_tris(chain: &mut Chain, raw_path: &str) -> Result<CollisionMesh> {
    let root_path = raw_path.to_ascii_lowercase();
    let bytes = chain
        .read_file(&root_path)
        .with_context(|| format!("reading WMO {root_path}"))?;
    let ParsedWmo::Root(root) = parse_wmo(&mut Cursor::new(&bytes))
        .map_err(|e| anyhow::anyhow!("parsing {root_path}: {e}"))?
    else {
        anyhow::bail!("{root_path} is not a WMO root file");
    };
    let stem = root_path.strip_suffix(".wmo").unwrap_or(&root_path);
    let (mut positions, mut indices): (Vec<[f32; 3]>, Vec<u32>) = (Vec::new(), Vec::new());
    for gi in 0..root.n_groups {
        let group_path = format!("{stem}_{gi:03}.wmo");
        let Ok(gbytes) = chain.read_file(&group_path) else {
            continue;
        };
        accumulate_wmo_group_collision(&gbytes, &mut positions, &mut indices);
    }
    Ok(CollisionMesh { positions, indices })
}

/// Append one WMO group file's walking-collidable triangles: every face but `DETAIL`.
pub fn accumulate_wmo_group_collision(
    group_bytes: &[u8],
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    accumulate_wmo_group_faces(group_bytes, MOPY_DETAIL, 0, positions, indices);
}

/// Append one WMO group file's camera and line-of-sight triangles: every face but `NOCAMCOLLIDE`,
/// so a `DETAIL` overhang the player walks under (forge pipes) still stops the camera.
pub fn accumulate_wmo_group_camera_collision(
    group_bytes: &[u8],
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    accumulate_wmo_group_faces(group_bytes, MOPY_NOCAMCOLLIDE, 0, positions, indices);
}

/// Append one WMO group file's camera-only triangles (`DETAIL` set, `NOCAMCOLLIDE` clear): the
/// camera set minus the walking set, for the down-ray's fallback once the walking leg misses.
pub fn accumulate_wmo_group_camera_only_collision(
    group_bytes: &[u8],
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    accumulate_wmo_group_faces(
        group_bytes,
        MOPY_NOCAMCOLLIDE,
        MOPY_DETAIL,
        positions,
        indices,
    );
}

/// Append every triangle whose MOPY flags miss `skip_mask` and carry all of `require_mask`, its
/// vertices remapped per group (the same index in two groups is two vertices).
fn accumulate_wmo_group_faces(
    group_bytes: &[u8],
    skip_mask: u8,
    require_mask: u8,
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    let Ok(ParsedWmo::Group(group)) = parse_wmo(&mut Cursor::new(group_bytes)) else {
        return;
    };
    let n_tri = group.vertex_indices.len() / 3;
    let mut map: HashMap<u16, u32> = HashMap::new();
    for t in 0..n_tri {
        let Some(mopy) = group.material_info.get(t) else {
            continue;
        };
        if mopy.flags & skip_mask != 0 || mopy.flags & require_mask != require_mask {
            continue;
        }
        for k in 0..3 {
            let Some(&vidx) = group.vertex_indices.get(t * 3 + k) else {
                continue;
            };
            let Some(p) = group.vertex_positions.get(vidx as usize) else {
                continue;
            };
            let global = *map.entry(vidx).or_insert_with(|| {
                positions.push([p.x, p.y, p.z]);
                (positions.len() - 1) as u32
            });
            indices.push(global);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_hull(positions: Vec<[f32; 3]>) -> CollisionMesh {
        CollisionMesh {
            positions,
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    #[test]
    fn a_fat_ground_hull_is_a_canopy_volume() {
        let m = box_hull(vec![
            [-8.0, -8.0, 0.0],
            [8.0, -8.0, 0.0],
            [8.0, 8.0, 10.0],
            [-8.0, 8.0, 10.0],
        ]);
        assert!(m.without_canopy_volume().is_empty());
    }

    #[test]
    fn a_thin_fence_hull_still_collides() {
        let m = box_hull(vec![
            [-4.2, -0.14, -0.5],
            [0.1, -0.14, -0.5],
            [0.1, 0.13, 1.8],
            [-4.2, 0.13, 1.8],
        ]);
        assert_eq!(m.without_canopy_volume().triangle_count(), 2);
    }

    #[test]
    fn a_trunk_hull_still_collides() {
        let m = box_hull(vec![
            [-1.3, -1.3, 0.0],
            [1.3, -1.3, 0.0],
            [1.3, 1.3, 10.0],
            [-1.3, 1.3, 10.0],
        ]);
        assert_eq!(m.without_canopy_volume().triangle_count(), 2);
    }
}
