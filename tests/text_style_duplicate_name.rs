//! A drawing can hold two text style records of one name, each with its own
//! font, and texts that name either by handle. Reading a DWG keeps both
//! records and each text's record, and writing one keeps them apart.

use std::io::Cursor;

use acadrust::entities::{EntityType, MText, Text};
use acadrust::tables::TextStyle;
use acadrust::{CadDocument, DwgReader, DwgWriter};

#[test]
fn two_style_records_of_one_name_stay_apart_through_dwg() {
    let mut document = CadDocument::new();
    let shx = document
        .text_styles
        .get("Standard")
        .expect("the default Standard")
        .handle;
    let mut verdana = TextStyle::new("Standard");
    verdana.handle = document.allocate_handle();
    verdana.true_type_font = "Verdana.ttf".into();
    verdana.font_file = "Verdana.ttf".into();
    let verdana_handle = verdana.handle;
    document.text_styles.add_allow_duplicate(verdana);

    let mut by_shx = Text::new();
    by_shx.value = "txt".into();
    by_shx.style_handle = Some(shx);
    let mut by_verdana = Text::new();
    by_verdana.value = "verdana".into();
    by_verdana.style_handle = Some(verdana_handle);
    let mut mtext = MText::new();
    mtext.value = "verdana mtext".into();
    mtext.style_handle = Some(verdana_handle);
    document.add_entity(EntityType::Text(by_shx)).expect("text");
    document.add_entity(EntityType::Text(by_verdana)).expect("text");
    document.add_entity(EntityType::MText(mtext.into())).expect("mtext");

    let dwg = DwgWriter::write_to_vec(&document).expect("write");
    let read = DwgReader::from_stream(Cursor::new(dwg))
        .read()
        .expect("read");

    let standards: Vec<&TextStyle> = read
        .text_styles
        .iter()
        .filter(|s| s.name.eq_ignore_ascii_case("Standard"))
        .collect();
    assert_eq!(standards.len(), 2, "both records of the name");
    let font_of = |handle| {
        read.text_styles
            .iter()
            .find(|s| Some(s.handle) == handle)
            .map(|s| s.font_file.clone())
            .expect("the text's record")
    };
    let mut seen = Vec::new();
    for entity in read.entities() {
        match entity {
            EntityType::Text(t) => {
                assert_eq!(t.style, "Standard");
                seen.push((t.value.clone(), font_of(t.style_handle)));
            }
            EntityType::MText(m) => seen.push((m.value.clone(), font_of(m.style_handle))),
            _ => {}
        }
    }
    seen.sort();
    let default_font = document.text_styles.get("Standard").expect("default").font_file.clone();
    assert_eq!(
        seen,
        [
            ("txt".to_owned(), default_font),
            ("verdana".to_owned(), "Verdana.ttf".to_owned()),
            ("verdana mtext".to_owned(), "Verdana.ttf".to_owned()),
        ]
    );
}
