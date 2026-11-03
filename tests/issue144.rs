use std::io::Cursor;

use acadrust::objects::{BlockOrientedGrip, DynamicBlockData, DynamicBlockObject, ObjectType};
use acadrust::{CadDocument, DxfReader, DxfVersion, DxfWriter, Handle, Vector3};

const ORIENTATION: Vector3 = Vector3::new(1.25, -2.5, 3.75);
const SUBCLASS: &str = "AcDbBlockAlignmentGrip";

fn drawing() -> (CadDocument, Handle) {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut object = DynamicBlockObject::new("BLOCKALIGNMENTGRIP", SUBCLASS);
    object.handle = doc.allocate_handle();
    object.owner = doc.header.named_objects_dict_handle;
    object.data = DynamicBlockData::AlignmentGrip(BlockOrientedGrip {
        orientation: ORIENTATION,
        ..Default::default()
    });
    let handle = object.handle;
    doc.objects
        .insert(handle, ObjectType::DynamicBlock(Box::new(object)));
    (doc, handle)
}

fn orientation(doc: &CadDocument, handle: Handle) -> Vector3 {
    let ObjectType::DynamicBlock(object) = &doc.objects[&handle] else {
        panic!("alignment grip was not decoded");
    };
    let DynamicBlockData::AlignmentGrip(grip) = &object.data else {
        panic!("incorrect dynamic block object");
    };
    grip.orientation
}

fn read(bytes: Vec<u8>) -> CadDocument {
    DxfReader::from_reader(Cursor::new(bytes))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn alignment_orientation_uses_native_ascii_group_codes() {
    let (doc, handle) = drawing();
    let bytes = DxfWriter::new(&doc).write_to_vec().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let pairs: Vec<_> = lines
        .chunks_exact(2)
        .map(|pair| (pair[0].trim().parse::<i32>().unwrap(), pair[1].trim()))
        .collect();
    let start = pairs
        .iter()
        .position(|pair| *pair == (100, SUBCLASS))
        .unwrap()
        + 1;
    let fields: Vec<_> = pairs[start..start + 3]
        .iter()
        .map(|(code, value)| (*code, value.parse::<f64>().unwrap()))
        .collect();
    assert_eq!(fields, [(140, 1.25), (141, -2.5), (142, 3.75)]);
    assert_eq!(orientation(&read(bytes), handle), ORIENTATION);
}

#[test]
fn alignment_orientation_uses_three_binary_doubles() {
    let (doc, handle) = drawing();
    let bytes = DxfWriter::new_binary(&doc).write_to_vec().unwrap();
    let mut marker = 100i16.to_le_bytes().to_vec();
    marker.extend_from_slice(SUBCLASS.as_bytes());
    marker.push(0);
    let start = bytes
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len();
    let mut expected = Vec::new();
    for (code, value) in [(140i16, 1.25f64), (141, -2.5), (142, 3.75)] {
        expected.extend_from_slice(&code.to_le_bytes());
        expected.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(&bytes[start..start + expected.len()], expected);
    assert_eq!(
        &bytes[start + expected.len()..start + expected.len() + 2],
        [0, 0]
    );
    assert_eq!(orientation(&read(bytes), handle), ORIENTATION);
}

#[test]
fn alignment_reader_accepts_native_and_legacy_ascii_vectors() {
    for groups in [
        "140\n1.25\n141\n-2.5\n142\n3.75\n",
        "140\n1.25\n150\n-2.5\n160\n3.75\n",
        "140\n1.25\n150\n9\n160\n10\n141\n-2.5\n142\n3.75\n",
    ] {
        let text = format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n\
             0\nSECTION\n2\nOBJECTS\n0\nBLOCKALIGNMENTGRIP\n5\n100\n\
             100\nAcDbEvalExpr\n100\nAcDbBlockElement\n100\nAcDbBlockGrip\n\
             100\n{SUBCLASS}\n{groups}0\nENDSEC\n0\nEOF\n"
        );
        assert_eq!(
            orientation(&read(text.into_bytes()), Handle::new(0x100)),
            ORIENTATION
        );
    }
}
