use acadrust::entities::solid3d::{
    AcisData, AcisMaterial, AcisRevision, Body, Region, Silhouette, Solid3D,
};
use acadrust::entities::surface::{Surface, SurfaceKind};
use acadrust::io::dwg::dwg_reference_type::DwgReferenceType;
use acadrust::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use acadrust::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use acadrust::io::dwg::dwg_stream_readers::object_reader::entities::read_acis_entity;
use acadrust::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
use acadrust::io::dwg::DwgVersion;
use acadrust::{
    CadDocument, DwgReader, DwgWriter, DxfVersion, EntityType, Handle, SabWriter, Vector3,
};
use std::io::Cursor;

// Encode the cache header independently of the entity writer. An absent
// isoline cache omits only the wire array, not the silhouette count.
#[test]
fn absent_isoline_cache_does_not_consume_the_modeler_tail() {
    for dxf_version in [
        DxfVersion::AC1012,
        DxfVersion::AC1014,
        DxfVersion::AC1015,
        DxfVersion::AC1018,
        DxfVersion::AC1021,
        DxfVersion::AC1024,
        DxfVersion::AC1027,
        DxfVersion::AC1032,
    ] {
        let version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        for offset in 0..8 {
            let mut writer = DwgBitWriter::new(version, dxf_version);
            for _ in 0..offset {
                writer.write_bit(true);
            }
            if !version.r2013_plus(dxf_version) {
                writer.write_bit(true); // no inline modeler body
            }
            writer.write_bit(true); // wireframe header
            writer.write_bit(false); // no point
            writer.write_bit_long(0); // isolines
            writer.write_bit(false); // no isoline cache: no wire count
            writer.write_bit_long(0); // silhouette count is still present
            writer.write_bit(true); // no extra modeler body
            if version.r2007_plus() {
                writer.write_bit_long(0);
            }
            if version.r2013_plus(dxf_version) {
                writer.write_bit(true);
                writer.write_bit_long(0x12345678);
                writer.write_bit_short(0x1234);
                writer.write_bit_short(0x5678);
                writer.write_bytes(&[1, 2, 3, 4, 5, 6, 7, 8]);
                writer.write_bit_long(0x23456789);
            }
            let end = writer.position_in_bits();
            let bytes = writer.into_bytes();
            for truncated in [false, true] {
                let main = DwgBitReader::new(bytes.clone(), version, dxf_version);
                let mut reader = DwgMergedReader::from_readers(main, None, None, dxf_version);
                reader.set_main_data_end(end - i64::from(truncated));
                reader.set_position_in_bits(offset);
                let data = read_acis_entity(
                    &mut reader,
                    version,
                    dxf_version,
                    version.r2013_plus(dxf_version),
                );
                if truncated {
                    assert!(reader.check_error().is_err(), "a truncated tail must fail");
                    continue;
                }
                reader
                    .check_error()
                    .unwrap_or_else(|error| panic!("{dxf_version:?}, offset {offset}: {error}"));
                assert_eq!(reader.position_in_bits(), end);
                assert!(data.wireframe_data_present);
                assert!(!data.wireframe_isoline_present);
                assert!(data.wires.is_empty());
                assert!(data.silhouettes.is_empty());
                assert!(data.acis_empty_bit);
                if version.r2013_plus(dxf_version) {
                    assert_eq!(
                        data.revision,
                        AcisRevision {
                            has_guid: true,
                            major: 0x12345678,
                            minor1: 0x1234,
                            minor2: 0x5678,
                            bytes: [1, 2, 3, 4, 5, 6, 7, 8],
                            end_marker: 0x23456789,
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn a_silhouette_only_cache_does_not_require_the_isoline_gate() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1021, DxfVersion::AC1024] {
        let mut document = CadDocument::with_version(version);
        let mut region = Region::new();
        region.acis_data = AcisData::from_sat("700 0 0 0\nbody $-1 -1 $-1 $-1 #\n");
        region.silhouettes.push(Silhouette {
            viewport_id: 123,
            target: Vector3::ZERO,
            view_direction: Vector3::UNIT_Z,
            up_vector: Vector3::UNIT_Y,
            is_perspective: false,
            has_wires: false,
            wires: Vec::new(),
        });
        let handle = document.add_entity(EntityType::Region(region)).unwrap();
        let bytes = DwgWriter::write_to_vec(&document).unwrap();
        let restored = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        let EntityType::Region(region) = restored.get_entity(handle).unwrap() else {
            panic!("missing REGION")
        };
        assert!(!region.acis_data.wireframe_isoline_present);
        assert!(region.wires.is_empty());
        assert_eq!(region.silhouettes.len(), 1);
        assert_eq!(region.silhouettes[0].viewport_id, 123);
    }
}

#[test]
fn legacy_sat_can_end_after_the_absent_wireframe_flag() {
    for dxf_version in [
        DxfVersion::AC1012,
        DxfVersion::AC1014,
        DxfVersion::AC1015,
        DxfVersion::AC1018,
    ] {
        let version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        let sat = "400 0 0 0\nbody $-1 $-1 $-1 $-1 #\n";
        let encrypted: Vec<_> = sat
            .bytes()
            .map(|b| if b <= 32 { b } else { 159 - b })
            .collect();
        let mut writer = DwgBitWriter::new(version, dxf_version);
        writer.write_bit(false);
        writer.write_bit(true);
        writer.write_bit_short(1);
        writer.write_bit_long(encrypted.len() as i32);
        writer.write_bytes(&encrypted);
        writer.write_bit_long(0);
        writer.write_bit(false); // last bit: no cache and no extra-body gate
        let end = writer.position_in_bits();
        let bytes = writer.into_bytes();
        for truncated in [false, true] {
            let main = DwgBitReader::new(bytes.clone(), version, dxf_version);
            let mut reader = DwgMergedReader::from_readers(main, None, None, dxf_version);
            reader.set_main_data_end(end - i64::from(truncated));
            let data = read_acis_entity(&mut reader, version, dxf_version, false);
            if truncated {
                assert!(reader.check_error().is_err());
            } else {
                reader.check_error().unwrap();
                assert_eq!(reader.position_in_bits(), end);
                assert_eq!(data.sat_data, sat);
                assert!(!data.wireframe_data_present);
                assert!(data.extra_acis_data.is_none());
            }
        }
    }
}

#[test]
fn acds_material_bindings_precede_the_revision_in_split_streams() {
    for dxf_version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        let mut main = DwgBitWriter::new(version, dxf_version);
        let mut handles = DwgBitWriter::new(version, dxf_version);
        main.write_bit(false); // no wireframe
        main.write_bit_long(1);
        main.write_bit_long(7);
        main.write_bit_long(9);
        handles.write_handle(DwgReferenceType::HardPointer, 0x1234);
        main.write_bit(true);
        main.write_bit_long(0x12345678);
        main.write_bit_short(0x1234);
        main.write_bit_short(0x5678);
        main.write_bytes(&[1, 2, 3, 4, 5, 6, 7, 8]);
        main.write_bit_long(0x23456789);
        let end = main.position_in_bits();
        let mut reader = DwgMergedReader::from_readers(
            DwgBitReader::new(main.into_bytes(), version, dxf_version),
            None,
            Some(DwgBitReader::new(
                handles.into_bytes(),
                version,
                dxf_version,
            )),
            dxf_version,
        );
        reader.set_main_data_end(end);
        let data = read_acis_entity(&mut reader, version, dxf_version, true);
        reader.check_error().unwrap();
        assert_eq!(reader.position_in_bits(), end);
        assert_eq!(
            data.materials,
            vec![AcisMaterial {
                array_index: 7,
                absolute_reference: 9,
                material_handle: Some(Handle::new(0x1234)),
            }]
        );
        assert_eq!(data.revision.major, 0x12345678);
        assert_eq!(data.revision.end_marker, 0x23456789);
    }
}

#[test]
fn silhouettes_without_isolines_preserve_materials_and_revision() {
    for dxf_version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        for offset in 0..8 {
            let mut main = DwgBitWriter::new(version, dxf_version);
            let mut handles = DwgBitWriter::new(version, dxf_version);
            for _ in 0..offset {
                main.write_bit(true);
            }
            main.write_bit(true); // wireframe
            main.write_bit(false); // no anchor point
            main.write_bit_long(0); // isolines
            main.write_bit(false); // no wire array
            main.write_bit_long(1); // independent silhouette array
            main.write_bit_long_long(0x123456789);
            main.write_3bit_double(Vector3::ZERO);
            main.write_3bit_double(Vector3::UNIT_Z);
            main.write_3bit_double(Vector3::UNIT_Y);
            main.write_bit(false); // orthographic
            main.write_bit(false); // no silhouette wires
            main.write_bit(true); // no extra modeler body
            main.write_bit_long(1); // material count
            main.write_bit_long(7);
            main.write_bit_long(9);
            handles.write_handle(DwgReferenceType::HardPointer, 0x1234);
            main.write_bit(true); // revision
            main.write_bit_long(0x12345678);
            main.write_bit_short(0x1234);
            main.write_bit_short(0x5678);
            main.write_bytes(&[1, 2, 3, 4, 5, 6, 7, 8]);
            main.write_bit_long(0x23456789);
            let end = main.position_in_bits();
            let mut reader = DwgMergedReader::from_readers(
                DwgBitReader::new(main.into_bytes(), version, dxf_version),
                None,
                Some(DwgBitReader::new(
                    handles.into_bytes(),
                    version,
                    dxf_version,
                )),
                dxf_version,
            );
            reader.set_main_data_end(end);
            reader.set_position_in_bits(offset);
            let data = read_acis_entity(&mut reader, version, dxf_version, true);
            reader.check_error().unwrap();
            assert_eq!(reader.position_in_bits(), end);
            assert!(!data.wireframe_isoline_present);
            assert!(data.wires.is_empty());
            assert_eq!(data.silhouettes.len(), 1);
            assert_eq!(data.silhouettes[0].viewport_id, 0x123456789);
            assert_eq!(data.silhouettes[0].view_direction, Vector3::UNIT_Z);
            assert_eq!(data.silhouettes[0].up_vector, Vector3::UNIT_Y);
            assert_eq!(data.materials.len(), 1);
            assert_eq!(data.materials[0].array_index, 7);
            assert_eq!(data.materials[0].absolute_reference, 9);
            assert_eq!(data.materials[0].material_handle, Some(Handle::new(0x1234)));
            assert_eq!(data.revision.major, 0x12345678);
            assert_eq!(data.revision.end_marker, 0x23456789);
        }
    }
}

#[test]
fn acds_material_bindings_survive_modeler_entity_roundtrips() {
    for version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let geometry = acadrust::entities::acis::primitives::build_box([0.0; 3], 2.0, 3.0, 4.0);
        let mut data = AcisData::from_sab(SabWriter::write(&geometry));
        let mut material = acadrust::objects::Material::new();
        material.handle = document.allocate_handle();
        material.name = "Test material".to_owned();
        data.materials.push(AcisMaterial {
            array_index: 7,
            absolute_reference: 9,
            material_handle: Some(material.handle),
        });
        document.objects.insert(
            material.handle,
            acadrust::objects::ObjectType::Material(Box::new(material)),
        );
        let mut solid = Solid3D::new();
        solid.acis_data = data.clone();
        let mut region = Region::new();
        region.acis_data = data.clone();
        let mut body = Body::new();
        body.acis_data = data.clone();
        let mut surface = Surface::new(SurfaceKind::Plane);
        surface.acis_data = data.clone();
        let handles: Vec<_> = [
            EntityType::Solid3D(solid),
            EntityType::Region(region),
            EntityType::Body(body),
            EntityType::Surface(Box::new(surface)),
        ]
        .into_iter()
        .map(|entity| document.add_entity(entity).unwrap())
        .collect();
        let bytes = DwgWriter::write_to_vec(&document).unwrap();
        let restored = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        for handle in handles {
            let actual = match restored.get_entity(handle).unwrap() {
                EntityType::Solid3D(entity) => &entity.acis_data,
                EntityType::Region(entity) => &entity.acis_data,
                EntityType::Body(entity) => &entity.acis_data,
                EntityType::Surface(entity) => &entity.acis_data,
                _ => unreachable!(),
            };
            assert_eq!(actual.materials, data.materials);
            assert!(actual.has_data());
        }
    }
}

// Bodies upgraded from AC1012-AC1024 keep their wireframe header without an
// isoline cache. The AcDs-backed writer must still emit the silhouette count
// that the reader expects in every wireframe block.
#[test]
fn acds_wireframe_without_isolines_keeps_the_silhouette_count() {
    for version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let geometry = acadrust::entities::acis::primitives::build_box([0.0; 3], 2.0, 3.0, 4.0);
        let mut data = AcisData::from_sab(SabWriter::write(&geometry));
        data.wireframe_data_present = true;
        data.wireframe_isoline_present = false;
        data.wireframe_isolines = 4;
        let mut solid = Solid3D::new();
        solid.acis_data = data.clone();
        let mut region = Region::new();
        region.acis_data = data.clone();
        let mut body = Body::new();
        body.acis_data = data.clone();
        let mut surface = Surface::new(SurfaceKind::Plane);
        surface.acis_data = data.clone();
        let handles: Vec<_> = [
            EntityType::Solid3D(solid),
            EntityType::Region(region),
            EntityType::Body(body),
            EntityType::Surface(Box::new(surface)),
        ]
        .into_iter()
        .map(|entity| document.add_entity(entity).unwrap())
        .collect();
        let bytes = DwgWriter::write_to_vec(&document).unwrap();
        let restored = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        for handle in handles {
            let actual = match restored.get_entity(handle) {
                Some(EntityType::Solid3D(entity)) => &entity.acis_data,
                Some(EntityType::Region(entity)) => &entity.acis_data,
                Some(EntityType::Body(entity)) => &entity.acis_data,
                Some(EntityType::Surface(entity)) => &entity.acis_data,
                other => panic!("{version:?}: modeler entity {handle:?} lost: {other:?}"),
            };
            assert!(actual.wireframe_data_present);
            assert!(!actual.wireframe_isoline_present);
            assert_eq!(actual.wireframe_isolines, 4);
            assert!(actual.has_data());
        }
    }
}
