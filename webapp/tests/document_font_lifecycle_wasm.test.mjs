// The HF-292 lifecycle on REAL engines: the built `webapp/pkg`, driven through
// the real `font_provisioning.mjs`, document after document. `109` HF-294.
//
// `document_font_lifecycle.test.mjs` proves the programme's bookkeeping against
// engines that only record calls. That cannot show the thing a reader sees: that
// the second document's own engine — which starts with none of the first one's
// faces — ends up drawing the same page and writing the same PDF as the first.
// This does, with the wasm the browser runs (no system fonts, no bundled
// Roboto/Noto: `external-web-fonts`), so "registered" here means a face really
// reached a shaper, not that a fake counted a call.
//
// The CJK buckets are served the committed OFL test subsets
// (`fixtures/fonts/README.md`) in place of the 16 MB production files, so each
// sentence below uses only scalars its subset covers; the named faces are the
// committed production files themselves. Every check is held against a
// control — the same bytes opened and never provisioned — so an assertion that
// could not tell a face from its absence would fail here rather than pass.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { createFontProvisioning } from "../src/font_provisioning.mjs";
import { NAMED_WEB_FONT_FACES, SCRIPT_FALLBACK_FONTS } from "../src/web_fonts.mjs";

const PKG = new URL("../pkg/", import.meta.url);
const STALE =
  "Run `./webapp/build.sh` first — this drives the module that build produces, and " +
  "`webapp/pkg` is not committed.";
const fixture = (name) =>
  readFileSync(fileURLToPath(new URL(`../../fixtures/fonts/${name}`, import.meta.url)));

/** Bucket → the committed subset served in place of the production face. */
const SUBSETS = {
  kr: fixture("NotoSansCJKkr-TestSubset.otf"),
  jp: fixture("NotoSansCJKjp-TestSubset.otf"),
  sc: fixture("NotoSansCJK-TestSubset.otf"),
};

/** One sentence per CJK bucket, each wholly inside its subset's coverage. */
const CASES = [
  { bucket: "kr", postscript: "NotoSansCJKkr-Regular", text: "대한민국 국세청 한글 문서 내보내기 검사" },
  { bucket: "jp", postscript: "NotoSansCJKjp-Regular", text: "日本語の文書をエクスポートする試験" },
  { bucket: "sc", postscript: "NotoSansCJKsc-Regular", text: "中文 文字 中国" },
];

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const flush = () => new Promise((resolve) => setImmediate(resolve));

let wasm;
async function engineModule() {
  if (wasm) return wasm;
  const binary = fileURLToPath(new URL("casual_doc_wasm_bg.wasm", PKG));
  assert.ok(existsSync(binary), `webapp/pkg is missing. ${STALE}`);
  wasm = await import(new URL("casual_doc_wasm.js", PKG).href);
  await wasm.default({ module_or_path: readFileSync(binary) });
  return wasm;
}

/** `url → bytes` the way the host fetches: named faces from the committed
 *  assets, CJK buckets as their subsets. Anything else is a request this test
 *  did not expect, and fails. */
function fetcher() {
  const byUrl = new Map();
  for (const face of NAMED_WEB_FONT_FACES) {
    byUrl.set(face.url, () => readFileSync(fileURLToPath(face.url)));
  }
  for (const [key, bytes] of Object.entries(SUBSETS)) {
    byUrl.set(SCRIPT_FALLBACK_FONTS[key].url, () => bytes);
  }
  const requested = [];
  return {
    requested,
    fetchBytes(url) {
      requested.push(url);
      const read = byUrl.get(url);
      if (!read) return Promise.reject(new Error(`unexpected font request: ${url}`));
      return Promise.resolve(new Uint8Array(read()));
    },
  };
}

/** `main.js`'s font host, reduced to its engine slot. */
function host() {
  let doc = null;
  const net = fetcher();
  const provisioning = createFontProvisioning({
    engine: () => doc,
    fetchBytes: (url) => net.fetchBytes(url),
    releaseBytes() {},
    progress() {},
    log() {},
  });
  return {
    net,
    provisioning,
    get doc() {
      return doc;
    },
    /** `openBytes`: retire, free, swap. */
    adopt(next) {
      if (doc) {
        provisioning.retire(doc);
        doc.free();
      }
      doc = next;
    },
  };
}

/** What a reader gets from `doc`: page 1's pixels and its PDF. */
function observe(doc) {
  const page = doc.renderPage(0, 48);
  const pixels = sha(page.rgba);
  page.free();
  const artifact = doc.exportAs("application.pdf", "semantic");
  const pdf = Buffer.from(artifact.bytes);
  const report = artifact.reportJson;
  artifact.free();
  return {
    pixels,
    pdf: sha(pdf),
    embeds: (postscript) => pdf.includes(Buffer.from(`+${postscript}`)),
    notdef: report.includes('"pdf.font.missing_glyphs"'),
    report,
  };
}

for (const { bucket, postscript, text } of CASES) {
  test(`${bucket}: a second engine opening the same document is given its own face`, async () => {
    const { open } = await engineModule();
    const bytes = new TextEncoder().encode(text);

    // The control: the same bytes, never provisioned.
    const bare = open(bytes);
    const control = observe(bare);
    bare.free();
    assert.ok(control.notdef, `${bucket}: unprovisioned must draw .notdef and say so`);
    assert.ok(!control.embeds(postscript), `${bucket}: nothing registered`);

    const h = host();
    h.adopt(open(bytes));
    assert.equal(await h.provisioning.provisionOnOpen("A"), null, "A reports no loss");
    const a = observe(h.doc);
    assert.ok(a.embeds(postscript), `${bucket}: A's PDF embeds ${postscript}`);
    assert.ok(!a.notdef, `${bucket}: A draws no .notdef: ${a.report}`);
    assert.notEqual(a.pixels, control.pixels, `${bucket}: the face changed A's page`);
    const bucketUrl = SCRIPT_FALLBACK_FONTS[bucket].url;
    assert.equal(h.net.requested.filter((url) => url === bucketUrl).length, 1);

    // B: same bytes, same name, a new engine — the open that painted ▯.
    h.adopt(open(bytes));
    assert.equal(await h.provisioning.provisionOnOpen("A"), null, "B reports no loss");
    await flush();
    const b = observe(h.doc);
    assert.ok(!b.notdef, `${bucket}: B draws no .notdef: ${b.report}`);
    assert.equal(b.pixels, a.pixels, `${bucket}: B's page is A's`);
    assert.equal(b.pdf, a.pdf, `${bucket}: B's PDF is A's`);
    assert.equal(
      h.net.requested.filter((url) => url === bucketUrl).length,
      2,
      `${bucket}: B's engine must be given the face again, not told it has it`,
    );
    h.adopt(null);
  });
}

test("a face that lands after its document was replaced reaches neither engine", async () => {
  const { open } = await engineModule();
  const korean = new TextEncoder().encode(CASES[0].text);
  const h = host();
  let release;
  const held = new Promise((resolve) => {
    release = resolve;
  });
  const fetchBytes = h.net.fetchBytes.bind(h.net);
  h.net.fetchBytes = (url) =>
    url === SCRIPT_FALLBACK_FONTS.kr.url ? held.then(() => fetchBytes(url)) : fetchBytes(url);

  h.adopt(open(korean));
  const pass = h.provisioning.provisionOnOpen("A");
  await flush();
  // Replaced (and freed) while its Korean download is in flight. A register into
  // the freed engine throws inside wasm-bindgen; into the new one would give B a
  // face its own pass never asked for.
  const bare = open(korean);
  h.adopt(bare);
  release();
  assert.equal(await pass, null, "a retired pass reports nothing");
  const b = observe(bare);
  assert.ok(b.notdef, "B was never provisioned, so it must still draw .notdef");
  assert.ok(!b.embeds(CASES[0].postscript));
  h.adopt(null);
});
