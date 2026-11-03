use std::io::Cursor;

use acadrust::classes::DxfClass;
use acadrust::io::dwg::dwg_stream_readers::object_reader::associative::read_associative_data;
use acadrust::io::dwg::dwg_stream_readers::object_reader::DwgObjectReader;
use acadrust::io::dwg::dwg_stream_writers::object_writer::DwgObjectWriter;
use acadrust::objects::{
    Assoc2dConstraintGroup, AssocAction, AssociativeData, AssociativeObject, ClassObject,
    ClassObjectData, MentalRayRenderSettings, ObjectType, RenderSettings,
};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfVersion, DxfWriter, Handle};

fn document() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let handle = document.allocate_handle();
    document.objects.insert(
        handle,
        ObjectType::Associative(Box::new(AssociativeObject {
            handle,
            owner: document.header.named_objects_dict_handle,
            dxf_name: "ASSOC2DCONSTRAINTGROUP".into(),
            cpp_class_name: "AcDbAssoc2dConstraintGroup".into(),
            data: AssociativeData::ConstraintGroup(Assoc2dConstraintGroup {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                version: 2,
                ..Default::default()
            }),
            ..Default::default()
        })),
    );
    (document, handle)
}

#[test]
fn empty_r2013_constraint_group_writes_registry_tail() {
    let (document, handle) = document();
    let (bytes, records, _, _) = DwgObjectWriter::new(&document).unwrap().write();
    let offsets = records
        .into_iter()
        .map(|(h, offset)| (h, i64::from(offset)))
        .collect();
    let objects = DwgObjectReader::new(bytes, document.version, offsets).unwrap();
    let (type_code, mut reader) = objects
        .read_record_at(objects.offset_for(handle.value()).unwrap() as usize)
        .unwrap();
    objects.read_common_non_entity_data(&mut reader, type_code);
    let AssociativeData::ConstraintGroup(group) = read_associative_data(
        &mut reader,
        "ASSOC2DCONSTRAINTGROUP",
        objects.version(),
        document.version,
    )
    .unwrap() else {
        panic!("not a constraint group")
    };
    assert!(group.nodes.is_empty());
    assert_eq!(reader.main_remaining_bits(), 9);
    assert_eq!(reader.read_bit_long(), 0);
    assert_eq!(reader.read_bit_long(), 0);
    assert!(!reader.read_bit());
    assert_eq!(reader.read_bit_long(), 0);
    assert_eq!(reader.read_bit_long(), 0);
    assert_eq!(reader.main_remaining_bits(), 0);
    reader.check_error().unwrap();
}

#[test]
fn empty_constraint_group_round_trips_through_dwg() {
    let (document, handle) = document();
    let bytes = DwgWriter::write_to_vec(&document).unwrap();
    let loaded = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    let Some(ObjectType::Associative(object)) = loaded.objects.get(&handle) else {
        panic!("group disappeared")
    };
    let AssociativeData::ConstraintGroup(group) = &object.data else {
        panic!("wrong object type")
    };
    assert!(group.nodes.is_empty());
    assert_eq!(group.next_node_id, 0);
}

#[test]
fn empty_r2010_inline_group_preserves_its_sparse_id_seed() {
    let (mut document, handle) = document();
    document.version = DxfVersion::AC1024;
    let Some(ObjectType::Associative(object)) = document.objects.get_mut(&handle) else {
        unreachable!()
    };
    let AssociativeData::ConstraintGroup(group) = &mut object.data else {
        unreachable!()
    };
    group.next_node_id = 1_000_000;
    let bytes = DwgWriter::write_to_vec(&document).unwrap();
    let loaded = DwgReader::from_stream(Cursor::new(bytes))
        .read_with_stats()
        .unwrap();
    assert!(
        loaded.stats.diagnostics.is_empty(),
        "{:?}",
        loaded.stats.diagnostics
    );
    let Some(ObjectType::Associative(object)) = loaded.document.objects.get(&handle) else {
        panic!("group must remain typed")
    };
    let AssociativeData::ConstraintGroup(group) = &object.data else {
        unreachable!()
    };
    assert!(group.nodes.is_empty());
    assert_eq!(group.next_node_id, 1_000_000);
}

#[test]
fn mental_ray_render_settings_survive_dxf_to_dwg_without_stream_shift() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.classes.add_or_update(DxfClass::new(
        "MENTALRAYRENDERSETTINGS",
        "AcDbMentalRayRenderSettings",
    ));
    let handle = document.allocate_handle();
    let owner = document.header.named_objects_dict_handle;
    let value = MentalRayRenderSettings {
        base: RenderSettings {
            class_version: 4,
            name: "RaySettings".into(),
            fog_enabled: true,
            fog_background_enabled: false,
            backfaces_enabled: true,
            environment_image_enabled: true,
            environment_image_filename: "environment.hdr".into(),
            description: "base".into(),
            display_index: 7,
            // MENTALRAYRENDERSETTINGS has no predefined flag in this base
            // layout (the flag belongs only to plain RENDERSETTINGS).
            has_predefined: false,
        },
        version: 11,
        sampling_min: 2,
        sampling_max: 9,
        sampling_filter: 3,
        sampling_filter_width: 1.25,
        sampling_filter_height: 2.5,
        sampling_contrast: [0.1, 0.2, 0.3, 0.4],
        shadow_mode: 5,
        shadow_maps_enabled: true,
        ray_tracing_enabled: false,
        ray_trace_depth: [1, 2, 3],
        global_illumination_enabled: true,
        global_illumination_sample_count: 17,
        global_illumination_sample_radius_enabled: true,
        global_illumination_sample_radius: 42.0,
        photons_per_light: 23,
        photon_trace_depth: [4, 5, 6],
        final_gathering_enabled: true,
        final_gathering_ray_count: 31,
        final_gathering_sample_radius_state: [true, false, true],
        final_gathering_sample_radius: [3.5, 4.5],
        light_luminance_scale: 1.75,
        diagnostics_mode: 2,
        diagnostics_grid_mode: 3,
        diagnostics_grid_size: 5.5,
        diagnostics_photon_mode: 4,
        diagnostics_bsp_mode: 5,
        export_mi_enabled: true,
        description: "mental-ray".into(),
        tile_size: 64,
        tile_order: 2,
        memory_limit: 1024,
        diagnostics_samples_mode: true,
        energy_multiplier: 0.875,
    };
    document.objects.insert(
        handle,
        ObjectType::ClassObject(Box::new(ClassObject {
            handle,
            owner,
            data: ClassObjectData::MentalRayRenderSettings(value.clone()),
            ..Default::default()
        })),
    );

    let dxf = DxfWriter::new(&document).write_to_vec().unwrap();
    let through_dxf = DxfReader::from_reader(Cursor::new(dxf))
        .unwrap()
        .read()
        .unwrap();
    let dwg = DwgWriter::write_to_vec(&through_dxf).unwrap();
    let round_trip = DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
    let Some(ObjectType::ClassObject(object)) = round_trip.objects.get(&handle) else {
        panic!("mental-ray render settings disappeared")
    };
    let ClassObjectData::MentalRayRenderSettings(actual) = &object.data else {
        panic!("wrong class object after round trip")
    };
    assert_eq!(actual, &value);
}
