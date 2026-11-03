// The producer of imported_static_width.ts: static methods narrower than the
// six parameters an importer used to declare them with.
export class Calc {
  static zero() { return 0; }
  static twice(x: number) { return x * 2; }
  static add(a: number, b: number) { return a + b; }
}
