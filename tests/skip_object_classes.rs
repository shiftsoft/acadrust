//! Reading can leave the records of chosen object classes out.
//!
//! Drawings converted from MicroStation can carry hundreds of thousands of
//! DGN line-style records that a viewer never uses. `skip_object_classes`
//! lets a reader leave such records undecoded while the rest of the drawing
//! reads as before.

use std::collections::BTreeMap;
use std::io::Cursor;

use acadrust::entities::{EntityType, Line, Wipeout};
use acadrust::io::dwg::dwg_reference_type::DwgReferenceType;
use acadrust::io::dwg::dwg_stream_writers::DwgMergedWriter;
use acadrust::io::dwg::DwgVersion;
use acadrust::io::read::ReadOutcome;
use acadrust::objects::{
    DgnLineStyleData, DgnLineStyleObject, DgnLsComponentData, DgnLsComponentType,
    DgnLsInternalComponent, DgnLsPhaseMode, DgnLsStroke, DgnLsStrokePattern, Dictionary,
    ObjectType,
};
use acadrust::tables::LineType;
use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::{
    CadDocument, DwgReadOptions, DwgReader, DwgWriter, DxfReader, DxfReaderConfiguration, DxfWriter,
};

const DGN_CLASSES: [&str; 3] = [
    "LSDEFINITION",
    "LSSTROKEPATTERNCOMPONENT",
    "LSINTERNALCOMPONENT",
];

fn pattern() -> DgnLsStrokePattern {
    DgnLsStrokePattern {
        has_iteration_limit: false,
        is_single_segment: false,
        iteration_limit: 0,
        auto_phase: 0.0,
        phase: 0.0,
        phase_mode: DgnLsPhaseMode::Distance,
        strokes: vec![DgnLsStroke {
            is_dash: true,
            bypass_corner: false,
            can_be_scaled: true,
            invert_at_origin: false,
            invert_at_end: false,
            length: 2.5,
            start_width: 0.0,
            end_width: 0.0,
            width_mode: 0,
            cap_mode: 0,
        }],
    }
}

fn component(kind: DgnLsComponentType, component: DgnLsComponentData) -> DgnLineStyleData {
    DgnLineStyleData::Component {
        kind,
        description: String::new(),
        version: 1,
        component_uid: [7; 16],
        scale: 1.0,
        property_flags: 0,
        component,
        properties: Vec::new(),
    }
}

/// A drawing laid out as AutoCAD stores an imported DGN line style: the
/// components sit in a dictionary under the named object dictionary, the
/// definition in the linetype's extension dictionary under `DGNLSDEF`.
struct Fixture {
    document: CadDocument,
    line: Handle,
    linetype: Handle,
    components: Handle,
    definition: Handle,
    stroke: Handle,
    internal: Handle,
}

fn fixture(version: DxfVersion) -> Fixture {
    let mut document = CadDocument::with_version(version);
    let line = document
        .add_entity(EntityType::Line(Line::from_coords(
            0.0, 0.0, 0.0, 10.0, 5.0, 0.0,
        )))
        .unwrap();
    let mut linetype = LineType::new("DGN_DASH");
    linetype.handle = document.allocate_handle();
    let linetype_handle = linetype.handle;
    document.line_types.add(linetype).unwrap();
    let linetype = linetype_handle;

    let root = document.header.named_objects_dict_handle;
    let components = document.allocate_handle();
    let stroke = document.allocate_handle();
    let internal = document.allocate_handle();
    let definition = document.allocate_handle();
    let mut store = Dictionary::new();
    store.handle = components;
    store.owner = root;
    store.hard_owner = true;
    store.add_entry("{STROKE}", stroke);
    store.add_entry("{INTERNAL}", internal);
    document
        .objects
        .insert(components, ObjectType::Dictionary(store));
    if let Some(ObjectType::Dictionary(root)) = document.objects.get_mut(&root) {
        root.add_entry("ACAD_DGNLINESTYLECOMP", components);
    }

    let objects = [
        (
            stroke,
            components,
            component(
                DgnLsComponentType::Stroke,
                DgnLsComponentData::Stroke(pattern()),
            ),
        ),
        (
            internal,
            components,
            component(
                DgnLsComponentType::Internal,
                DgnLsComponentData::Internal(DgnLsInternalComponent {
                    pattern: pattern(),
                    internal_version: 1,
                    hardware_style: 0,
                    is_hardware_style: false,
                    line_code: 0,
                }),
            ),
        ),
    ];
    for (handle, owner, data) in objects {
        document.objects.insert(
            handle,
            ObjectType::DgnLineStyle(DgnLineStyleObject {
                handle,
                owner,
                reactors: vec![owner],
                xdictionary_handle: None,
                data,
            }),
        );
    }
    let extension = document.ensure_extension_dictionary(linetype);
    if let Some(ObjectType::Dictionary(extension)) = document.objects.get_mut(&extension) {
        extension.add_entry("DGNLSDEF", definition);
    }
    document.objects.insert(
        definition,
        ObjectType::DgnLineStyle(DgnLineStyleObject {
            handle: definition,
            owner: extension,
            reactors: vec![extension],
            xdictionary_handle: None,
            data: DgnLineStyleData::Definition {
                description: "DGN_DASH".to_string(),
                version: 1,
                style_number: 0,
                component_uid: [7; 16],
                is_continuous: false,
                unit_definition: 1.0,
                unit_scale: 1.0,
                units_type: 0,
                is_element: false,
                is_physical: false,
                undecoded_fields: None,
                is_scale_independent: false,
                is_snappable: false,
                root_component: stroke,
                properties: Vec::new(),
            },
        }),
    );
    Fixture {
        document,
        line,
        linetype,
        components,
        definition,
        stroke,
        internal,
    }
}

fn read_dwg(bytes: &[u8], options: DwgReadOptions) -> ReadOutcome {
    DwgReader::from_stream_with_options(Cursor::new(bytes.to_vec()), options)
        .read_with_stats()
        .unwrap()
}

fn read_dxf(bytes: &[u8], skip: &[&str]) -> ReadOutcome {
    let configuration = DxfReaderConfiguration {
        skip_object_classes: skip.iter().map(|name| name.to_string()).collect(),
        ..DxfReaderConfiguration::default()
    };
    DxfReader::from_reader(Cursor::new(bytes.to_vec()))
        .unwrap()
        .with_configuration(configuration)
        .read_with_stats()
        .unwrap()
}

fn dgn_records(document: &CadDocument) -> usize {
    document
        .objects
        .values()
        .filter(|object| matches!(object, ObjectType::DgnLineStyle(_)))
        .count()
}

fn entries(document: &CadDocument, dictionary: Handle) -> Vec<(String, Handle)> {
    match document.objects.get(&dictionary) {
        Some(ObjectType::Dictionary(dictionary)) => dictionary.entries.clone(),
        other => panic!("dictionary {dictionary:?} missing: {other:?}"),
    }
}

fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
    pairs
        .iter()
        .map(|(name, count)| (name.to_string(), *count))
        .collect()
}

#[test]
fn skipped_dwg_classes_are_neither_decoded_nor_kept() {
    for version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let fixture = fixture(version);
        let bytes = DwgWriter::write_to_vec(&fixture.document).unwrap();
        let full = read_dwg(&bytes, DwgReadOptions::default());
        assert_eq!(dgn_records(&full.document), 3, "{version:?}: fixture");
        assert!(full.stats.skipped_object_classes.is_empty());

        for failsafe in [false, true] {
            let options = DwgReadOptions {
                failsafe,
                // Case does not matter.
                ..DwgReadOptions::default()
                    .with_skipped_object_classes(DGN_CLASSES.iter().map(|name| name.to_lowercase()))
            };
            let read = read_dwg(&bytes, options);
            let document = &read.document;
            let context = format!("{version:?} failsafe={failsafe}");
            assert_eq!(dgn_records(document), 0, "{context}");
            assert!(document.dgn_ls_definitions.is_empty(), "{context}");
            assert!(document.dgn_ls_components.is_empty(), "{context}");
            assert_eq!(
                read.stats.skipped_object_classes,
                counts(&[
                    ("LSDEFINITION", 1),
                    ("LSINTERNALCOMPONENT", 1),
                    ("LSSTROKEPATTERNCOMPONENT", 1),
                ]),
                "{context}"
            );
            // Leaving a record out is not a read failure.
            assert_eq!(read.stats.skipped_source_records, 0, "{context}");
            assert_eq!(
                read.stats.decoded_source_records + 3,
                full.stats.decoded_source_records,
                "{context}"
            );
            assert_eq!(read.stats.source_records, full.stats.source_records);
            assert_eq!(document.notifications.error_count(), 0, "{context}");
            // Everything else reads as before, and the references to the
            // skipped records are kept as they are.
            assert_eq!(
                document.objects.len() + 3,
                full.document.objects.len(),
                "{context}"
            );
            assert!(document.get_entity(fixture.line).is_some(), "{context}");
            assert!(document.line_types.get("DGN_DASH").is_some(), "{context}");
            assert_eq!(
                entries(document, fixture.components),
                entries(&full.document, fixture.components),
                "{context}"
            );
            let extension = document
                .extension_dictionary_handle(fixture.linetype)
                .expect("linetype extension dictionary");
            assert_eq!(
                entries(document, extension),
                vec![("DGNLSDEF".to_string(), fixture.definition)],
                "{context}"
            );
            for handle in [fixture.definition, fixture.stroke, fixture.internal] {
                assert!(!document.objects.contains_key(&handle), "{context}");
            }
        }
    }
}

#[test]
fn a_document_read_without_skipped_classes_saves_and_reads_again() {
    let fixture = fixture(DxfVersion::AC1032);
    let bytes = DwgWriter::write_to_vec(&fixture.document).unwrap();
    let read = read_dwg(
        &bytes,
        DwgReadOptions::default().with_skipped_object_classes(DGN_CLASSES),
    );
    let saved = DwgWriter::write_to_vec(&read.document).unwrap();
    let back = read_dwg(&saved, DwgReadOptions::default());
    assert_eq!(dgn_records(&back.document), 0);
    assert!(back.document.get_entity(fixture.line).is_some());
    assert_eq!(back.stats.skipped_source_records, 0);
    for binary in [false, true] {
        let mut dxf = Vec::new();
        let writer = if binary {
            DxfWriter::new_binary(&read.document)
        } else {
            DxfWriter::new(&read.document)
        };
        writer.write_to_writer(&mut dxf).unwrap();
        let back = read_dxf(&dxf, &[]);
        assert_eq!(dgn_records(&back.document), 0);
        assert!(back.document.get_entity(fixture.line).is_some());
    }
}

#[test]
fn only_object_classes_can_be_skipped() {
    let mut fixture = fixture(DxfVersion::AC1032);
    // WIPEOUT is an entity with a class of its own.
    let wipeout = fixture
        .document
        .add_entity(EntityType::Wipeout(Wipeout::from_corners(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 3.0, 0.0),
        )))
        .unwrap();
    let bytes = DwgWriter::write_to_vec(&fixture.document).unwrap();
    let full = read_dwg(&bytes, DwgReadOptions::default());
    assert!(full.document.classes.contains("WIPEOUT"));
    assert!(full.document.get_entity(wipeout).is_some());
    // Fixed object types, table records and entities are never skipped, nor
    // are names that are not classes of the drawing.
    let read = read_dwg(
        &bytes,
        DwgReadOptions::default().with_skipped_object_classes([
            "DICTIONARY",
            "LTYPE",
            "LINE",
            "WIPEOUT",
            "NO_SUCH_CLASS",
        ]),
    );
    assert!(read.stats.skipped_object_classes.is_empty());
    assert_eq!(read.document.objects.len(), full.document.objects.len());
    assert_eq!(
        read.document.entities().count(),
        full.document.entities().count()
    );
    assert_eq!(dgn_records(&read.document), 3);
    assert!(read.document.get_entity(wipeout).is_some());
}

/// A record of a skipped class is not decoded, so a payload the decoder
/// rejects leaves no diagnostic behind.
#[test]
fn a_skipped_record_is_not_decoded() {
    let version = DxfVersion::AC1032;
    let fixture = fixture(version);
    // The writer numbers the classes; take the drawing as read back.
    let bytes = DwgWriter::write_to_vec(&fixture.document).unwrap();
    let mut document = read_dwg(&bytes, DwgReadOptions::default()).document;
    let class_number = document
        .classes
        .iter()
        .find(|class| class.dxf_name == "LSSTROKEPATTERNCOMPONENT")
        .map(|class| class.class_number)
        .expect("class declared");
    let broken = document.allocate_handle();
    let mut writer = DwgMergedWriter::new(DwgVersion::from_dxf_version(version).unwrap(), version);
    writer.write_object_type(class_number);
    writer.main_mut().write_handle_undefined(broken.value());
    writer.write_bit_short(0); // No EED.
    writer.write_bit_long(0); // No reactors.
    writer.write_handle(DwgReferenceType::SoftPointer, fixture.components.value());
    writer.write_bit(true); // No extension dictionary.
    writer.write_bit(false); // No data store record.
                             // The component body is missing: its description and counts run off the
                             // end of the record.
    let raw = writer.merge();
    let handle_bits = raw.len() as i64 * 8 - writer.handle_start_bits();
    document.objects.insert(
        broken,
        ObjectType::Unknown {
            type_name: "LSSTROKEPATTERNCOMPONENT".to_string(),
            handle: broken,
            owner: fixture.components,
            raw_dxf_codes: None,
            raw_dwg_data: Some(raw),
            raw_dwg_handle_bits: handle_bits,
            raw_dwg_version: Some(version),
        },
    );
    document.dwg_source_version = Some(version);
    let bytes = DwgWriter::write_to_vec(&document).unwrap();

    let full =
        DwgReader::from_stream_with_options(Cursor::new(bytes.clone()), DwgReadOptions::failsafe())
            .read_with_stats()
            .unwrap();
    assert!(
        full.stats
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.record_handle == Some(broken.value())),
        "the fixture record must fail to decode"
    );

    for failsafe in [false, true] {
        let options = DwgReadOptions {
            failsafe,
            ..DwgReadOptions::default().with_skipped_object_classes(DGN_CLASSES)
        };
        let read = read_dwg(&bytes, options);
        assert!(read.stats.diagnostics.is_empty(), "failsafe={failsafe}");
        assert_eq!(read.document.notifications.error_count(), 0);
        assert!(!read.document.objects.contains_key(&broken));
        assert_eq!(
            read.stats.skipped_object_classes["LSSTROKEPATTERNCOMPONENT"], 2,
            "failsafe={failsafe}"
        );
    }
}

#[test]
fn skipped_dxf_classes_are_neither_decoded_nor_kept() {
    let fixture = fixture(DxfVersion::AC1032);
    for binary in [false, true] {
        let mut bytes = Vec::new();
        let writer = if binary {
            DxfWriter::new_binary(&fixture.document)
        } else {
            DxfWriter::new(&fixture.document)
        };
        writer.write_to_writer(&mut bytes).unwrap();
        let full = read_dxf(&bytes, &[]);
        assert_eq!(dgn_records(&full.document), 3, "binary={binary}: fixture");
        assert!(full.stats.skipped_object_classes.is_empty());

        let read = read_dxf(&bytes, &DGN_CLASSES);
        let document = &read.document;
        assert_eq!(dgn_records(document), 0, "binary={binary}");
        assert_eq!(
            read.stats.skipped_object_classes,
            counts(&[
                ("LSDEFINITION", 1),
                ("LSINTERNALCOMPONENT", 1),
                ("LSSTROKEPATTERNCOMPONENT", 1),
            ]),
            "binary={binary}"
        );
        assert_eq!(read.stats.skipped_source_records, 0);
        assert_eq!(read.stats.source_records, full.stats.source_records);
        assert_eq!(document.objects.len() + 3, full.document.objects.len());
        assert!(document.get_entity(fixture.line).is_some());
        assert_eq!(
            entries(document, fixture.components),
            entries(&full.document, fixture.components)
        );

        // Entity names are not object classes.
        let read = read_dxf(&bytes, &["LINE", "DICTIONARY"]);
        assert!(read.stats.skipped_object_classes.is_empty());
        assert_eq!(read.document.objects.len(), full.document.objects.len());
        assert!(read.document.get_entity(fixture.line).is_some());
    }
}
