# Test font faces

Font files that exist only so a test can create a condition. Not production
assets: nothing here is served to a reader, bundled into the engine, or
`include_bytes!`-ed by anything but a test target.

| File | Licence | Bytes |
| --- | --- | --- |
| `NotoSansCJK-TestSubset.otf` | SIL OFL 1.1 (`OFL-1.1-NotoSansCJK.txt`) | 8,652 |
| `NotoSansCJKkr-TestSubset.otf` | SIL OFL 1.1 (`OFL-1.1-NotoSansCJK.txt`) | 4,892 |
| `NotoSansCJKjp-TestSubset.otf` | SIL OFL 1.1 (`OFL-1.1-NotoSansCJK.txt`) | 8,588 |

## `NotoSansCJK-TestSubset.otf`

A 19-codepoint subset of **Noto Sans CJK SC**, covering one small sample of each
CJK script family:

| Script | Scalars |
| --- | --- |
| Han | 中 文 字 測 试 試(験) 日 本 語 漢 国 |
| Hiragana | あ い う |
| Katakana | ア イ ウ |
| Hangul | 한 글 |

Used by `crates/casual-doc-layout/tests/cjk_coverage.rs` and
`crates/casual-doc-render/tests/cjk_paint.rs` to prove, deterministically and
offline, that a face registered through the host seam
(`ParleyShaper::register_fallback_font`) is selected **because it covers the
code point** — checked against the face's own `cmap` — and that the renderer
rasterizes real glyphs rather than the notdef box. `109` HF-176.

### Why a subset and not the real face

The production face is 16.4 MB, and the owner's font-provisioning decision is
that the browser fetches CJK over the network rather than carrying it in the
bundle — so committing one would contradict the design the tests are there to
protect. Subsetting to exactly the scalars the tests name also keeps the fixture
honest about what it proves: it covers these 19 scalars and nothing else, so a
test cannot accidentally come to rely on coverage it was never given.

### It is NOT derived from any corpus document

The scalars above were chosen by hand to sample the four script families. The
owner's real CJK `.docx` files in `~/Downloads` are private, are excluded by
`.gitignore`, and nothing here is derived from them or quotes them.

### Regenerating

Requires `fonttools` (`pip install fonttools`) and the upstream face from
`github.com/notofonts/noto-cjk` at commit
`f8d157532fbfaeda587e826d4cd5b21a49186f7c`,
`Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf`:

```sh
python3 -m fontTools.subset NotoSansCJKsc-Regular.otf \
  --text="中文字測试试験日本語한글あいうアイウ漢国" \
  --output-file=NotoSansCJK-TestSubset.otf \
  --no-hinting --desubroutinize --drop-tables+=DSIG \
  --name-IDs='*' --notdef-outline --notdef-glyph
```

`--notdef-outline` is load-bearing and was missed the first time.
`pyftsubset` strips the `.notdef` outline by default, which left glyph 0 blank —
and `cjk_paint.rs` compares a real CJK glyph's raster against *this face's own*
`.notdef` raster, so a blank box would have made that comparison trivially true
and the guard worth less than it claims. The test asserts the `.notdef` has ink
precisely so that mistake cannot return silently.

`--name-IDs='*'` keeps the `name` table, including the OFL licence record
(ID 13/14) that the licence claim above is verified from.

## `NotoSansCJKkr-TestSubset.otf`

A 14-syllable subset (plus the space) of **Noto Sans CJK KR**, the face the
browser registers for Hangul (`webapp/src/web_fonts.mjs`, bucket `kr`). Its
source is that exact production file: `NotoSansCJKkr-Regular.otf` from
`github.com/notofonts/noto-cjk` at the same commit as above
(`Sans/OTF/Korean/`, 16,433,112 bytes, SHA-256
`6bcb2a0703aa137e874fc2dffa85f6c21ba9a67fa329e81b8c801663af7e992a`, the value
the web manifest pins), `name` ID 5 `Version 2.004`.

Covered: 검 국 글 기 내 대 문 민 보 사 서 세 청 한 — the hand-written sentence
"대한민국 국세청 한글 문서 내보내기 검사" ("Republic of Korea, National Tax
Service, Hangul document, export check"), chosen for this test and not taken
from any document.

Used by `crates/casual-doc-wasm/src/pdf_fonts_tests.rs` and
`crates/casual-doc-pdf/tests/registered_fonts.rs` (`109` HF-293) to prove that a
PDF export embeds the Korean face the host registered, that the text extracts
back as Hangul, and that it draws ink rather than `.notdef`. Like any
`pyftsubset` output of a CID-keyed face it keeps its source CIDs (glyph 2 is
CID 47739), which is precisely the case the PDF writer's charset rewrite exists
for, so it guards that too.

Regenerating (deterministic: two runs produce SHA-256
`aaaf46db77171c8b198f69151dbd52bb20e3f12d50936026f148a0e719d68efd` with
fontTools 4.65.0):

```sh
python3 -m fontTools.subset NotoSansCJKkr-Regular.otf \
  --text="대한민국 국세청 한글 문서 내보내기 검사" \
  --output-file=NotoSansCJKkr-TestSubset.otf \
  --no-hinting --desubroutinize --drop-tables+=DSIG \
  --name-IDs='*' --notdef-outline --notdef-glyph
```

The full production face is deliberately not committed (see above). The opt-in
test `the_production_korean_face_exports_whole_and_extracts_as_hangul` in
`crates/casual-doc-pdf/tests/registered_fonts.rs` runs against it when
`OPENDOC_NOTO_CJK_KR` names a local copy. It checks the pinned size only, so
verify the copy first with `sha256sum` against the digest above:

```sh
sha256sum NotoSansCJKkr-Regular.otf   # 6bcb2a07…af7e992a
OPENDOC_NOTO_CJK_KR=$PWD/NotoSansCJKkr-Regular.otf \
  cargo test -p casual-doc-pdf --test registered_fonts -- --ignored
```

## `NotoSansCJKjp-TestSubset.otf`

A 17-character subset of **Noto Sans CJK JP**, for the browser's `jp` bucket
(`Hani`, `Hira`, `Kana`). Covered: the hand-written sentence
"日本語の文書をエクスポートする試験" ("a test of exporting a Japanese document"),
chosen for this test and not taken from any document — Han, Hiragana and
Katakana in one line, so one paragraph shapes as three script runs.

Used by `crates/casual-doc-wasm/src/pdf_fonts_tests.rs` (the font matrix, and
the corpus `scripts/pdf-readers/check.py` reads with Poppler and MuPDF) and
`webapp/tests/document_font_lifecycle_wasm.test.mjs` (`109` HF-294). Like the KR
subset it keeps its source CIDs (glyph 1 is CID 1484), so it exercises the PDF
writer's charset rewrite too.

Its source is NOT the pinned production file: it is face 0 of the Noto Sans
CJK collection `NotoSansCJK-Regular.ttc` as Arch Linux packages it
(`noto-fonts-cjk` 20240730-1, SHA-256
`b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a`), whose
`name` ID 5 is `Version 2.004` — the same release as the pinned
`NotoSansCJKjp-Regular.otf`. Regenerating (deterministic: two runs produce
SHA-256 `27b5ea7730af6e08184a79c7d06d2e46d38924980b5bb80371193da69cde8a78`
with fontTools 4.65.0):

```sh
python3 -m fontTools.subset NotoSansCJK-Regular.ttc --font-number=0 \
  --text="日本語の文書をエクスポートする試験" \
  --output-file=NotoSansCJKjp-TestSubset.otf \
  --no-hinting --desubroutinize --drop-tables+=DSIG \
  --name-IDs='*' --notdef-outline --notdef-glyph
```

The Latin case of the same matrix uses no fixture from here: it registers the
browser's own `webapp/assets/fonts/script/NotoSerif-Variable.ttf`, the committed
production face, so that case tests exactly what a reader's browser registers.
