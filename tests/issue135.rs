use std::io::Cursor;

use acadrust::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use acadrust::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use acadrust::io::dwg::dwg_stream_readers::object_reader::associative::read_associative_data;
use acadrust::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
use acadrust::io::dwg::dwg_version::DwgVersion;
use acadrust::objects::{
    AssocDependency, AssocGeomDependency, AssocPersistentSubentId, AssociativeData,
    AssociativeObject, ObjectType,
};
use acadrust::types::DxfVersion;
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn document_with_geom_dependency(version: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    let handle = document.allocate_handle();
    let owner = document.header.named_objects_dict_handle;
    document.objects.insert(
        handle,
        ObjectType::Associative(Box::new(AssociativeObject {
            handle,
            owner,
            dxf_name: "ASSOCGEOMDEPENDENCY".to_string(),
            cpp_class_name: "AcDbAssocGeomDependency".to_string(),
            data: AssociativeData::GeomDependency(AssocGeomDependency {
                dependency: AssocDependency {
                    name: Some("dependency".to_string()),
                    ..Default::default()
                },
                persistent_subent: AssocPersistentSubentId {
                    class_name: "AcDbAssocSingleEdgePersSubentId".to_string(),
                    dependent_on_compound_object: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        })),
    );
    document
}

fn read_geom_dependency(bytes: Vec<u8>) -> AssocGeomDependency {
    let document = DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .expect("DWG should read without a truncated persistent subentity id");
    document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Associative(object) => match &object.data {
                AssociativeData::GeomDependency(value) => Some(value.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("geometry dependency should remain typed")
}

#[test]
fn r2010_geom_dependency_uses_the_text_stream_for_its_class_name() {
    let source = document_with_geom_dependency(DxfVersion::AC1024);
    let expected = match source
        .objects
        .values()
        .find(|object| matches!(object, ObjectType::Associative(_)))
        .unwrap()
    {
        ObjectType::Associative(object) => match &object.data {
            AssociativeData::GeomDependency(value) => value.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };

    let actual = read_geom_dependency(DwgWriter::write_to_vec(&source).unwrap());
    assert_eq!(actual.class_version, expected.class_version);
    assert_eq!(actual.enabled, expected.enabled);
    assert_eq!(actual.dependency, expected.dependency);
    assert_eq!(
        actual.persistent_subent.class_name,
        expected.persistent_subent.class_name
    );
    assert_eq!(actual.persistent_subent.class_code, 1);
    assert!(actual.persistent_subent.values.is_empty());
    assert_eq!(
        actual.persistent_subent.dependent_on_compound_object,
        expected.persistent_subent.dependent_on_compound_object
    );
}

#[test]
fn r2013_compact_geom_dependency_layout_still_round_trips() {
    let source = document_with_geom_dependency(DxfVersion::AC1027);
    let actual = read_geom_dependency(DwgWriter::write_to_vec(&source).unwrap());
    assert_eq!(
        actual.persistent_subent.class_name,
        "AcDbAssocSingleEdgePersSubentId"
    );
    assert_eq!(actual.persistent_subent.class_code, 1);
    assert!(actual.persistent_subent.values.is_empty());
    assert!(actual.persistent_subent.dependent_on_compound_object);
}

#[test]
fn truncated_r2010_persistent_subent_keeps_the_typed_dependency() {
    let dxf_version = DxfVersion::AC1024;
    let dwg_version = DwgVersion::AC24;

    let mut main = DwgBitWriter::new(dwg_version, dxf_version);
    main.write_bit_short(0); // dependency class version
    main.write_bit_long(0); // status
    for _ in 0..4 {
        main.write_bit(false);
    }
    main.write_bit_long(0); // order
    main.write_bit(false); // no dependency name
    main.write_bit_long(0); // dependency body id
    main.write_bit_short(0); // geometry dependency class version
    main.write_bit(false); // disabled
    main.write_bit(true); // compound-object flag; class name is omitted

    let mut handles = DwgBitWriter::new(dwg_version, dxf_version);
    for _ in 0..4 {
        handles.write_handle_undefined(0);
    }

    let main = DwgBitReader::new(main.to_bytes(), dwg_version, dxf_version);
    let text = DwgBitReader::new(Vec::new(), dwg_version, dxf_version);
    let handles = DwgBitReader::new(handles.to_bytes(), dwg_version, dxf_version);
    let mut reader = DwgMergedReader::from_readers(main, Some(text), Some(handles), dxf_version);

    let value = read_associative_data(&mut reader, "ASSOCGEOMDEPENDENCY", dwg_version, dxf_version)
        .expect("ASSOCGEOMDEPENDENCY should remain typed when its optional tail is absent");

    let AssociativeData::GeomDependency(value) = value else {
        panic!("expected a geometry dependency");
    };
    assert_eq!(value.dependency.status, 0);
    assert!(value.persistent_subent.class_name.is_empty());
    assert!(value.persistent_subent.dependent_on_compound_object);
    assert!(reader.check_error().is_ok());
}
