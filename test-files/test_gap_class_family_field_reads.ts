// A family word fixes slot locations, but each read still proves presence.
// Recursive readers retain a typed receiver without fresh-local slot proofs.
"use strict";
class FamilyFields {
    constructor(a: any, b: any) {
        this.a = a;
        this.b = b;
    }
    setLate(value: any): void { this.late = value; }
    read(n: number): any {
        if (n) return this.read(n - 1);
        return this.a + ":" + this.b;
    }
}
class FamilyChild extends FamilyFields {
    constructor(a: any, b: any, c: any) {
        super(a, b);
        this.c = c;
    }
}
function a(o: FamilyFields, n: number): any {
    if (n) return a(o, n - 1);
    return o.a;
}
function b(o: FamilyFields, n: number): any {
    if (n) return b(o, n - 1);
    return o.b;
}
function late(o: FamilyFields, n: number): any {
    if (n) return late(o, n - 1);
    return o.late;
}
function add(o: FamilyFields, value: any): void { o.setLate(value); }
const one = new FamilyFields(11, 22);
console.log("birth", a(one, 1), b(one, 1), one.read(1));
console.log("hole", late(one, 1), "late" in one);
add(one, "added");
console.log("added", late(one, 1), "late" in one);
add(one, undefined);
console.log("present-undefined", late(one, 1), Object.hasOwn(one, "late"));
delete (one as any).a;
console.log("deleted", a(one, 1), b(one, 1));
(one as any).a = "again";
console.log("readded", a(one, 1), b(one, 1));
const child = new FamilyChild("A", "B", "C");
console.log("subclass", a(child, 1), b(child, 1), child.c, child.read(1));
const frozen = new FamilyFields("frozen-a", "frozen-b");
Object.freeze(frozen);
console.log("frozen", a(frozen, 1), b(frozen, 1), frozen.read(1));
try { (frozen as any).a = "bad"; } catch (e) { console.log("frozen-write", e instanceof TypeError); }
console.log("still-frozen", a(frozen, 1));
const descriptor = new FamilyFields("old", "data");
Object.defineProperty(descriptor, "a", { get() { return "getter"; }, configurable: true });
console.log("descriptor", a(descriptor, 1), b(descriptor, 1));
const changed = new FamilyFields(1, 2);
(changed as any).a = "string";
console.log("boxed", a(changed, 1), b(changed, 1));
const fake: any = { b: "reverse-b", a: "reverse-a" };
console.log("other-layout", a(fake, 1), b(fake, 1));
console.log("nullish");
try { a(null as any, 1); } catch (e) { console.log(e instanceof TypeError); }

// A not-yet-owned slot must resolve through the prototype on a hole read.
class DuringBirth {
    constructor(value: any) {
        this.first = observe(this, 1);
        this.last = value;
    }
}
function observe(o: DuringBirth, n: number): any {
    if (n) return observe(o, n - 1);
    return o.last;
}
Object.defineProperty(DuringBirth.prototype, "last", { value: "inherited", writable: true, configurable: true });
const during = new DuringBirth("own");
console.log("during", during.first, observe(during, 1));

let receiverEvaluations = 0;
function receiver(): FamilyFields { receiverEvaluations++; return child; }
console.log("receiver-once", receiver().b, receiverEvaluations);
function scale(o: FamilyFields, n: number): number {
    if (n) return scale(o, n - 1);
    return o.a * 2;
}
console.log("numeric-context", scale(new FamilyFields("7", 0), 1), scale(new FamilyFields(true, 0), 1));

// Creation variants defined in another module need their own descriptor
// declarations and can put the same late key at different family slots.
import { VariantFields, makeX, makeY } from "./_helpers/awc_family_fields.ts";
function variantRead(o: VariantFields, n: number): string {
    if (n) return variantRead(o, n - 1);
    return o.value + ":" + o.x + ":" + o.y;
}
const vx = makeX();
const vy = makeY();
console.log("variants", variantRead(vx, 1), variantRead(vy, 1));
(vx as any).y = "filled-y";
console.log("variant-add", variantRead(vx, 1));
delete (vy as any).y;
console.log("variant-delete", variantRead(vy, 1));
