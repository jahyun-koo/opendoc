#!/usr/bin/env python3
"""Reads the engine's PDF font corpus with two independent PDF readers.

`docs/109` HF-294. The corpus is written by the Rust test
`every_font_path_reaches_the_pdf_and_a_second_engine_must_register_its_own`
(`crates/casual-doc-wasm/src/pdf_fonts_tests.rs`) when
`OPENDOC_PDF_READER_CORPUS` names a directory: per case, the exported PDF, a
control exported before the host registered any face, and the engine's own
raster of page 1. Every fixture in it is synthetic: hand-written sentences in
OFL test-subset faces.

The engine's own inspector can say a font is embedded and its text extracts
while a real reader still draws nothing, or draws a box: MuPDF resolved a
subset CID through the charset while Poppler took it as a glyph id, so the same
file was blank in one and fine in the other (HF-293). So this asks the readers
themselves, Poppler (`pdftotext`, `pdffonts`, `pdftoppm`) and MuPDF (PyMuPDF),
for each case:

* text: the page's text, whitespace aside, is the case's text, with no U+FFFD;
* fonts: each face the case names is embedded, and no font is left unembedded;
* ink: page 1 is not blank, and its ink matches the engine's own raster of the
  same page (F1 of the two ink masks, one pixel of tolerance). The engine draws
  the real glyph, so a reader drawing ▯, nothing, or another face scores low;
* discrimination: where the control draws `.notdef`, the same reader's raster
  of the control must score far below the export, which proves the ink check
  can tell ▯ from a glyph on this case rather than assuming it can.

Usage: check.py CORPUS_DIR [--require poppler,mupdf] [--report out.json]
Exits non-zero on any failure, or when a required reader is unavailable.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# An export's ink must agree with the engine's raster at least this well. A
# glyph rasterised by two different rasterisers at one DPI agrees to ~0.9 or
# better; ▯, a blank page or a substituted face does not reach 0.6 on these
# cases (the measured values are printed on every run).
MIN_F1 = 0.75
# A `.notdef` control must score at most this, AND this far below its export:
# if a control scored like its export, the ink check could not tell ▯ from a
# glyph on that case and would be a false green.
MAX_NOTDEF_F1 = 0.6
MIN_MARGIN = 0.2
# Grey levels below this count as ink.
INK = 128
# Below this many ink pixels a page is blank for our purposes.
MIN_INK_PIXELS = 50


class Grey:
    """An 8-bit grey raster."""

    def __init__(self, width: int, height: int, pixels: bytes):
        if len(pixels) != width * height:
            raise ValueError(f"raster is {len(pixels)} bytes, expected {width}x{height}")
        self.width, self.height, self.pixels = width, height, pixels

    @classmethod
    def from_pgm(cls, data: bytes) -> "Grey":
        # P5 with a single whitespace after each header field and maxval 255,
        # the shape both the Rust writer and pdftoppm produce.
        fields: list[bytes] = []
        pos = 0
        while len(fields) < 4:
            while data[pos : pos + 1].isspace():
                pos += 1
            if data[pos : pos + 1] == b"#":
                pos = data.index(b"\n", pos) + 1
                continue
            end = pos
            while not data[end : end + 1].isspace():
                end += 1
            fields.append(data[pos:end])
            pos = end
        if fields[0] != b"P5" or fields[3] != b"255":
            raise ValueError(f"not an 8-bit P5 PGM: {fields}")
        width, height = int(fields[1]), int(fields[2])
        return cls(width, height, data[pos + 1 : pos + 1 + width * height])

    def ink(self) -> set[tuple[int, int]]:
        width = self.width
        return {
            (index % width, index // width)
            for index, value in enumerate(self.pixels)
            if value < INK
        }


def dilate(points: set[tuple[int, int]]) -> set[tuple[int, int]]:
    return {(x + dx, y + dy) for x, y in points for dx in (-1, 0, 1) for dy in (-1, 0, 1)}


def ink_f1(reader: set[tuple[int, int]], engine: set[tuple[int, int]]) -> float:
    """F1 of two ink masks, each pixel matched within one pixel of the other."""
    if not reader or not engine:
        return 0.0
    precision = len(reader & dilate(engine)) / len(reader)
    recall = len(engine & dilate(reader)) / len(engine)
    return 0.0 if precision + recall == 0 else 2 * precision * recall / (precision + recall)


def squash(text: str) -> str:
    return "".join(text.split())


# --- Poppler ---------------------------------------------------------------


def poppler_available() -> bool:
    return all(shutil.which(tool) for tool in ("pdftotext", "pdffonts", "pdftoppm"))


def run(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def poppler_text(pdf: Path) -> str:
    return run("pdftotext", "-enc", "UTF-8", str(pdf), "-")


def poppler_fonts(pdf: Path) -> list[tuple[str, bool]]:
    """(name, embedded) per font. `pdffonts` columns are fixed-width under a
    dashed rule whose dash runs give each column's extent."""
    lines = run("pdffonts", str(pdf)).splitlines()
    rule = lines[1]
    spans, start = [], None
    for index, char in enumerate(rule + " "):
        if char == "-" and start is None:
            start = index
        elif char != "-" and start is not None:
            spans.append((start, index))
            start = None
    name_span, emb_span = spans[0], spans[3]
    return [
        (line[slice(*name_span)].strip(), line[slice(*emb_span)].strip() == "yes")
        for line in lines[2:]
    ]


def poppler_raster(pdf: Path, dpi: float, scratch: Path) -> Grey:
    prefix = scratch / pdf.stem
    run(
        "pdftoppm", "-gray", "-r", f"{dpi:g}", "-f", "1", "-l", "1",
        "-singlefile", str(pdf), str(prefix),
    )
    return Grey.from_pgm((scratch / f"{pdf.stem}.pgm").read_bytes())


# --- MuPDF -----------------------------------------------------------------


def mupdf_module():
    try:
        import pymupdf  # noqa: PLC0415 - optional reader
    except ImportError:
        return None
    return pymupdf


def mupdf_read(pymupdf, pdf: Path, dpi: float) -> tuple[str, list[tuple[str, bool]], Grey]:
    with pymupdf.open(pdf) as document:
        text = "".join(page.get_text() for page in document)
        # (xref, ext, type, basefont, name, encoding, ...): ext is the embedded
        # program's kind, "n/a" when nothing is embedded.
        fonts = [(entry[3], entry[1] != "n/a") for entry in document.get_page_fonts(0, full=False)]
        pix = document[0].get_pixmap(dpi=round(dpi), colorspace=pymupdf.csGRAY, alpha=False)
        raster = Grey(pix.width, pix.height, bytes(pix.samples))
    return text, fonts, raster


# --- The check -------------------------------------------------------------


def check_case(case, corpus, readers, dpi, scratch, failures, results):
    engine = Grey.from_pgm((corpus / case["engine"]).read_bytes())
    engine_ink = engine.ink()
    if len(engine_ink) < MIN_INK_PIXELS:
        failures.append(f"{case['id']}: the engine's own raster is blank")
        return
    for reader, read in readers.items():
        label = f"{case['id']} [{reader}]"
        text, fonts, raster = read(corpus / case["pdf"])
        record = {"case": case["id"], "class": case["class"], "reader": reader}
        results.append(record)

        if "�" in text:
            failures.append(f"{label}: text holds U+FFFD")
        if squash(text) != squash(case["text"]):
            failures.append(f"{label}: text {squash(text)!r} != {squash(case['text'])!r}")
        unembedded = [name for name, embedded in fonts if not embedded]
        if unembedded:
            failures.append(f"{label}: fonts not embedded: {unembedded}")
        for face in case["faces"]:
            if not any(name.endswith(f"+{face}") and embedded for name, embedded in fonts):
                failures.append(f"{label}: {face} is not embedded: {fonts}")

        ink = raster.ink()
        f1 = ink_f1(ink, engine_ink)
        record.update(inkPixels=len(ink), engineInkPixels=len(engine_ink), f1=round(f1, 4))
        if len(ink) < MIN_INK_PIXELS:
            failures.append(f"{label}: page 1 is blank ({len(ink)} ink pixels)")
        if f1 < MIN_F1:
            failures.append(f"{label}: ink F1 {f1:.3f} against the engine raster < {MIN_F1}")

        if case["registered"]:
            _, _, control = read(corpus / case["control"])
            control_f1 = ink_f1(control.ink(), engine_ink)
            record["controlF1"] = round(control_f1, 4)
            if case["controlIsNotdef"] and control_f1 > MAX_NOTDEF_F1:
                failures.append(
                    f"{label}: the .notdef control scores {control_f1:.3f} > {MAX_NOTDEF_F1}, "
                    "so the ink check cannot tell a box from a glyph on this case"
                )
            if f1 - control_f1 < MIN_MARGIN:
                failures.append(
                    f"{label}: export {f1:.3f} vs control {control_f1:.3f}: margin < {MIN_MARGIN}"
                )
        print(
            f"{label:32} text ok={squash(text) == squash(case['text'])!s:5} "
            f"ink={len(ink):6} f1={f1:.3f}"
            + (f" control={record['controlF1']:.3f}" if "controlF1" in record else "")
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("corpus", type=Path)
    parser.add_argument("--require", default="", help="comma-separated readers that must run")
    parser.add_argument("--report", type=Path, help="write per-case measurements as JSON")
    args = parser.parse_args()

    manifest_path = args.corpus / "manifest.json"
    if not manifest_path.is_file():
        # The Rust test writes it last; its absence means the test did not run
        # (renamed, filtered out) or failed before finishing.
        print(f"{manifest_path} is missing: the corpus was not written", file=sys.stderr)
        return 1
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    dpi = float(manifest["dpi"])
    cases = manifest["cases"]
    if not cases:
        print("the corpus has no cases", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory() as scratch_dir:
        scratch = Path(scratch_dir)
        readers = {}
        if poppler_available():
            readers["poppler"] = lambda pdf: (
                poppler_text(pdf),
                poppler_fonts(pdf),
                poppler_raster(pdf, dpi, scratch),
            )
        pymupdf = mupdf_module()
        if pymupdf is not None:
            readers["mupdf"] = lambda pdf: mupdf_read(pymupdf, pdf, dpi)

        required = {name for name in args.require.split(",") if name}
        missing = required - readers.keys()
        if missing:
            print(f"required reader(s) unavailable: {sorted(missing)}", file=sys.stderr)
            return 1
        if not readers:
            print("no PDF reader available (poppler-utils, pymupdf)", file=sys.stderr)
            return 1

        failures: list[str] = []
        results: list[dict] = []
        for case in cases:
            check_case(case, args.corpus, readers, dpi, scratch, failures, results)

    if args.report:
        args.report.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    if failures:
        print(f"\n{len(failures)} failure(s):", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1
    print(f"\n{len(cases)} cases x {len(readers)} readers ({', '.join(readers)}): ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
