// Cross-module return-shape producer for `test_gap_repsel_return_shape.ts`
// (#7170 R2). Keep this as an anonymous-record return: its content-addressed
// `__AnonShape_*` class is the identity the importing module can prove without
// relying on a module-local `FuncId`.

export interface CrossModuleRow {
  key: string;
  value: number;
  tag: string;
}

export function makeCrossModuleRow(i: number): CrossModuleRow {
  return { key: "xm" + i, value: i * 3, tag: "cross" };
}
