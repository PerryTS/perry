// Type declarations for `@perryts/pdf` — Perry's official PDF
// creation binding (issue #516).
//
// The companion read/render side ships in the PdfView widget
// (`perry/ui`), already available on iOS / visionOS / macOS with
// stubs on the other platform crates. This module is the producer
// side: build PDFs at runtime, save them to disk, hand them off to
// PdfView for display (or to the OS share sheet, email, etc.).
//
// All coordinates are in PDF points (1 pt = 1/72 inch). The origin
// is the bottom-left corner — that is the PDF native coordinate
// system, NOT a Perry convention. The default page is US Letter
// (612 × 792 pt).
//
// Underlying engine: the pure-Rust `printpdf` crate. v1 of the
// surface intentionally exposes only what fits in five FFI calls;
// images, custom fonts, encryption, forms, and annotations beyond
// text + straight lines are deliberate out-of-scope and tracked as
// follow-ups under #516.

/**
 * Options for [`createPdf`].
 */
export interface CreatePdfOptions {
  /**
   * Filesystem path where [`pdfSave`] will write the PDF. Captured
   * at `createPdf` time so the save call doesn't have to re-thread
   * it.
   */
  path: string;
  /** Page width in PDF points. Defaults to 612 (US Letter portrait). */
  pageWidth?: number;
  /** Page height in PDF points. Defaults to 792 (US Letter portrait). */
  pageHeight?: number;
}

/**
 * Start a new in-progress PDF document. Returns an opaque handle
 * that the other functions in this module accept as their first
 * argument. The handle is freed by [`pdfSave`]; subsequent calls on
 * a saved handle are silently no-op (warn-once on stderr).
 *
 * @example
 *   const pdf = createPdf({ path: "out.pdf" });
 *   pdfAddText(pdf, "Hello, world!", 72, 720, 14);
 *   pdfSave(pdf);
 */
export declare function createPdf(opts: CreatePdfOptions): number;

/**
 * Draw `text` at `(x, y)` in Helvetica. `fontSize` is in points;
 * defaults to 12 when omitted or non-positive. Multiple calls on
 * the same page stack without resetting state — each call emits a
 * self-contained `BT … ET` PDF text block.
 */
export declare function pdfAddText(
  pdf: number,
  text: string,
  x: number,
  y: number,
  fontSize?: number,
): void;

/**
 * Draw a straight black 1 pt line from `(x1, y1)` to `(x2, y2)`.
 * Coordinates: bottom-left origin, PDF points.
 */
export declare function pdfAddLine(
  pdf: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): void;

/**
 * Finalize the current page and start a fresh one with the same
 * dimensions. No-op when the current page has no drawing ops yet,
 * so it's safe to call after [`createPdf`] without emitting a
 * leading blank page.
 */
export declare function pdfNewPage(pdf: number): void;

/**
 * Flush the current page, serialize the document, and write it to
 * the `path` passed to [`createPdf`]. Drops the handle from the
 * internal handle table; subsequent calls with this handle become
 * no-ops.
 *
 * Failure modes (lock poisoning, I/O error, serialize warnings) are
 * logged to stderr but not thrown — the surface returns `void`.
 */
export declare function pdfSave(pdf: number): void;
