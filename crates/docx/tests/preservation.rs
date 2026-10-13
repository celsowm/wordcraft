//! Regression cases for opt-in source-backed DOCX preservation.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use wordcraft_doc::props::CharProps;
use wordcraft_doc::{Document, Paragraph, para_block};
use wordcraft_docx::{SourceBackedDocx, write};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut result = BTreeMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).expect("entry");
        if file.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("read entry");
        result.insert(file.name().to_string(), bytes);
    }
    result
}

fn zip_parts(files: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files {
        writer.start_file(path, SimpleFileOptions::default()).expect("start");
        writer.write_all(&bytes).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

fn fixture() -> Vec<u8> {
    let mut p = parts(&write(&Document::new()).expect("canonical DOCX"));
    let body = String::from_utf8(p["word/document.xml"].clone()).expect("xml");
    let body = body.replacen("</w:body>", "<w:unknownExtension w:val=\"keep-me\"/></w:body>", 1);
    p.insert("word/document.xml".into(), body.into_bytes());
    p.insert("customXml/item99.xml".into(), b"<case><id>42</id></case>".to_vec());
    p.insert("word/embeddings/opaque99.bin".into(), vec![1, 9, 4, 2]);
    for path in ["_rels/.rels", "word/_rels/document.xml.rels"] {
        let existing = String::from_utf8(p[path].clone()).expect("relationships");
        let rel = if path == "_rels/.rels" {
            "<Relationship Id=\"rIdCustom99\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml\" Target=\"customXml/item99.xml\"/>"
        } else {
            "<Relationship Id=\"rIdOpaque99\" Type=\"http://example.org/relationships/opaque\" Target=\"embeddings/opaque99.bin\"/>"
        };
        let next = existing.replacen("</Relationships>", &format!("{rel}</Relationships>"), 1);
        p.insert(path.into(), next.into_bytes());
    }
    let types = String::from_utf8(p["[Content_Types].xml"].clone()).expect("content types");
    let types = types.replacen(
        "</Types>",
        "<Default Extension=\"bin\" ContentType=\"application/octet-stream\"/><Override PartName=\"/customXml/item99.xml\" ContentType=\"application/xml\"/></Types>",
        1,
    );
    p.insert("[Content_Types].xml".into(), types.into_bytes());
    zip_parts(p)
}

#[test]
fn unchanged_document_keeps_unknown_xml_and_opc_parts() {
    let original = fixture();
    let source = SourceBackedDocx::open(&original).expect("open");
    let saved = source.save().expect("save");
    assert_eq!(parts(&original), parts(&saved));
}

#[test]
fn edited_body_retains_unknown_package_parts_and_relationships() {
    let original = fixture();
    let mut source = SourceBackedDocx::open(&original).expect("open");
    let mut paragraph = Paragraph::new();
    paragraph.insert_text(0, "Edited by WordCraft", &CharProps::default()).expect("text");
    source.document.body = vec![para_block(paragraph)];
    let output = parts(&source.save().expect("save"));
    assert_eq!(output.get("customXml/item99.xml").map(Vec::as_slice), Some(b"<case><id>42</id></case>".as_slice()));
    assert_eq!(output.get("word/embeddings/opaque99.bin").map(Vec::as_slice), Some([1, 9, 4, 2].as_slice()));
    let rels = String::from_utf8(output["word/_rels/document.xml.rels"].clone()).expect("rels");
    assert!(rels.contains("rIdOpaque99"));
    let root_rels = String::from_utf8(output["_rels/.rels"].clone()).expect("root");
    assert!(root_rels.contains("rIdCustom99"));
    let ct = String::from_utf8(output["[Content_Types].xml"].clone()).expect("types");
    assert!(ct.contains("/customXml/item99.xml"));
    assert!(ct.contains("Extension=\"bin\""));
    let edited = String::from_utf8(output["word/document.xml"].clone()).expect("body");
    assert!(edited.contains("Edited by WordCraft"));
}

#[test]
fn editing_metadata_does_not_rewrite_the_original_document_xml() {
    let original = fixture();
    let raw = parts(&original);
    let mut source = SourceBackedDocx::open(&original).expect("open");
    source.document.core.title = "Updated title".into();
    let output = parts(&source.save().expect("save"));
    assert_eq!(output["word/document.xml"], raw["word/document.xml"]);
    assert_eq!(output["customXml/item99.xml"], raw["customXml/item99.xml"]);
}

#[test]
fn a_text_edit_preserves_source_only_sibling_xml() {
    let original = fixture();
    let mut source = SourceBackedDocx::open(&original).expect("open");
    let mut paragraph = Paragraph::new();
    paragraph.insert_text(0, "Changed paragraph", &CharProps::default()).expect("text");
    source.document.body = vec![para_block(paragraph)];
    let saved = parts(&source.save().expect("save"));
    let xml = String::from_utf8(saved["word/document.xml"].clone()).expect("xml");
    assert!(xml.contains("Changed paragraph"));
    // This source-only OOXML sibling was not part of the semantic model.
    assert!(xml.contains("w:unknownExtension w:val=\"keep-me\""));
}

#[test]
fn editing_a_second_paragraph_preserves_extensions_in_the_first() {
    let mut first = Paragraph::new();
    first.insert_text(0, "First", &CharProps::default()).expect("text");
    let mut second = Paragraph::new();
    second.insert_text(0, "Second", &CharProps::default()).expect("text");
    let mut doc = Document::new();
    doc.body = vec![para_block(first), para_block(second)];

    let mut source_files = parts(&write(&doc).expect("write"));
    let body = String::from_utf8(source_files["word/document.xml"].clone()).expect("xml");
    // Unknown content in an untouched paragraph must survive editing its sibling.
    let body = body.replacen("<w:p>", "<w:p><future:private xmlns:future=\"urn:test:future\" value=\"keep\"/>", 1);
    source_files.insert("word/document.xml".into(), body.into_bytes());
    let raw = zip_parts(source_files);

    let mut document = SourceBackedDocx::open(&raw).expect("open");
    let mut updated = Paragraph::new();
    updated.insert_text(0, "Second edited", &CharProps::default()).expect("text");
    document.document.body[1] = para_block(updated);

    let output = parts(&document.save().expect("save"));
    let xml = String::from_utf8(output["word/document.xml"].clone()).expect("xml");
    assert!(xml.contains("Second edited"));
    assert!(xml.contains("future:private xmlns:future=\"urn:test:future\" value=\"keep\""));
}
