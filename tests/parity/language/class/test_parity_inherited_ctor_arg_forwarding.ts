abstract class Base {
  _def: any;
  constructor(def: any) { this._def = def; }
  abstract _p(): any;
}
class Num extends Base {
  _p() { return 1; }
  addCheck(c: any): Num { return new Num({ ...this._def, checks: [...this._def.checks, c] }); }
  static create = (): Num => new Num({ checks: [], tn: "N" });
}

const a: any = Num.create();
if (JSON.stringify(a._def) !== '{"checks":[],"tn":"N"}') throw new Error("create _def: " + JSON.stringify(a._def));
const b: any = a.addCheck({ kind: "int" });
if (JSON.stringify(b._def.checks) !== '[{"kind":"int"}]') throw new Error("addCheck _def: " + JSON.stringify(b._def));
const c: any = a.addCheck({ kind: "int" }).addCheck({ kind: "min" });
if (c._def.checks.length !== 2) throw new Error("chain: " + JSON.stringify(c._def.checks));
console.log("OK");
