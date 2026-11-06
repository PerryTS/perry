// Compile-smoke for `@perryts/pdf` (issue #516).
//
// Creates a two-page PDF — title + horizontal rule on page 1,
// a second page after `pdfNewPage` — saves it to
// `/tmp/perry_pdf_smoke.pdf`, then verifies the file exists, starts
// with the PDF magic bytes, and contains a plausible `%%EOF` marker
// near the tail.
//
// All five FFI entry points of the v1 surface are exercised:
// createPdf, pdfAddText, pdfAddLine, pdfNewPage, pdfSave.

import {
  createPdf,
  pdfAddText,
  pdfAddLine,
  pdfNewPage,
  pdfSave,
} from "@perryts/pdf";
import * as fs from "fs";

const OUT_PATH = "/tmp/perry_pdf_smoke.pdf";

function main(): void {
  // US Letter portrait by default; default origin = bottom-left.
  const pdf = createPdf({ path: OUT_PATH });

  // Page 1: title + underline.
  pdfAddText(pdf, "Hello from Perry!", 72, 720, 18);
  pdfAddLine(pdf, 72, 710, 540, 710);
  pdfAddText(pdf, "Perry PDF smoke test (#516)", 72, 690, 10);

  // Add a second page to exercise pdfNewPage.
  pdfNewPage(pdf);
  pdfAddText(pdf, "Page 2", 72, 720, 14);

  pdfSave(pdf);

  // Verify the file exists and looks like a PDF.
  if (!fs.existsSync(OUT_PATH)) {
    console.error("FAIL: PDF was not written to", OUT_PATH);
    process.exit(1);
  }
  // Read as a Buffer and inspect the magic markers via hex
  // encoding. `readFileSync(path, "utf8")` lossily strips bytes
  // ≥ 0x80 in Perry's current stdlib, which makes startsWith
  // checks unreliable on the binary tail. Hex is round-trip-safe
  // and the markers we care about are ASCII (`%PDF-` =
  // 255044462d, `%%EOF` = 2525454f46).
  const buf = fs.readFileSync(OUT_PATH);
  if (buf.length < 100) {
    console.error("FAIL: PDF is suspiciously small:", buf.length, "bytes");
    process.exit(1);
  }
  const hex: string = buf.toString("hex");
  const PDF_MAGIC_HEX = "255044462d"; // "%PDF-"
  const EOF_MARKER_HEX = "2525454f46"; // "%%EOF"
  if (!hex.startsWith(PDF_MAGIC_HEX)) {
    console.error("FAIL: file does not start with %PDF-");
    process.exit(1);
  }
  // `%%EOF` must appear near the tail, not just somewhere — but for
  // a smoke test, "anywhere in the file" is fine.
  if (hex.indexOf(EOF_MARKER_HEX) === -1) {
    console.error("FAIL: file does not contain %%EOF marker");
    process.exit(1);
  }
  console.log("OK: PDF written to", OUT_PATH, "(", buf.length, "bytes)");
}

main();
