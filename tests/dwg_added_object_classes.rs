//! Objects added to a drawing read from DWG keep their class on save.
//!
//! Switching a dynamic block reference creates an AcDbBlockRepresentationData
//! record. When the source file never used that class, the writer used to
//! emit the record under the placeholder type code 500, the reader resolved it
//! to another class or dropped it, and the reference lost its definition.

use acadrust::{
    objects::{BlockRepresentationData, DynamicBlockData, DynamicBlockObject, ObjectType},
    types::DxfVersion,
    CadDocument, DwgReader, DwgWriter, Handle,
};
use std::io::Cursor;

fn read(bytes: Vec<u8>) -> CadDocument {
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap()
}

#[test]
fn representation_data_added_after_reading_survives_dwg_save() {
    for version in [DxfVersion::AC1024, DxfVersion::AC1032] {
        // A drawing whose class table has no ACDB_BLOCKREPRESENTATION_DATA.
        let mut source = CadDocument::with_version(version);
        source.classes.retain_legacy_dwg_classes();
        let mut doc = read(DwgWriter::write_to_vec(&source).unwrap());
        assert!(
            !doc.classes.contains("ACDB_BLOCKREPRESENTATION_DATA"),
            "{version:?}: fixture must lack the class"
        );

        let handle = doc.allocate_handle();
        let block = doc.header.model_space_block_handle;
        let mut object = DynamicBlockObject::new(
            "ACDB_BLOCKREPRESENTATION_DATA",
            "AcDbBlockRepresentationData",
        );
        object.handle = handle;
        object.owner = doc.header.named_objects_dict_handle;
        object.data = DynamicBlockData::Representation(BlockRepresentationData { flags: 1, block });
        doc.objects
            .insert(handle, ObjectType::DynamicBlock(Box::new(object)));

        let back = read(DwgWriter::write_to_vec(&doc).unwrap());
        assert!(
            back.classes.contains("ACDB_BLOCKREPRESENTATION_DATA"),
            "{version:?}: class not declared"
        );
        match back.objects.get(&handle) {
            Some(ObjectType::DynamicBlock(object))
                if matches!(object.data, DynamicBlockData::Representation(_)) =>
            {
                let DynamicBlockData::Representation(data) = &object.data else {
                    unreachable!()
                };
                assert_eq!((data.flags, data.block), (1, block));
            }
            other => panic!("{version:?}: representation lost, read {other:?}"),
        }
        assert_ne!(handle, Handle::NULL);
    }
}
