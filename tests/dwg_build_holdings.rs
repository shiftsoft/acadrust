//! A DWG read keeps only what it needs while it builds the document: the
//! photometric lookup runs only for drawings with lights and keeps only the
//! LIGHTINGUNITS entry, a visiting read reserves no room for entities it may
//! drop, and the entity-to-block map covers only stored entities. None of
//! that may change what a caller reads.

use std::collections::BTreeMap;
use std::io::Cursor;

use acadrust::entities::light::LightPhotometricData;
use acadrust::entities::{EntityType, Light, Line};
use acadrust::objects::{Dictionary, ObjectType};
use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn photometric_light() -> Light {
    let mut light = Light::new();
    light.position = Vector3::new(1.0, 2.0, 3.0);
    light.photometric_mode = true;
    light.photometric_data = Some(LightPhotometricData {
        physical_intensity: 1500.0,
        lamp_color_temperature: 3200.0,
        ..LightPhotometricData::default()
    });
    light
}

fn read_light(bytes: Vec<u8>) -> Light {
    let document = DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .expect("read DWG");
    let light = document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Light(light) => Some((**light).clone()),
            _ => None,
        })
        .expect("light");
    light
}

/// A dictionary with many entries, none of them LIGHTINGUNITS, beside the
/// variable dictionary.
fn add_large_dictionary(document: &mut CadDocument) {
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = document.header.named_objects_dict_handle;
    for i in 0..2_000 {
        dictionary.add_entry(format!("ENTRY_{i:05}"), Handle::new(0x7000_0000 + i));
    }
    document
        .objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    let root = document.header.named_objects_dict_handle;
    if let Some(ObjectType::Dictionary(root)) = document.objects.get_mut(&root) {
        root.add_entry("LARGE", handle);
    }
}

#[test]
fn lighting_units_still_reach_a_photometric_light() {
    for version in [DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        assert!(document.set_drawing_variable("LIGHTINGUNITS", "2"));
        add_large_dictionary(&mut document);
        document
            .add_entity(EntityType::Light(Box::new(photometric_light())))
            .expect("light");
        let bytes = DwgWriter::write_to_vec(&document).expect("write DWG");
        let light = read_light(bytes);
        assert!(light.photometric_mode, "{version:?}");
        let data = light.photometric_data.expect("photometric data");
        assert_eq!(data.physical_intensity, 1500.0, "{version:?}");
        assert_eq!(data.lamp_color_temperature, 3200.0, "{version:?}");
        assert_eq!(light.position, Vector3::new(1.0, 2.0, 3.0));
    }
}

#[test]
fn a_light_without_lighting_units_has_no_photometric_tail() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    add_large_dictionary(&mut document);
    let mut light = photometric_light();
    light.photometric_mode = false;
    light.photometric_data = None;
    document
        .add_entity(EntityType::Light(Box::new(light)))
        .expect("light");
    let bytes = DwgWriter::write_to_vec(&document).expect("write DWG");
    let light = read_light(bytes);
    assert!(!light.photometric_mode);
    assert!(light.photometric_data.is_none());
}

/// Entities of each block record, in the record's order, with their owners
/// checked against it.
fn memberships(document: &CadDocument) -> BTreeMap<String, Vec<Handle>> {
    document
        .block_records
        .iter()
        .map(|block| {
            for handle in &block.entity_handles {
                let entity = document.get_entity(*handle).expect("member entity");
                assert_eq!(entity.common().owner_handle, block.handle, "{}", block.name);
            }
            (block.name.clone(), block.entity_handles.clone())
        })
        .collect()
}

fn layered_document(version: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    document.add_layout("Second").expect("layout");
    for (layout, handle) in [
        ("Second", 0x3000),
        ("Layout1", 0x5000),
        ("Second", 0x1000),
        ("Second", 0x4000),
        ("Layout1", 0x2000),
    ] {
        let mut line = Line::from_coords(handle as f64, 0.0, 0.0, handle as f64, 1.0, 0.0);
        line.common.handle = Handle::new(handle);
        document
            .add_entity_to_layout(EntityType::Line(line), layout)
            .expect("paper line");
    }
    for handle in [0x6000, 0x6001] {
        let mut line = Line::from_coords(0.0, handle as f64, 0.0, 1.0, handle as f64, 0.0);
        line.common.handle = Handle::new(handle);
        document
            .add_entity(EntityType::Line(line))
            .expect("model line");
    }
    document
}

#[test]
fn visiting_reads_keep_block_ownership_and_order_of_kept_entities() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let source = layered_document(version);
        let bytes = DwgWriter::write_to_vec(&source).expect("write DWG");
        let full = DwgReader::from_stream(Cursor::new(bytes.clone()))
            .read()
            .expect("read");
        let dropped = [Handle::new(0x1000), Handle::new(0x6000)];
        let mut expected = memberships(&full);
        for handles in expected.values_mut() {
            handles.retain(|handle| !dropped.contains(handle));
        }
        let visited = DwgReader::from_stream(Cursor::new(bytes))
            .read_visiting(|_, entity| {
                (!dropped.contains(&entity.common().handle)).then_some(entity)
            })
            .expect("read_visiting");
        for handle in dropped {
            assert!(visited.get_entity(handle).is_none(), "{version:?}");
        }
        assert_eq!(memberships(&visited), expected, "{version:?}");
    }
}
