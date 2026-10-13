//! Source-preservation integration through the actual command-based Open/Save path.
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use serde_json::json;
use wordcraft_doc::Document;
use wordcraft_engine::Session;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("DOCX zip");
    let mut map = BTreeMap::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("entry");
        if entry.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        map.insert(entry.name().to_string(), data);
    }
    map
}

fn assemble(entries: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(name, SimpleFileOptions::default()).expect("start");
        zip.write_all(&bytes).expect("write");
    }
    zip.finish().expect("finish").into_inner()
}

fn fixture() -> Vec<u8> {
    let base = wordcraft_docx::write(&Document::from_text("Original")).expect("write");
    let mut map = entries(&base);
    map.insert("customXml/opaque99.xml".into(), b"<legal:case xmlns:legal=\"urn:legal\"><legal:id>42</legal:id></legal:case>".to_vec());
    let types = String::from_utf8(map["[Content_Types].xml"].clone()).expect("utf8");
    map.insert(
        "[Content_Types].xml".into(),
        types.replace("</Types>", "<Override PartName=\"/customXml/opaque99.xml\" ContentType=\"application/xml\"/></Types>").into_bytes(),
    );
    assemble(map)
}

fn scratch(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("wordcraft-source-open-save-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("directory");
    path
}

#[test]
fn explicit_source_mode_survives_command_open_edit_save() {
    let dir = scratch("roundtrip");
    let input = dir.join("original.docx");
    let output = dir.join("edited.docx");
    std::fs::write(&input, fixture()).expect("input");
    let mut s = Session::new(Document::new());
    s.run("file.open", &json!({"path": input.to_string_lossy(), "preserveSource": true})).expect("open source");
    assert!(s.source_docx.is_some());
    s.run("document.setText", &json!({"text": "Edited by the user"})).expect("edit");
    s.run("file.saveAs", &json!({"path": output.to_string_lossy()})).expect("save");
    let parts = entries(&std::fs::read(&output).expect("output"));
    assert_eq!(
        parts.get("customXml/opaque99.xml").map(Vec::as_slice),
        Some(b"<legal:case xmlns:legal=\"urn:legal\"><legal:id>42</legal:id></legal:case>".as_slice())
    );
    let reopened = wordcraft_docx::read(&std::fs::read(&output).expect("output")).expect("reopened");
    assert!(reopened.plain_text(wordcraft_doc::StoryRef::Body).contains("Edited by the user"));
}

#[test]
fn new_document_discards_source_snapshot() {
    let dir = scratch("new");
    let input = dir.join("original.docx");
    std::fs::write(&input, fixture()).expect("input");
    let mut s = Session::new(Document::new());
    s.run("file.open", &json!({"path": input.to_string_lossy(), "preserveSource": true})).expect("open");
    assert!(s.source_docx.is_some());
    s.run("file.new", &json!({})).expect("new");
    assert!(s.source_docx.is_none());
}

#[test]
fn default_open_does_not_enable_experimental_mode() {
    let dir = scratch("default");
    let input = dir.join("original.docx");
    std::fs::write(&input, fixture()).expect("input");
    let mut s = Session::new(Document::new());
    s.run("file.open", &json!({"path": input.to_string_lossy()})).expect("open");
    assert!(s.source_docx.is_none());
}

#[test]
fn browser_style_base64_open_and_download_preserve_unknown_parts() {
    let original = fixture();
    let mut s = Session::new(Document::new());
    let data = wordcraft_engine::cmd::insert::base64_encode(&original);
    s.run("file.open", &json!({"path": "uploaded.docx", "data": data, "preserveSource": true})).expect("open uploaded DOCX");
    assert!(s.source_docx.is_some());
    s.run("document.setText", &json!({"text": "Edited in WASM"})).expect("edit");
    let bytes = wordcraft_engine::io::save_bytes_with_source(
        "uploaded.docx",
        &s.doc,
        None,
        &Default::default(),
        s.source_docx.as_deref(),
    )
    .expect("download bytes");
    let zipped = entries(&bytes);
    assert_eq!(
        zipped.get("customXml/opaque99.xml").map(Vec::as_slice),
        Some(b"<legal:case xmlns:legal=\"urn:legal\"><legal:id>42</legal:id></legal:case>".as_slice())
    );
    let reopened = wordcraft_docx::read(&bytes).expect("reopen");
    assert!(reopened.plain_text(wordcraft_doc::StoryRef::Body).contains("Edited in WASM"));
}
