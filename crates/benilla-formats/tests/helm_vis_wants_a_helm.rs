//! A `HelmetGeosetVisData` hide mask applies only when the head display is a worn helm, one that
//! names a model. Jubie Gadgetspring (display 7969, extra 5503) wears head display 15676, a
//! model-less amulet row with a full gnome mask; the reference shows her hair, ears and earrings.

use benilla_formats::{
    load_creature_catalog, load_item_display_catalog, open_chain, CharacterGeosets, EquipGeosets,
};

const JUBIE_DISPLAY: u32 = 7969;
const AMULET_DISPLAY: u32 = 15676;

#[test]
fn a_modelless_head_display_is_not_a_worn_helm() {
    let data = benilla_formats::wow_data_or_skip!();
    let mut chain = open_chain(&data).expect("open vanilla patch chain");
    let items = load_item_display_catalog(&mut chain).expect("ItemDisplayInfo");

    let amulet = items.get(AMULET_DISPLAY).expect("display 15676 exists");
    assert!(
        amulet.model.iter().all(Option::is_none),
        "display {AMULET_DISPLAY} names no model in either slot, got {:?}",
        amulet.model
    );
    assert_eq!(
        amulet.helmet_vis,
        [248, 306],
        "display {AMULET_DISPLAY} still carries a HelmetGeosetVisData pair"
    );
    assert_eq!(
        amulet.worn_helm_vis(),
        None,
        "a model-less display is not a worn helm"
    );

    let (_, helm) = items
        .iter()
        .find(|(_, d)| d.model[0].is_some() && d.helmet_vis != [0, 0])
        .expect("some display is a modelled helm with a vis row");
    assert_eq!(
        helm.worn_helm_vis(),
        Some(helm.helmet_vis),
        "a modelled helm keeps its vis pair"
    );

    // Winning HD ItemDisplayInfo (patch-G) carries these vis-row counts; 21 leave the left model empty.
    let masked = items.iter().filter(|(_, d)| d.helmet_vis != [0, 0]).count();
    let masked_modelless = items
        .iter()
        .filter(|(_, d)| d.helmet_vis != [0, 0] && d.model[0].is_none())
        .count();
    assert_eq!((masked, masked_modelless), (2639, 21));

    // The reference gates on `ModelName[0]` alone (`0x4799c1`); right-only rows carry no vis pair.
    let right_only = items
        .iter()
        .filter(|(_, d)| d.model[0].is_none() && d.model[1].is_some())
        .count();
    let right_only_masked = items
        .iter()
        .filter(|(_, d)| d.model[0].is_none() && d.model[1].is_some() && d.helmet_vis != [0, 0])
        .count();
    assert_eq!((right_only, right_only_masked), (74, 0));
}

#[test]
fn jubie_keeps_her_hair_ears_and_earrings() {
    let data = benilla_formats::wow_data_or_skip!();
    let mut chain = open_chain(&data).expect("open vanilla patch chain");
    let creatures = load_creature_catalog(&mut chain).expect("creature catalog");
    let items = load_item_display_catalog(&mut chain).expect("ItemDisplayInfo");
    let geosets = CharacterGeosets::load(&mut chain).expect("customization tables");

    let model = creatures
        .model(JUBIE_DISPLAY)
        .expect("display 7969 resolves a model");
    let npc = model
        .npc_appearance
        .as_ref()
        .expect("display 7969 has a CreatureDisplayInfoExtra row");
    assert_eq!(
        (npc.race, npc.sex, npc.hair_style, npc.facial_hair),
        (7, 1, 1, 2),
        "gnome female, hairstyle 1, facial-hair (earring) variation 2"
    );
    assert_eq!(
        npc.equipment[0], AMULET_DISPLAY,
        "her head column is the model-less amulet row"
    );

    let head = items.get(npc.equipment[0]).expect("head display row");
    let eg = EquipGeosets {
        helm_vis: head.worn_helm_vis(),
        ..EquipGeosets::default()
    };
    let set = geosets.visible_geosets(npc.race, npc.sex, npc.hair_style, npc.facial_hair, &eg);
    // CharHairGeosets (7,1,1) gives 3, CharacterFacialHairStyles (7,1,2) gives 203, ears keep 702.
    for want in [3u16, 203, 702] {
        assert!(set.contains(&want), "geoset {want} is on, got {set:?}");
    }
    for hidden in [1u16, 201, 701] {
        assert!(
            !set.contains(&hidden),
            "geoset {hidden} (the helm-tucked variant) is off, got {set:?}"
        );
    }

    // The mask, honoured: a bare scalp, no earrings and tucked ears.
    let broken = geosets.visible_geosets(
        npc.race,
        npc.sex,
        npc.hair_style,
        npc.facial_hair,
        &EquipGeosets {
            helm_vis: Some(head.helmet_vis),
            ..EquipGeosets::default()
        },
    );
    for want in [1u16, 201, 701] {
        assert!(broken.contains(&want), "the mask, honoured, forces {want}");
    }
}

/// Stormwind Royal Guard (display 3167) wears head display 14964. patch-G points that row at a
/// visor-only uprez; the catalog restores the vanilla plate cap so it covers the HD head.
#[test]
fn stormwind_guard_wears_the_plate_cap() {
    let data = benilla_formats::wow_data_or_skip!();
    let mut chain = open_chain(&data).expect("open vanilla patch chain");
    let creatures = load_creature_catalog(&mut chain).expect("creature catalog");
    let items = load_item_display_catalog(&mut chain).expect("ItemDisplayInfo");
    let geosets = CharacterGeosets::load(&mut chain).expect("customization tables");

    let model = creatures
        .model(3167)
        .expect("display 3167 resolves a model");
    let npc = model
        .npc_appearance
        .as_ref()
        .expect("display 3167 has a CreatureDisplayInfoExtra row");
    assert_eq!(
        npc.equipment[0], 14964,
        "Stormwind Royal Guard head column is display 14964"
    );

    let head = items.get(npc.equipment[0]).expect("head display row");
    assert_eq!(
        head.model[0].as_deref(),
        Some("helm_plate_b_01stormwind.m2"),
        "catalog restores the vanilla Stormwind plate cap"
    );
    assert_eq!(
        head.model_texture[0].as_deref(),
        Some("Helm_Plate_B_01Stormwind"),
        "cap keeps its Stormwind skin"
    );
    assert_eq!(
        head.worn_helm_vis(),
        Some([248, 306]),
        "cap still names a model so the hide-mask applies"
    );

    let helm_m2 = "Item\\ObjectComponents\\Head\\helm_plate_b_01stormwind_HuM.m2";
    assert!(
        chain.contains(helm_m2),
        "vanilla Stormwind cap mesh is on the chain ({helm_m2})"
    );
    let helm_blp = "Item\\ObjectComponents\\Head\\Helm_Plate_B_01Stormwind.blp";
    assert!(
        chain.contains(helm_blp),
        "vanilla Stormwind cap skin is on the chain ({helm_blp})"
    );

    let eg = EquipGeosets {
        helm_vis: head.worn_helm_vis(),
        ..EquipGeosets::default()
    };
    let set = geosets.visible_geosets(npc.race, npc.sex, npc.hair_style, npc.facial_hair, &eg);
    assert!(
        set.contains(&1),
        "helm must force the scalp (geoset 1); vis={:?} set={set:?}",
        head.helmet_vis
    );
    for id in 2u16..=16 {
        assert!(
            !set.contains(&id),
            "helm left hair geoset {id} on; vis={:?}",
            head.helmet_vis
        );
    }
}
