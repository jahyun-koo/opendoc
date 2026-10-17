// SPDX-License-Identifier: Apache-2.0

//! `exportAs("application.pdf")` lays the document out with the faces the host
//! registered on THIS document (`docs/109` HF-293).
//!
//! The browser has no system fonts. It registers Roboto by name
//! (`registerFonts`) and Noto CJK as a script fallback (`registerFallbackFont`)
//! on the session's shaper. The PDF adapter used to lay the document out again on a fresh shaper, so
//! none of those reached the file: a Korean document exported every syllable as
//! `.notdef`, and the report was empty.
//!
//! The browser's position is reproduced natively by swapping in
//! [`ParleyShaper::without_system_fonts`] before anything is registered, which
//! is the shaper the `wasm32` build has; otherwise an installed system CJK face
//! would answer for Hangul and the guards would pass on a developer machine for
//! the wrong reason.

use casual_doc_export::write_document;
use casual_doc_layout::shape::ParleyShaper;
use casual_doc_model::NodeId;
use casual_doc_model::v1::{
    BlockNode, Definitions, Document, FontName, FontRef, InlineNode, Paragraph,
    ParagraphProperties, Run, RunProperties,
};
use casual_doc_pdf::inspect;

use crate::{WasmDocument, open_document};

/// Noto Sans CJK KR, subset to [`KOREAN`] (`fixtures/fonts/README.md`).
const KOREAN_FACE: &[u8] = include_bytes!("../../../fixtures/fonts/NotoSansCJKkr-TestSubset.otf");
const KOREAN: &str = "대한민국 국세청 한글 문서 내보내기 검사";
/// The family the Korean face's `name` table declares.
const KOREAN_FAMILY: &str = "Noto Sans CJK KR";
const PDF: &str = casual_doc_io::formats::PDF;

/// A one-paragraph DOCX whose run asks for `family`, when given, for both its
/// Latin and its East Asian text.
fn docx(text: &str, family: Option<&str>) -> Vec<u8> {
    let id = |n: u64| NodeId::from_parts(n, 1).unwrap();
    let named = family.map(|name| {
        FontRef::Named(FontName {
            name: name.to_owned(),
        })
    });
    let properties = RunProperties {
        font_ref: named.clone(),
        font_ref_east_asia: named,
        ..RunProperties::default()
    };
    let document = Document::new(
        id(1),
        vec![BlockNode::Paragraph(Paragraph {
            id: id(2),
            properties: ParagraphProperties::default().into(),
            inlines: vec![InlineNode::Run(Run {
                id: id(3),
                properties: properties.into(),
                text: text.to_owned(),
            })],
        })],
        Definitions::default(),
    )
    .expect("a valid document");
    write_document(&document, &Default::default()).expect("write the package")
}

/// Opens `bytes` on the browser's shaper: bundled faces only.
fn open_in_browser(bytes: &[u8]) -> WasmDocument {
    let mut doc = open_document(bytes).expect("open");
    doc.shaper = ParleyShaper::without_system_fonts();
    doc.repaginate();
    doc
}

struct Exported {
    text: String,
    faces: Vec<String>,
    report: String,
}

impl Exported {
    fn embeds(&self, postscript: &str) -> bool {
        self.faces
            .iter()
            .any(|face| face.ends_with(&format!("+{postscript}")))
    }

    fn reports_missing_glyphs(&self) -> bool {
        self.report.contains("\"pdf.font.missing_glyphs\"")
    }

    fn words(&self) -> Vec<&str> {
        self.text.split_whitespace().collect()
    }
}

/// `exportAs("application.pdf", "semantic")`, read back as a reader would.
fn export_pdf(doc: &mut WasmDocument) -> Exported {
    let artifact = doc.export_as_inner(PDF, "semantic").expect("export a PDF");
    let pdf = inspect::parse(&artifact.bytes).expect("the export parses");
    Exported {
        text: pdf
            .pages()
            .iter()
            .map(|page| pdf.page_contents(page).plain_text())
            .collect::<Vec<_>>()
            .join("\n"),
        faces: pdf
            .embedded_fonts()
            .into_iter()
            .map(|font| font.base_font)
            .collect(),
        report: artifact.report_json,
    }
}

#[test]
fn a_fallback_face_registered_on_the_document_is_the_one_its_pdf_embeds() {
    let text = format!("{KOREAN} PDF");
    let mut doc = open_in_browser(text.as_bytes());

    // Before the host's Korean download lands: drawn as `.notdef`, and said so.
    let before = export_pdf(&mut doc);
    assert!(before.reports_missing_glyphs(), "{}", before.report);
    assert!(
        !before.embeds("NotoSansCJKkr-Regular"),
        "{:?}",
        before.faces
    );

    doc.register_fallback_font(KOREAN_FACE, vec!["Hani".to_owned(), "Hang".to_owned()]);
    let after = export_pdf(&mut doc);
    assert!(
        after.embeds("NotoSansCJKkr-Regular"),
        "the registered face is embedded: {:?}",
        after.faces
    );
    assert!(!after.reports_missing_glyphs(), "{}", after.report);
    assert_eq!(after.words(), text.split_whitespace().collect::<Vec<_>>());
}

#[test]
fn a_named_face_registered_with_register_fonts_is_the_one_its_pdf_embeds() {
    // `registerFonts` is the named-family seam (Roboto in the browser). The run
    // asks for the Korean family by name; no fallback is wired, so only the
    // named lookup can find it.
    let mut doc = open_in_browser(&docx(KOREAN, Some(KOREAN_FAMILY)));
    assert!(export_pdf(&mut doc).reports_missing_glyphs());

    doc.register_fonts_inner(KOREAN_FACE, &[u32::try_from(KOREAN_FACE.len()).unwrap()])
        .expect("register the named face");
    let after = export_pdf(&mut doc);
    assert!(after.embeds("NotoSansCJKkr-Regular"), "{:?}", after.faces);
    assert!(!after.reports_missing_glyphs(), "{}", after.report);
    assert_eq!(after.words(), KOREAN.split_whitespace().collect::<Vec<_>>());
}

#[test]
fn a_face_registered_on_one_document_never_reaches_anothers_pdf() {
    let mut a = open_in_browser(KOREAN.as_bytes());
    let mut b = open_in_browser(KOREAN.as_bytes());
    a.register_fallback_font(KOREAN_FACE, vec!["Hani".to_owned(), "Hang".to_owned()]);

    let a_pdf = export_pdf(&mut a);
    let b_pdf = export_pdf(&mut b);
    assert!(a_pdf.embeds("NotoSansCJKkr-Regular"));
    assert!(!a_pdf.reports_missing_glyphs(), "{}", a_pdf.report);
    assert!(!b_pdf.embeds("NotoSansCJKkr-Regular"), "{:?}", b_pdf.faces);
    assert!(b_pdf.reports_missing_glyphs(), "{}", b_pdf.report);
}

#[test]
fn exporting_leaves_the_session_and_a_repeat_export_unchanged() {
    // The export reads the live shaper; it must not register anything, so the
    // page on screen and a second export are what they were.
    let mut doc = open_in_browser(KOREAN.as_bytes());
    doc.register_fallback_font(KOREAN_FACE, vec!["Hani".to_owned(), "Hang".to_owned()]);
    let page_before = doc.render_page_inner(0, 48.0).expect("render").rgba;
    let first = doc.export_as_inner(PDF, "semantic").expect("export").bytes;
    let second = doc.export_as_inner(PDF, "semantic").expect("export").bytes;
    let page_after = doc.render_page_inner(0, 48.0).expect("render").rgba;
    assert!(first == second, "the PDF is deterministic");
    assert!(
        page_before == page_after,
        "exporting did not change the page"
    );
}

// ---------------------------------------------------------------------------
// The font matrix (`docs/109` HF-294).
//
// The guards above are about Korean. What they proved — the export reads the
// session's shaper — is GENERAL, but "Korean works" is not "fonts work": a
// named TrueType face goes through the glyf subsetter, a CID-keyed CFF face
// through the charset rewrite, and each script reaches its face through a
// different fallback registration. One case per path, every one on the
// browser's shaper and through `exportAs`:
//
// | case               | class    | path                                         |
// | ------------------ | -------- | -------------------------------------------- |
// | `latin-bundled`    | general  | bundled TrueType, nothing registered         |
// | `latin-named-ttf`  | format   | `registerFonts`, variable TrueType (glyf)    |
// | `korean-cff`       | script   | `registerFallbackFont`, CID CFF, `Hang`      |
// | `chinese-cff`      | script   | `registerFallbackFont`, CID CFF, `Hani`      |
// | `japanese-cff`     | script   | `registerFallbackFont`, CID CFF, `Hira/Kana` |
// | `mixed-ttf-cff`    | general  | named TrueType and fallback CFF in one file  |
//
// Each case is exported before registration (the control), after it, and again
// on a SECOND engine that opened the same bytes (the HF-292 lifecycle: a fresh
// engine has none of the first one's faces and must be given its own). With
// `OPENDOC_PDF_READER_CORPUS` naming a directory, the PDFs, the controls and
// the engine's own raster of page 1 are written there for
// `scripts/pdf-readers/check.py`, which reads them with Poppler and MuPDF —
// two readers that are not this crate — and fails on blank pages, ▯ and text
// that does not extract (`.github/workflows/ci.yml`, job `pdf-readers`).
// ---------------------------------------------------------------------------

/// Noto Sans CJK SC, subset to 19 scalars (`fixtures/fonts/README.md`).
const CHINESE_FACE: &[u8] = include_bytes!("../../../fixtures/fonts/NotoSansCJK-TestSubset.otf");
/// Noto Sans CJK JP, subset to [`JAPANESE`] (`fixtures/fonts/README.md`).
const JAPANESE_FACE: &[u8] = include_bytes!("../../../fixtures/fonts/NotoSansCJKjp-TestSubset.otf");
/// The browser's own named face for `Noto Serif` (`webapp/src/web_fonts.mjs`,
/// `NAMED_WEB_FONT_FACES`): a variable TrueType the engine does not bundle.
const LATIN_NAMED_FACE: &[u8] =
    include_bytes!("../../../webapp/assets/fonts/script/NotoSerif-Variable.ttf");
const CHINESE: &str = "中文 文字 中国";
const JAPANESE: &str = "日本語の文書をエクスポートする試験";
/// Plain Latin, with an `fi`/`ffl` pair so a ligature glyph has to map back to
/// its letters in the text layer.
const LATIN: &str = "Quick brown fox files waffle 0123";
/// The engine's raster resolution for the corpus; the checker renders the PDF
/// at the same value.
const CORPUS_DPI: f32 = 96.0;

#[derive(Clone, Copy)]
enum Registration {
    /// `registerFonts`: by family name, no coverage fallback.
    Named(&'static [u8]),
    /// `registerFallbackFont`: by script.
    Fallback(&'static [u8], &'static [&'static str]),
}

struct Case {
    id: &'static str,
    class: &'static str,
    text: String,
    /// The family the run names, for both its Latin and East Asian text.
    family: Option<&'static str>,
    registrations: Vec<Registration>,
    /// PostScript names the export must embed once registered.
    faces: &'static [&'static str],
    /// Whether the control (nothing registered) draws `.notdef`. A Latin face
    /// that is missing is substituted, not ▯, so its control is the wrong face.
    control_is_notdef: bool,
}

fn matrix() -> Vec<Case> {
    let kr = Registration::Fallback(KOREAN_FACE, &["Hani", "Hang"]);
    vec![
        Case {
            id: "latin-bundled",
            class: "general",
            text: LATIN.to_owned(),
            family: None,
            registrations: vec![],
            faces: &[],
            control_is_notdef: false,
        },
        Case {
            id: "latin-named-ttf",
            class: "format",
            text: LATIN.to_owned(),
            family: Some("Noto Serif"),
            registrations: vec![Registration::Named(LATIN_NAMED_FACE)],
            faces: &["NotoSerif-Regular"],
            control_is_notdef: false,
        },
        Case {
            id: "korean-cff",
            class: "script",
            text: KOREAN.to_owned(),
            family: None,
            registrations: vec![kr],
            faces: &["NotoSansCJKkr-Regular"],
            control_is_notdef: true,
        },
        Case {
            id: "chinese-cff",
            class: "script",
            text: CHINESE.to_owned(),
            family: None,
            // The `sc` bucket's scripts.
            registrations: vec![Registration::Fallback(CHINESE_FACE, &["Hani"])],
            faces: &["NotoSansCJKsc-Regular"],
            control_is_notdef: true,
        },
        Case {
            id: "japanese-cff",
            class: "script",
            text: JAPANESE.to_owned(),
            family: None,
            // The `jp` bucket's scripts.
            registrations: vec![Registration::Fallback(
                JAPANESE_FACE,
                &["Hani", "Hira", "Kana"],
            )],
            faces: &["NotoSansCJKjp-Regular"],
            control_is_notdef: true,
        },
        Case {
            id: "mixed-ttf-cff",
            class: "general",
            text: format!("Serif {KOREAN} PDF"),
            family: Some("Noto Serif"),
            registrations: vec![Registration::Named(LATIN_NAMED_FACE), kr],
            faces: &["NotoSerif-Regular", "NotoSansCJKkr-Regular"],
            control_is_notdef: true,
        },
    ]
}

fn register(doc: &mut WasmDocument, registration: Registration) {
    match registration {
        Registration::Named(bytes) => doc
            .register_fonts_inner(bytes, &[u32::try_from(bytes.len()).unwrap()])
            .expect("register the named face"),
        Registration::Fallback(bytes, scripts) => doc.register_fallback_font(
            bytes,
            scripts.iter().map(|script| (*script).to_owned()).collect(),
        ),
    }
}

/// Page 1 as 8-bit grey composited on white, the shape a reader's raster has.
fn grey_page(doc: &mut WasmDocument) -> (u32, u32, Vec<u8>) {
    let page = doc.render_page_inner(0, CORPUS_DPI).expect("render");
    let grey = page
        .rgba
        .chunks_exact(4)
        .map(|px| {
            let [r, g, b, a] = [px[0], px[1], px[2], px[3]].map(u32::from);
            let luma = (299 * r + 587 * g + 114 * b) / 1000;
            u8::try_from((luma * a + 255 * (255 - a)) / 255).unwrap()
        })
        .collect();
    (page.width_px, page.height_px, grey)
}

/// The words of an export: no `.notdef` reported and the text layer intact.
fn assert_clean(case: &Case, label: &str, export: &Exported) {
    assert!(
        !export.reports_missing_glyphs(),
        "{}: {label} draws .notdef: {}",
        case.id,
        export.report
    );
    for face in case.faces {
        assert!(
            export.embeds(face),
            "{}: {label} must embed {face}: {:?}",
            case.id,
            export.faces
        );
    }
    // Compared without whitespace: the inspector joins each drawn run with a
    // space, and Japanese is one run per script (Han, Hiragana, Katakana).
    assert_eq!(
        export.words().concat(),
        case.text.split_whitespace().collect::<String>(),
        "{}: {label}'s text layer",
        case.id
    );
}

#[test]
fn every_font_path_reaches_the_pdf_and_a_second_engine_must_register_its_own() {
    let corpus = std::env::var_os("OPENDOC_PDF_READER_CORPUS").map(std::path::PathBuf::from);
    if let Some(dir) = &corpus {
        std::fs::create_dir_all(dir).expect("create the corpus directory");
    }
    let mut manifest = Vec::new();
    for case in matrix() {
        let bytes = docx(&case.text, case.family);

        // A: the control first, then the faces.
        let mut a = open_in_browser(&bytes);
        let control_bytes = a.export_as_inner(PDF, "semantic").expect("export").bytes;
        let control = export_pdf(&mut a);
        let control_raster = grey_page(&mut a);
        assert_eq!(
            control.reports_missing_glyphs(),
            case.control_is_notdef,
            "{}: the control: {}",
            case.id,
            control.report
        );
        for face in case.faces {
            assert!(
                !control.embeds(face),
                "{}: nothing registered, yet {face} is embedded",
                case.id
            );
        }
        for registration in &case.registrations {
            register(&mut a, *registration);
        }
        let a_export = export_pdf(&mut a);
        assert_clean(&case, "A", &a_export);
        let a_bytes = a.export_as_inner(PDF, "semantic").expect("export").bytes;
        let a_raster = grey_page(&mut a);
        if !case.registrations.is_empty() {
            assert!(
                a_raster != control_raster,
                "{}: registering changed nothing on the page, so the raster reference \
                 cannot tell the face from its absence",
                case.id
            );
        }

        // B: a second engine on the same bytes starts with none of A's faces.
        let mut b = open_in_browser(&bytes);
        let b_before = b.export_as_inner(PDF, "semantic").expect("export").bytes;
        assert!(
            b_before == control_bytes,
            "{}: A's registration leaked into B's export",
            case.id
        );
        for registration in &case.registrations {
            register(&mut b, *registration);
        }
        let b_bytes = b.export_as_inner(PDF, "semantic").expect("export").bytes;
        assert!(
            b_bytes == a_bytes,
            "{}: B given the same faces must export the same file",
            case.id
        );
        assert!(
            grey_page(&mut b) == a_raster,
            "{}: B's page is A's",
            case.id
        );

        if let Some(dir) = &corpus {
            let write = |name: String, data: &[u8]| {
                std::fs::write(dir.join(name), data).expect("write the corpus");
            };
            let pgm = |(width, height, grey): &(u32, u32, Vec<u8>)| {
                let mut out = format!("P5\n{width} {height}\n255\n").into_bytes();
                out.extend_from_slice(grey);
                out
            };
            write(format!("{}.pdf", case.id), &a_bytes);
            write(format!("{}.control.pdf", case.id), &control_bytes);
            write(format!("{}.engine.pgm", case.id), &pgm(&a_raster));
            manifest.push(serde_json::json!({
                "id": case.id,
                "class": case.class,
                "text": case.text,
                "faces": case.faces,
                "controlIsNotdef": case.control_is_notdef,
                "registered": !case.registrations.is_empty(),
                "pdf": format!("{}.pdf", case.id),
                "control": format!("{}.control.pdf", case.id),
                "engine": format!("{}.engine.pgm", case.id),
            }));
        }
    }
    if let Some(dir) = &corpus {
        let manifest = serde_json::json!({ "dpi": CORPUS_DPI, "cases": manifest });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .expect("write the manifest");
    }
}
