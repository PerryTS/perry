// Helper for test_gap_static_member_call.ts (#1787 / #321).
// A class exposing callable STATIC FIELDS (arrow + function expression) and a
// static METHOD, plus a subclass that inherits them — mirrors effect's
// `SchemaAST.Union` (`static make = (...) => ...`, `static unify = ...`).

export class Factory {
  static tag = "F";
  static make = (n: number): number => n * 2;
  static makeFn = function (n: number): number {
    return n + 100;
  };
  static label(n: number): string {
    return Factory.tag + ":" + n;
  }
}

export class SubFactory extends Factory {
  static extra = (n: number): number => n - 3;
}
