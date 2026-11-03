//! Serde retains typed references and provenance without exposing raw DWG storage.
#![cfg(feature = "serde")]

use acadrust::{
    entities::{EntityCommon, EntityType, Helix, Line, RawRecord, Spline},
    objects::{AssociativeObject, ObjectType},
    types::{DxfVersion, Handle},
};
use serde_json::{json, Value};
use std::sync::Arc;

const HIGH_HANDLE: u64 = 0x20_0000_0000_0001;
const COMMON_FIELDS: [&str; 11] = [
    "layer_handle",
    "linetype_handle",
    "color_book_handle",
    "face_visual_style_handle",
    "edge_visual_style_handle",
    "material_flags",
    "material_handle",
    "shadow_flags",
    "plotstyle_flags",
    "plotstyle_handle",
    "entity_mode",
];

fn common_with_retained_values(handle: u64) -> EntityCommon {
    let mut common = EntityCommon::new();
    let reference = Some(Handle::new(handle));
    common.layer_handle = reference;
    common.linetype_handle = reference;
    common.color_book_handle = reference;
    common.face_visual_style_handle = reference;
    common.edge_visual_style_handle = reference;
    common.material_flags = 3;
    common.material_handle = reference;
    common.shadow_flags = 2;
    common.plotstyle_flags = 3;
    common.plotstyle_handle = reference;
    common.entity_mode = Some(2);
    common
}

fn assert_retained_common(common: &EntityCommon, handle: u64) {
    let reference = Some(Handle::new(handle));
    assert_eq!(common.layer_handle, reference);
    assert_eq!(common.linetype_handle, reference);
    assert_eq!(common.color_book_handle, reference);
    assert_eq!(common.face_visual_style_handle, reference);
    assert_eq!(common.edge_visual_style_handle, reference);
    assert_eq!(common.material_flags, 3);
    assert_eq!(common.material_handle, reference);
    assert_eq!(common.shadow_flags, 2);
    assert_eq!(common.plotstyle_flags, 3);
    assert_eq!(common.plotstyle_handle, reference);
    assert_eq!(common.entity_mode, Some(2));
}

fn assert_missing_common_defaults(common: &EntityCommon) {
    assert_eq!(common.layer_handle, None);
    assert_eq!(common.linetype_handle, None);
    assert_eq!(common.color_book_handle, None);
    assert_eq!(common.face_visual_style_handle, None);
    assert_eq!(common.edge_visual_style_handle, None);
    assert_eq!(common.material_flags, 0);
    assert_eq!(common.material_handle, None);
    assert_eq!(common.shadow_flags, 0);
    assert_eq!(common.plotstyle_flags, 0);
    assert_eq!(common.plotstyle_handle, None);
    assert_eq!(common.entity_mode, None);
    // Unrelated fields from the old serialized input must also survive.
    assert_eq!(common.layer, "LegacyLayer");
    assert_eq!(common.full_visual_style_handle, Some(Handle::new(42)));
}

#[test]
fn serde_keeps_all_thirteen_understood_fields() {
    let common = common_with_retained_values(HIGH_HANDLE);
    let mut spline = Spline::new();
    spline.common = common.clone();
    spline.dwg_scenario = Some(2);
    let associative = AssociativeObject {
        source_version: Some(DxfVersion::AC1027),
        ..Default::default()
    };

    let common_json = serde_json::to_value(&common).unwrap();
    let spline_json = serde_json::to_value(&spline).unwrap();
    let associative_json = serde_json::to_value(&associative).unwrap();
    let mut missing: Vec<String> = COMMON_FIELDS
        .iter()
        .filter(|field| common_json.get(**field).is_none())
        .map(|field| format!("EntityCommon.{field}"))
        .collect();
    if spline_json.get("dwg_scenario").is_none() {
        missing.push("Spline.dwg_scenario".into());
    }
    if associative_json.get("source_version").is_none() {
        missing.push("AssociativeObject.source_version".into());
    }
    assert!(
        missing.is_empty(),
        "Serde omitted typed fields: {missing:?}"
    );
    for field in [
        "layer_handle",
        "linetype_handle",
        "color_book_handle",
        "face_visual_style_handle",
        "edge_visual_style_handle",
        "material_handle",
        "plotstyle_handle",
    ] {
        assert_eq!(common_json[field].as_u64(), Some(HIGH_HANDLE), "{field}");
    }
    assert_eq!(common_json["material_flags"], json!(3));
    assert_eq!(common_json["shadow_flags"], json!(2));
    assert_eq!(common_json["plotstyle_flags"], json!(3));
    assert_eq!(common_json["entity_mode"], json!(2));
    assert_eq!(spline_json["dwg_scenario"], json!(2));
    assert_eq!(associative_json["source_version"], json!("AC1027"));

    let restored: EntityCommon = serde_json::from_value(common_json).unwrap();
    assert_retained_common(&restored, HIGH_HANDLE);
    let restored: Spline = serde_json::from_value(spline_json).unwrap();
    assert_retained_common(&restored.common, HIGH_HANDLE);
    assert_eq!(restored.dwg_scenario, Some(2));
    let restored: AssociativeObject = serde_json::from_value(associative_json).unwrap();
    assert_eq!(restored.source_version, Some(DxfVersion::AC1027));
}

#[test]
fn serde_keeps_nested_helix_spline_references_and_provenance() {
    let mut helix = Helix::new();
    helix.common = common_with_retained_values(HIGH_HANDLE);
    helix.spline.common = common_with_retained_values(HIGH_HANDLE + 7);
    helix.spline.dwg_scenario = Some(2);
    let bytes = serde_json::to_vec(&helix).unwrap();
    let restored: Helix = serde_json::from_slice(&bytes).unwrap();
    assert_retained_common(&restored.common, HIGH_HANDLE);
    assert_retained_common(&restored.spline.common, HIGH_HANDLE + 7);
    assert_eq!(restored.spline.dwg_scenario, Some(2));
}

#[test]
fn serde_old_missing_fields_keep_documented_defaults() {
    // Frozen output from upstream before the thirteen fields participated in Serde.
    let legacy: Value =
        serde_json::from_str(include_str!("fixtures/serde/retained_fields_legacy.json")).unwrap();
    for common in [
        &legacy["common"],
        &legacy["spline"]["common"],
        &legacy["helix"]["common"],
        &legacy["helix"]["spline"]["common"],
    ] {
        for field in COMMON_FIELDS {
            assert!(
                common.get(field).is_none(),
                "legacy fixture contains {field}"
            );
        }
    }
    assert!(legacy["spline"].get("dwg_scenario").is_none());
    assert!(legacy["helix"]["spline"].get("dwg_scenario").is_none());
    assert!(legacy["associative"].get("source_version").is_none());

    let common: EntityCommon = serde_json::from_value(legacy["common"].clone()).unwrap();
    assert_missing_common_defaults(&common);
    let spline: Spline = serde_json::from_value(legacy["spline"].clone()).unwrap();
    assert_missing_common_defaults(&spline.common);
    assert_eq!(spline.dwg_scenario, None);
    let helix: Helix = serde_json::from_value(legacy["helix"].clone()).unwrap();
    assert_missing_common_defaults(&helix.common);
    assert_missing_common_defaults(&helix.spline.common);
    assert_eq!(helix.spline.dwg_scenario, None);
    let associative: AssociativeObject =
        serde_json::from_value(legacy["associative"].clone()).unwrap();
    assert_eq!(associative.source_version, None);
    assert_eq!(associative.dxf_name, "ASSOC");
}

#[test]
fn serde_keeps_inactive_and_reserved_common_values() {
    for flags in [0, 1, 2, 3, u8::MAX] {
        let mut common = common_with_retained_values(HIGH_HANDLE);
        common.material_flags = flags;
        common.plotstyle_flags = flags;
        common.shadow_flags = u8::MAX;
        common.entity_mode = Some(u8::MAX);
        let restored: EntityCommon =
            serde_json::from_slice(&serde_json::to_vec(&common).unwrap()).unwrap();
        assert_eq!(restored.material_flags, flags);
        assert_eq!(restored.plotstyle_flags, flags);
        assert_eq!(restored.shadow_flags, u8::MAX);
        assert_eq!(restored.entity_mode, Some(u8::MAX));
        assert_eq!(restored.material_handle, common.material_handle);
        assert_eq!(restored.plotstyle_handle, common.plotstyle_handle);
    }
}

#[test]
fn serde_still_excludes_raw_entity_storage() {
    let mut common = common_with_retained_values(HIGH_HANDLE);
    common.graphic_data = Some(vec![0xde, 0xad, 0xbe, 0xef]);
    common.raw_record = Some(Arc::new(RawRecord {
        data: vec![0xfa, 0xce],
        handle_bits: 8,
        version: DxfVersion::AC1032,
    }));
    common.has_ds_data = true;
    let mut helix = Helix::new();
    helix.common = common.clone();
    helix.spline.common = common;
    let value = serde_json::to_value(&helix).unwrap();
    for common in [&value["common"], &value["spline"]["common"]] {
        for field in ["graphic_data", "raw_record", "has_ds_data"] {
            assert!(common.get(field).is_none(), "raw storage leaked: {field}");
        }
    }
    let restored: Helix = serde_json::from_value(value).unwrap();
    for common in [&restored.common, &restored.spline.common] {
        assert_eq!(common.graphic_data, None);
        assert_eq!(common.raw_record, None);
        assert!(!common.has_ds_data);
    }
}

fn assert_entity_storage_restore_respects_edits(clear: bool) {
    let mut line = Line {
        common: common_with_retained_values(HIGH_HANDLE),
        ..Default::default()
    };
    line.common.handle = Handle::new(42);
    line.common.graphic_data = Some(vec![0xde, 0xad]);
    line.common.has_ds_data = true;
    line.common.raw_record = Some(Arc::new(RawRecord {
        data: vec![0xfa, 0xce],
        handle_bits: 8,
        version: DxfVersion::AC1032,
    }));
    let source = EntityType::Line(line);
    let mut edited_json = serde_json::to_value(&source).unwrap();
    for field in COMMON_FIELDS {
        let value = match field {
            "material_flags" | "shadow_flags" | "plotstyle_flags" => json!(u8::from(!clear)),
            _ if clear => Value::Null,
            "entity_mode" => json!(1),
            _ => json!(HIGH_HANDLE + 17),
        };
        assert_ne!(edited_json["Line"]["common"][field], value, "{field}");
        edited_json["Line"]["common"][field] = value;
    }
    let mut edited: EntityType = serde_json::from_value(edited_json.clone()).unwrap();
    assert_eq!(edited.common().handle, source.common().handle);
    assert_eq!(edited.common().graphic_data, None);
    assert!(!edited.common().has_ds_data);
    assert_eq!(edited.common().raw_record, None);

    edited.preserve_storage_data_from(&source);

    let restored_json = serde_json::to_value(&edited).unwrap();
    for field in COMMON_FIELDS {
        assert_eq!(
            restored_json["Line"]["common"][field], edited_json["Line"]["common"][field],
            "storage restoration overwrote the edit to {field}"
        );
    }
    assert_eq!(edited.common().graphic_data, Some(vec![0xde, 0xad]));
    assert!(edited.common().has_ds_data);
    assert_eq!(edited.common().raw_record, None);
    assert!(source.common().raw_record.is_some());
}

#[test]
fn storage_restore_keeps_serde_reference_and_flag_edits() {
    assert_entity_storage_restore_respects_edits(false);
}

#[test]
fn storage_restore_keeps_serde_explicit_null_and_zero_edits() {
    assert_entity_storage_restore_respects_edits(true);
}

#[test]
fn storage_restore_keeps_serde_associative_provenance_edits() {
    let source = ObjectType::Associative(Box::new(AssociativeObject {
        handle: Handle::new(42),
        source_version: Some(DxfVersion::AC1027),
        ..Default::default()
    }));
    for version in [json!("AC1032"), Value::Null] {
        let mut edited_json = serde_json::to_value(&source).unwrap();
        edited_json["Associative"]["source_version"] = version.clone();
        let mut edited: ObjectType = serde_json::from_value(edited_json).unwrap();
        assert_eq!(edited.intrinsic_handle(), source.intrinsic_handle());
        assert_eq!(
            serde_json::to_value(&edited).unwrap()["Associative"]["source_version"],
            version
        );

        edited.preserve_storage_data_from(&source);

        assert_eq!(
            serde_json::to_value(&edited).unwrap()["Associative"]["source_version"],
            version
        );
    }
}
