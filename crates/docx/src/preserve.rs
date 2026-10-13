//! Source-backed DOCX round trips inspired by the Oasis Editor OPC preservation layer.
//!
//! Opt-in API: ordinary read/write keep their existing canonical behavior.
//! This keeps the original bounded package beside the editable model and only
//! regenerates parts whose modeled content changed. Unknown OPC entries survive.
//!
//! This is not a lossless editor for XML inside an edited part: that requires
//! source-fragment matching and relationship-aware patching (the next milestone).

use std::collections::{BTreeMap, BTreeSet};

use wordcraft_doc::Document;

use crate::package::{ContentTypes, Package, rt, zip_entries};
use crate::xml::{self, El, W};
use crate::{DocxError, Flavor, read, write};

/// An imported DOCX with a source snapshot for conservative round-trip saves.
///
/// For existing DOCX files only; new documents should use the ordinary writer.
/// The baseline model and the source parts consume additional memory, so callers
/// can opt in according to their file-size and platform policies.
pub struct SourceBackedDocx {
    pub document: Document,
    baseline: Document,
    original: BTreeMap<String, Vec<u8>>,
}

impl SourceBackedDocx {
    /// Read a standard .docx package, retaining all bounded source entries.
    ///
    /// Macro-enabled files and templates deliberately remain on the ordinary
    /// import/export path until flavour-aware preservation is implemented.
    pub fn open(bytes: &[u8]) -> Result<Self, DocxError> {
        let pkg = Package::open(bytes)?;
        let rels = pkg.rels("");
        let main = rels.by_type(rt::OFFICE_DOC).filter(|r| !r.external).map(|r| r.target.as_str()).unwrap_or("word/document.xml");
        let types = ContentTypes::read(&pkg);
        if types.of(main) != Some(Flavor::Document.main_content_type()) {
            return Err(DocxError::NotWord("source-backed preservation currently requires a transitional .docx".into()));
        }
        let document = read::read_opened(&pkg)?;
        let baseline = document.clone();
        Ok(Self { document, baseline, original: pkg.into_parts() })
    }

    /// Serialize edits while preserving source-only package parts and unchanged
    /// XML subdocuments. A changed XML part is still rebuilt canonically.
    pub fn save(&self) -> Result<Vec<u8>, DocxError> {
        if self.document == self.baseline {
            return zip_entries(&self.original.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>());
        }
        let baseline: BTreeMap<String, Vec<u8>> = write::write_parts(&self.baseline, Flavor::Document)?.into_iter().collect();
        let current = write::write_parts(&self.document, Flavor::Document)?;
        let mut result: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut reused = BTreeSet::new();

        for (name, rebuilt) in current {
            // If the canonical projection of a part is unchanged, the original
            // XML (including namespaces, unknown children and attributes) wins.
            // Relationship and content-type manifests are reconciled below.
            let source = self.original.get(&name);
            let unchanged = baseline.get(&name) == Some(&rebuilt);
            if unchanged && !is_relationship_part(&name) && name != "[Content_Types].xml" {
                if let Some(bytes) = source {
                    result.insert(name.clone(), bytes.clone());
                    reused.insert(name);
                    continue;
                }
            }
            result.insert(name, rebuilt);
        }

        // Source-only binary/XML parts survive edits, including future producer
        // extensions. Do not restore standard parts intentionally removed by the
        // modeled writer, or create case-insensitive OPC name collisions.
        let mut names: BTreeSet<String> = result.keys().map(|k| k.to_ascii_lowercase()).collect();
        for (name, bytes) in &self.original {
            if !safe_part_name(name) || is_managed_deleted_part(name) {
                continue;
            }
            if names.insert(name.to_ascii_lowercase()) {
                result.insert(name.clone(), bytes.clone());
            }
        }

        // Keep source relationship IDs when the owning XML part was retained.
        // For rewritten owners canonical relationships take precedence; source
        // IDs that conflict cannot safely be transplanted and are skipped.
        let rel_paths: Vec<String> = result.keys().filter(|k| is_relationship_part(k)).cloned().collect();
        for path in rel_paths {
            let (Some(original), Some(rebuilt)) = (self.original.get(&path), result.get(&path)) else {
                continue;
            };
            let owner = relationship_owner(&path);
            let source_first = owner.as_deref().is_some_and(|o| reused.contains(o));
            let unchanged = baseline.get(&path) == Some(rebuilt);
            if source_first && unchanged {
                result.insert(path, original.clone());
            } else if let Some(merged) = merge_rels(original, rebuilt, source_first) {
                result.insert(path, merged);
            }
        }
        // Source-only .rels were already copied byte-for-byte above.
        if let (Some(original), Some(rebuilt)) = (self.original.get("[Content_Types].xml"), result.get("[Content_Types].xml")) {
            if let Some(merged) = merge_content_types(original, rebuilt) {
                result.insert("[Content_Types].xml".to_string(), merged);
            }
        }

        zip_entries(&result.into_iter().collect::<Vec<_>>())
    }
}

fn safe_part_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && name.split('/').all(|s| !s.is_empty() && s != "." && s != ".." && !s.contains(':'))
}

fn is_relationship_part(path: &str) -> bool {
    path == "_rels/.rels" || (path.contains("/_rels/") && path.ends_with(".rels"))
}

fn relationship_owner(path: &str) -> Option<String> {
    if path == "_rels/.rels" {
        return Some(String::new());
    }
    let (dir, name) = path.rsplit_once("/_rels/")?;
    let name = name.strip_suffix(".rels")?;
    Some(format!("{dir}/{name}"))
}

fn is_managed_deleted_part(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "word/document.xml"
            | "word/styles.xml"
            | "word/numbering.xml"
            | "word/settings.xml"
            | "word/theme/theme1.xml"
            | "word/footnotes.xml"
            | "word/endnotes.xml"
            | "word/comments.xml"
            | "word/commentsextended.xml"
            | "docprops/core.xml"
            | "docprops/app.xml"
            | "docprops/custom.xml"
    ) || n.starts_with("word/header") && n.ends_with(".xml")
        || n.starts_with("word/footer") && n.ends_with(".xml")
}

fn children<'a>(bytes: &'a [u8], kind: &str) -> Option<Vec<El>> {
    let tree = xml::parse(bytes).ok()?;
    Some(tree.els().filter(|e| e.local() == kind).cloned().collect())
}

/// The generated IDs remain authoritative on edited XML. For preserved XML,
/// original IDs win; colliding IDs are omitted instead of creating invalid OPC.
fn merge_rels(original: &[u8], rebuilt: &[u8], source_first: bool) -> Option<Vec<u8>> {
    let orig = children(original, "Relationship")?;
    let generated = children(rebuilt, "Relationship")?;
    let mut ids = BTreeSet::new();
    let mut w = W::new();
    w.open("Relationships", &[("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships")]);
    let primary = if source_first { &orig } else { &generated };
    let secondary = if source_first { &generated } else { &orig };
    for rel in primary.iter().chain(secondary) {
        let (Some(id), Some(kind), Some(target)) = (rel.attr("Id"), rel.attr("Type"), rel.attr("Target")) else {
            continue;
        };
        if !ids.insert(id.to_string()) {
            continue;
        }
        if rel.attr("TargetMode") == Some("External") {
            w.empty("Relationship", &[("Id", id), ("Type", kind), ("Target", target), ("TargetMode", "External")]);
        } else {
            w.empty("Relationship", &[("Id", id), ("Type", kind), ("Target", target)]);
        }
    }
    w.close("Relationships");
    Some(w.into_bytes())
}

fn merge_content_types(original: &[u8], rebuilt: &[u8]) -> Option<Vec<u8>> {
    let source = xml::parse(original).ok()?;
    let generated = xml::parse(rebuilt).ok()?;
    let mut seen_defaults = BTreeSet::new();
    let mut seen_overrides = BTreeSet::new();
    let mut w = W::new();
    w.open("Types", &[("xmlns", "http://schemas.openxmlformats.org/package/2006/content-types")]);
    for el in generated.els().chain(source.els()) {
        match el.local() {
            "Default" => {
                let (Some(ext), Some(ct)) = (el.attr("Extension"), el.attr("ContentType")) else { continue };
                if seen_defaults.insert(ext.to_ascii_lowercase()) {
                    w.empty("Default", &[("Extension", ext), ("ContentType", ct)]);
                }
            }
            "Override" => {
                let (Some(path), Some(ct)) = (el.attr("PartName"), el.attr("ContentType")) else { continue };
                if seen_overrides.insert(path.to_ascii_lowercase()) {
                    w.empty("Override", &[("PartName", path), ("ContentType", ct)]);
                }
            }
            _ => {}
        }
    }
    w.close("Types");
    Some(w.into_bytes())
}
