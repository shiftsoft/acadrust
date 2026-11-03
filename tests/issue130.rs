//! Regression coverage for issue #130: table merges and row suppression
//! must survive conversion between modern DWG and both DXF encodings.

use std::io::Cursor;

use acadrust::entities::{CellRange, EntityType, Table};
use acadrust::objects::{ObjectType, TableStyle, TableStyleFlags};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfVersion, DxfWriter, Vector3};

const MODERN_VERSIONS: [DxfVersion; 3] =
    [DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032];

fn dwg_roundtrip(document: &CadDocument) -> CadDocument {
    DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(document).unwrap()))
        .read()
        .unwrap()
}

fn dxf_roundtrip(document: &CadDocument, binary: bool) -> CadDocument {
    let writer = if binary {
        DxfWriter::new_binary(document)
    } else {
        DxfWriter::new(document)
    };
    DxfReader::from_reader(Cursor::new(writer.write_to_vec().unwrap()))
        .unwrap()
        .read()
        .unwrap()
}

fn table(document: &CadDocument) -> &Table {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Table(table) => Some(&**table),
            _ => None,
        })
        .unwrap()
}

fn style(document: &CadDocument) -> &TableStyle {
    document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::TableStyle(style) if style.name == "Standard" => Some(style),
            _ => None,
        })
        .unwrap()
}

fn style_mut(document: &mut CadDocument) -> &mut TableStyle {
    document
        .objects
        .values_mut()
        .find_map(|object| match object {
            ObjectType::TableStyle(style) if style.name == "Standard" => Some(style),
            _ => None,
        })
        .unwrap()
}

fn merged_ranges() -> Vec<CellRange> {
    vec![
        CellRange::new(0, 0, 0, 3),
        CellRange::new(1, 0, 3, 0),
        CellRange::new(2, 2, 4, 4),
    ]
}

fn merged_document(version: DxfVersion, ranges_only: bool) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    let mut table = Table::new(Vector3::ZERO, 5, 5);
    if ranges_only {
        table.merged_ranges = merged_ranges();
    } else {
        for range in merged_ranges() {
            table.merge_cells(range);
        }
    }
    document
        .add_entity(EntityType::Table(Box::new(table)))
        .unwrap();
    document
}

fn assert_merges(document: &CadDocument) {
    assert_table_merges(table(document));
}

fn assert_table_merges(table: &Table) {
    assert_eq!(table.merged_ranges, merged_ranges());
    for range in merged_ranges() {
        let cell = &table.rows[range.top_row].cells[range.left_col];
        assert_eq!(cell.merge_width, range.col_count() as i32);
        assert_eq!(cell.merge_height, range.row_count() as i32);
        for row in range.top_row..=range.bottom_row {
            for column in range.left_col..=range.right_col {
                let anchor = row == range.top_row && column == range.left_col;
                assert_eq!(
                    table.rows[row].cells[column].merged,
                    if anchor { 0 } else { 1 }
                );
            }
        }
    }
    let unmerged = &table.rows[1].cells[1];
    assert_eq!((unmerged.merge_width, unmerged.merge_height), (1, 1));
    assert_eq!(unmerged.merged, 0);
}

#[test]
fn modern_dwg_merges_survive_repeated_dxf_conversion() {
    for version in MODERN_VERSIONS {
        let loaded = dwg_roundtrip(&merged_document(version, false));
        assert_merges(&loaded);
        for binary in [false, true] {
            let mut converted = loaded.clone();
            for _ in 0..2 {
                converted = dxf_roundtrip(&converted, binary);
                assert_merges(&converted);
                converted = dwg_roundtrip(&converted);
                assert_merges(&converted);
            }
            for legacy_version in [DxfVersion::AC1018, DxfVersion::AC1021] {
                converted.version = legacy_version;
                assert_merges(&dwg_roundtrip(&converted));
            }
        }
    }
}

#[test]
fn range_only_tables_write_dxf_cell_spans() {
    for version in MODERN_VERSIONS {
        for binary in [false, true] {
            assert_merges(&dxf_roundtrip(&merged_document(version, true), binary));
        }
    }
}

fn assert_suppression(document: &CadDocument, title: bool, header: bool) {
    let style = style(document);
    assert_eq!(style.title_suppressed, title);
    assert_eq!(style.header_suppressed, header);
    assert_eq!(
        style.flags.contains(TableStyleFlags::TITLE_SUPPRESSED),
        title
    );
    assert_eq!(
        style.flags.contains(TableStyleFlags::HEADER_SUPPRESSED),
        header
    );
    assert_eq!(style.has_title_row(), !title);
    assert_eq!(style.has_header_row(), !header);
    assert_eq!(style.flags.bits() & 0x10, 0x10, "unrelated flags survive");
    if let Some(raw) = &style.raw_dxf_codes {
        for (code, suppressed) in [(280, title), (281, header)] {
            let value = raw
                .iter()
                .skip_while(|(code, _)| *code != 3)
                .find(|(candidate, _)| *candidate == code)
                .map(|(_, value)| value.trim());
            assert_eq!(value, Some(if suppressed { "1" } else { "0" }));
        }
    }
}

#[test]
fn title_and_header_suppression_survive_both_conversion_directions() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1021]
        .into_iter()
        .chain(MODERN_VERSIONS)
    {
        for title in [false, true] {
            for header in [false, true] {
                for representation in 0..3 {
                    let mut document = CadDocument::with_version(version);
                    let style = style_mut(&mut document);
                    style.flags = TableStyleFlags::from_bits_retain(0x10);
                    match representation {
                        0 => {
                            style.title_suppressed = title;
                            style.header_suppressed = header;
                        }
                        1 => {
                            style.flags.set(TableStyleFlags::TITLE_SUPPRESSED, title);
                            style.flags.set(TableStyleFlags::HEADER_SUPPRESSED, header);
                        }
                        _ => {
                            style.set_title_visible(!title);
                            style.set_header_visible(!header);
                        }
                    }
                    let dwg = dwg_roundtrip(&document);
                    assert_suppression(&dwg, title, header);
                    for binary in [false, true] {
                        let dxf = dxf_roundtrip(&document, binary);
                        assert_suppression(&dxf, title, header);
                        assert_suppression(&dwg_roundtrip(&dxf), title, header);
                        assert_suppression(&dxf_roundtrip(&dwg, binary), title, header);
                    }
                }
            }
        }
    }
}

#[test]
fn visibility_setters_clear_flags_and_survive_resaving_loaded_styles() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1021]
        .into_iter()
        .chain(MODERN_VERSIONS)
    {
        let mut document = CadDocument::with_version(version);
        style_mut(&mut document).flags = TableStyleFlags::from_bits_retain(0x13);
        for binary in [false, true] {
            let mut loaded = dxf_roundtrip(&dwg_roundtrip(&document), binary);
            let table_style = style_mut(&mut loaded);
            table_style
                .raw_dxf_codes
                .as_mut()
                .unwrap()
                .push((300, "Preserved style metadata".to_string()));
            table_style.set_title_visible(true);
            table_style.set_header_visible(true);
            assert!(table_style.has_title_row());
            assert!(table_style.has_header_row());
            assert_eq!(table_style.flags.bits(), 0x10);
            let resaved = dxf_roundtrip(&loaded, binary);
            assert_suppression(&resaved, false, false);
            assert!(style(&resaved)
                .raw_dxf_codes
                .as_ref()
                .unwrap()
                .contains(&(300, "Preserved style metadata".to_string())));
            assert_suppression(&dwg_roundtrip(&loaded), false, false);
        }
    }
}

#[test]
fn merges_read_from_dwg_can_be_removed_using_a_covered_cell() {
    for version in MODERN_VERSIONS {
        let mut document = dwg_roundtrip(&merged_document(version, false));
        let handle = table(&document).common.handle;
        let Some(EntityType::Table(loaded_table)) = document.get_entity_mut(handle) else {
            panic!("Table missing after DWG read");
        };
        for (row, column) in [(0, 2), (2, 0), (4, 4)] {
            loaded_table.unmerge_cell(row, column);
        }
        assert!(loaded_table.merged_ranges.is_empty());
        for binary in [false, true] {
            let resaved = dwg_roundtrip(&dxf_roundtrip(&document, binary));
            let table = table(&resaved);
            assert!(table.merged_ranges.is_empty());
            assert!(table.rows.iter().flat_map(|row| &row.cells).all(|cell| {
                cell.merge_width == 1 && cell.merge_height == 1 && cell.merged == 0
            }));
        }
    }
}

#[test]
fn table_content_objects_project_merged_ranges_on_cells_when_loaded() {
    for version in MODERN_VERSIONS {
        let mut document = CadDocument::with_version(version);
        let mut content = table(&merged_document(version, true)).clone();
        content.common.handle = document.allocate_handle();
        let handle = content.common.handle;
        document
            .objects
            .insert(handle, ObjectType::TableContent(Box::new(content)));
        let loaded = dwg_roundtrip(&document);
        let Some(ObjectType::TableContent(content)) = loaded.objects.get(&handle) else {
            panic!("TABLECONTENT object missing after DWG read");
        };
        assert_table_merges(content);
        for binary in [false, true] {
            let converted = dxf_roundtrip(&loaded, binary);
            let Some(ObjectType::TableContent(content)) = converted.objects.get(&handle) else {
                panic!("TABLECONTENT object missing after DXF read");
            };
            assert_table_merges(content);
        }
    }
}

#[test]
fn raw_dxf_style_suppression_preserves_the_initial_version_group() {
    for binary in [false, true] {
        for missing_flags in [false, true] {
            let mut document = CadDocument::with_version(DxfVersion::AC1032);
            let table_style = style_mut(&mut document);
            table_style.flags = TableStyleFlags::from_bits_retain(0x13);
            let mut raw = vec![
                (5, table_style.handle.to_string()),
                (330, table_style.owner_handle.to_string()),
                (100, "AcDbTableStyle".to_string()),
                (280, "3".to_string()),
                (3, "Standard".to_string()),
            ];
            if !missing_flags {
                raw.push((71, "19".to_string()));
            }
            raw.push((300, "Preserved style metadata".to_string()));
            table_style.raw_dxf_codes = Some(raw);
            let mut loaded = dxf_roundtrip(&document, binary);
            assert_suppression(&loaded, true, true);
            assert_eq!(style(&loaded).version, 3);
            style_mut(&mut loaded).set_title_visible(true);
            style_mut(&mut loaded).set_header_visible(true);
            let resaved = dxf_roundtrip(&loaded, binary);
            assert_suppression(&resaved, false, false);
            assert_eq!(style(&resaved).version, 3);
            assert!(style(&resaved)
                .raw_dxf_codes
                .as_ref()
                .unwrap()
                .contains(&(300, "Preserved style metadata".to_string())));
        }
    }
}
