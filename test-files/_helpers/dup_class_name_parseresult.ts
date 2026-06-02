// Issue #26 / #321: models effect's `ParseResult.Type` — a class named `Type`
// whose fields (`_tag, ast, actual, message`) are DISTINCT from the same-named
// `Type` in `dup_class_name_schemaast.ts` (which has `type, annotations`).
// Param-properties are written out explicitly so `node --experimental-strip-types`
// can run this file (strip mode rejects TS parameter properties).
export class Type {
  _tag: string;
  ast: unknown;
  actual: unknown;
  message?: string;
  constructor(ast: unknown, actual: unknown, message?: string) {
    this._tag = "Type";
    this.ast = ast;
    this.actual = actual;
    this.message = message;
  }
}
