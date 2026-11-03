// DECISIONS 212: a published class method that leaves its module as a value
// (a result, a field, an array element, the prototype, an instance, a
// published literal, an instance field) runs on a receiver of another
// layout. Per-unit facts proved `this.raw` on the class alone and read the
// literal's `pad` slot (MBW2, qb9 /root/MBW2/tmp/i2x).
import { Ret, Fld, Arr, Proto, Inst, Lit, box, St } from "./_helpers/escaped_method_this/a.ts";

const other = (raw: string) => ({ pad: 0, raw });

const r = new Ret();
console.log("ret", r.get(), Ret.box().get.call(other("r")));

const f = new Fld();
const o: any = {};
Fld.put(o);
console.log("fld", f.get(), o.get.call(other("f")));

const a = new Arr();
const ag: any = Arr.arr()[0];
console.log("arr", a.get(), ag.call(other("a")));

const p = new Proto();
const pg: any = Proto.prototype.get;
console.log("proto", p.get(), pg.call(other("p")));

const i = new Inst();
const ig: any = i.get;
console.log("inst", i.get(), ig.call(other("i")));

const l = new Lit();
console.log("lit", l.get(), box.get.call(other("l")));

const s = new St();
console.log("st", s.get(), s.fn.call(other("s")));
