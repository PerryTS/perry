declare function js_pdf_parse(buf: Uint8Array): string;

/**
 * Extract text from a PDF buffer.
 */
export function parse(buf: Uint8Array): string {
  return js_pdf_parse(buf);
}
