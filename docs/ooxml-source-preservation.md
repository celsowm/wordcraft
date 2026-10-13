# Source-backed DOCX preservation (Oasis-inspired)

The opt-in Rust API SourceBackedDocx in crates/docx/src/preserve.rs is the
first migration milestone of the Oasis Editor preservation strategy.

## What is implemented

- Open a transitional DOCX with the normal WordCraft semantic reader while
  retaining the original bounded OPC parts in memory.
- Save without modifications with the source XML entries unchanged.
- Compare each current canonical output part against the canonical projection
  of the imported model. If a part is unchanged, reuse its original XML bytes.
- Keep source-only package parts, including unfamiliar XML and binary data,
  with basic path/case-collision protection.
- Merge source-only relationships and content-type declarations into rebuilt
  manifests, preserving original relationship IDs where safe.
- Rebuild modified modeled parts using the existing canonical writer.
- Regression tests cover unchanged source, editing main text and editing only
  metadata with unknown XML, custom data and opaque binary entries.

The existing wordcraft_docx::read/write API is unchanged. The engine now accepts
`file.open` with `{"path":"example.docx","preserveSource":true}` (with optional
base64 `data`, including in the browser), and native `file.save`/Save As and
browser downloads use the retained source automatically. This opt-in mode is
limited to unencrypted transitional DOCX files at most 16 MiB. Replacing the
document clears the source snapshot, and non-DOCX exports still use their
existing writers. The default file-open behavior remains canonical while this
mode is validated on real-world files. To exercise it through the actual web
picker/drag-and-drop, visit `https://celsowm.github.io/wordcraft/?preserveDocx=1`.
The flag applies only to DOCX uploads; other formats keep normal import.
Saving/downloading the opened DOCX uses the same retained source snapshot.

## M2 (initial): unchanged top-level body fragments

For body content with the same sequence of direct paragraphs, tables and SDT
blocks, a token-offset patch now reuses untouched raw XML ranges from the source
document and replaces only changed blocks. Non-block XML and namespace declarations
are kept from the source. Generated replacement blocks carry their namespace
declarations locally, so producer-specific prefixes cannot invalidate them.

The optimization is deliberately disabled if the modeled block sequence changes
or if the canonical XML outside those blocks changes. It is not a complete
run-level merge: editing a paragraph can still drop unknown markup inside that
same paragraph. Tests include source-only extension siblings and untouched
paragraph preservation across edits.

## Known limits: do not claim lossless edited DOCX yet

1. XML extensions inside *changed individual paragraphs/tables* may still be
   discarded. Oasis solves this with source fragments and schema-aware merging.
   Add granular runs and paragraph properties, then tables/sections and stories.
2. Source relationships whose IDs conflict with generated IDs are skipped
   when the XML owner was rebuilt. Precise relationship-ID remapping requires
   a semantic reference graph and must not be guessed.
3. The package overlay uses conventional generated paths. Producer-specific
   alternate paths for modeled parts need relationship-discovered aliases.
4. A source-only part is preserved even if no live element uses it. Further
   graph analysis is needed for safe garbage collection/deletions.
5. This mode accepts transitional DOCX only. Macro documents and templates
   should continue to use the ordinary read/write path.
6. In-memory source and baseline copies increase memory usage; callers should
   set an application-level policy for large documents and web builds.
7. Existing tests are synthetic. Real Word/LibreOffice/Google Docs corpus tests
   and Open XML SDK checks are required before making interoperability claims.

## Follow-up milestones

- M2 continued: source fragments for changed paragraphs/runs and XML-property
  merge; then tables, sections and related stories.
- M3: namespace/MC processing and safe OPC relationship path remapping.
- M4: custom-XML data-binding synchronization, where bindings are supported.
- M5: corpus of DOCX samples, repeated edited save cycles, Open XML validation
  and layout comparisons. Include legal documents and complex tables.

The implementation is pure Rust and uses only existing XML/ZIP dependencies.
It is compatible in principle with wasm32-unknown-unknown; CI checks this.
