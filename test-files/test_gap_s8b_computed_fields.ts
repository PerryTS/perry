// S8B: class fields with computed keys naming string constants join families.
import { TypeId, OtherId, SymId } from "./_helpers/s8b_computed_fields/ids.ts";
import * as Ids from "./_helpers/s8b_computed_fields/ids.ts";
const LocalId = "~s8b/Local";
const AliasId = Ids.OtherId;
const NumKey = "7";
const keys = (o: object) => JSON.stringify(Reflect.ownKeys(o).map(String));
class Base {
  readonly [TypeId] = TypeId;
  readonly annotations: unknown;
  constructor(a: unknown) { this.annotations = a; }
}
class Lit extends Base {
  readonly _tag = "Lit";
  readonly [LocalId] = 1;
  readonly literal: number;
  constructor(literal: number, a?: unknown) { super(a); this.literal = literal; }
}
class Al {
  [AliasId] = "alias";
  [OtherId]: string;
  [NumKey] = "num";
  [SymId] = "sym";
  x = 1;
  constructor() { (this as any)[OtherId] = "set"; }
}
class Imp {
  readonly [TypeId]: typeof TypeId;
  readonly reasons: number[];
  constructor(r: number[]) { this[TypeId] = TypeId; this.reasons = r; }
}
let s = 0;
let l: any, a: any, m: any;
for (let i = 0; i < 20000; i++) {
  l = new Lit(i, { i });
  a = new Al();
  m = new Imp([i]);
  if (l[TypeId] === TypeId && l[LocalId] === 1 && m[TypeId] === TypeId) s += l.literal;
  if ((a as any)[OtherId] === "set") s++;
}
console.log(s, keys(l), keys(a), keys(m), JSON.stringify(l), JSON.stringify(a), JSON.stringify(m));
const o: any = new Lit(1);
delete o[TypeId];
o[TypeId] = 5;
console.log(keys(o), o[TypeId], TypeId in new Lit(2), Object.getOwnPropertyDescriptor(new Lit(3), TypeId));
