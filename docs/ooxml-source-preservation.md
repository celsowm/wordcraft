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

The existing wordcraft_docx::read/write API is intentionally unchanged.
SourceBackedDocx::open/save must be explicitly used by a caller. That avoids
changing memory costs, serialization and behavior of existing applications.

## Known limits: do not claim lossless edited DOCX yet

1. XML extensions *inside a modified part* may be discarded. Oasis solves this
   through source fragments and schema-aware, local element patching. Port that
   next for paragraphs/runs, then tables/sections and headers/footers.
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

- M2: associate source fragments with semantic paragraphs/runs and retain
  unknown nodes/attributes during local text or formatting edits.
- M3: namespace/MC processing and safe OPC relationship path remapping.
- M4: custom-XML data-binding synchronization, where bindings are supported.
- M5: corpus of DOCX samples, repeated edited save cycles, Open XML validation
  and layout comparisons. Include legal documents and complex tables.

The implementation is pure Rust and uses only existing XML/ZIP dependencies.
It is compatible in principle with wasm32-unknown-unknown; CI checks this.
