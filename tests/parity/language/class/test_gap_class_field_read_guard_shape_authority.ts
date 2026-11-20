// The class-field READ guard (`emit_class_field_read_precheck`) compares the
// receiver's ShapeId against the class's expectation and, for a `number`
// field read as a raw double, also the class id and the per-object
// typed-layout intact bit. The GcHeader predicates it used to test (GC kind,
// forwarded, descriptor flag, tombstone flag) are carried by the ShapeId.
//
// Every reader below takes a recursion count so it stays a real call (an
// inlined read of a locally-born receiver takes the statically proven route
// instead, and would not test the guard at all). Each scenario is a receiver
// on which one of those facts DIFFERS from a plain instance; each must answer
// what node answers.

class Pt {
    x: number;
    y: number;
    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }
    getX(n: number = 0): number {
        if (n > 0) return this.getX(n - 1);
        return this.x;
    }
}

class Tag {
    name: any;
    size: any;
    constructor(name: any, size: any) {
        this.name = name;
        this.size = size;
    }
    getName(n: number = 0): any {
        if (n > 0) return this.getName(n - 1);
        return this.name;
    }
}

// Same key list as Pt / Tag, different classes: they share the class's
// key-list ShapeId.
class SamePt {
    x: number;
    y: number;
    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }
}
class BoolPt {
    x: boolean;
    y: number;
    constructor(x: boolean, y: number) {
        this.x = x;
        this.y = y;
    }
}
class SubPt extends Pt {
    constructor() {
        super(50, 60);
    }
}

function readX(p: Pt, n: number = 0): number {
    if (n > 0) return readX(p, n - 1);
    return p.x;
}
function readX2(p: Pt, n: number = 0): number {
    if (n > 0) return readX2(p, n - 1);
    return p.x * 2 + (p.x < 5 ? 100 : 0);
}
function readName(t: Tag, n: number = 0): any {
    if (n > 0) return readName(t, n - 1);
    return t.name;
}

// Warm every site on genuine instances first.
const warm: Pt[] = [];
for (let i = 0; i < 64; i++) warm.push(new Pt(i, 1));
let w = 0;
for (let r = 0; r < 50; r++) for (let i = 0; i < 64; i++) w += readX(warm[i]) + warm[i].getX();
console.log("warm:" + w);
const t0 = new Tag("t0", 1);
console.log("warm-tag:" + readName(t0) + t0.getName());

// 1. SAME KEY LIST, OTHER CLASS. The ShapeId matches; the class id does not.
//    A boxed read needs no class id (the key list fixes the slot); a raw-f64
//    read keeps the class id, so these take the miss path and still answer
//    the right value.
console.log("same-shape:" + readX(new SamePt(3, 4) as any) + "," + readX(new SubPt()));
console.log("same-shape-bool:" + readX(new BoolPt(true, 2) as any) + "," + readX2(new BoolPt(true, 2) as any));
const lookalike: any = { x: "not-a-number", y: 2 };
console.log("lit:" + readX(lookalike) + "," + typeof readX(lookalike) + "," + Pt.prototype.getX.call(lookalike));
console.log("boxed-lit:" + readName({ name: "lit-name", size: 3 } as any));

// 2. DOWNGRADED LAYOUT. Storing a string into a `number` field clears the
//    per-object intact bit WITHOUT a shape transition.
const down = new Pt(1, 2);
(down as any).x = "downgraded";
console.log("down:" + readX(down) + "," + down.getX() + "," + typeof readX(down) + "," + readX2(down));
(down as any).x = 9;
console.log("repromoted:" + readX(down) + "," + readX2(down));

// 3. INSTANCE ACCESSOR (rule 1: a descriptor install transitions the shape).
const acc = new Pt(3, 4);
let gets = 0;
Object.defineProperty(acc, "x", {
    get() {
        gets++;
        return 1000;
    },
    configurable: true,
});
console.log("acc:" + readX(acc) + "," + acc.getX() + "," + gets);
const tacc = new Tag("plain", 1);
Object.defineProperty(tacc, "name", { get: () => "from-getter", configurable: true });
console.log("acc-boxed:" + readName(tacc) + "," + tacc.getName());

// 4. NON-DEFAULT ATTRIBUTES without an accessor.
const ro = new Pt(7, 8);
Object.defineProperty(ro, "x", { value: 77, writable: false });
console.log("ro:" + readX(ro) + "," + ro.getX());
const tro = new Tag("ro", 1);
Object.defineProperty(tro, "name", { value: "ro-value", writable: false });
console.log("ro-boxed:" + readName(tro));

// 5. DELETE (#10826: a delete is a shape transition; no tombstone survives a
//    shape hit).
const del = new Pt(11, 12);
delete (del as any).x;
console.log("del:" + readX(del) + "," + del.getX());
const tdel = new Tag("gone", 2);
delete (tdel as any).name;
console.log("del-boxed:" + readName(tdel) + "," + tdel.getName());
(tdel as any).name = "back";
console.log("readd-boxed:" + readName(tdel));

// 6. NOT A HEAP OBJECT. The receiver range check (one biased compare) must
//    send every non-pointer value, and a pointer-tagged handle-band value, to
//    the miss path, which reads them by name or throws.
const odd: any[] = [42, "abc", true, 1.5, -0, Symbol.iterator];
let oddOut = "";
for (let i = 0; i < odd.length; i++) oddOut += String(readName(odd[i])) + "|";
console.log("non-object:" + oddOut);
for (const nullish of [null, undefined]) {
    try {
        readName(nullish as any);
        console.log("nullish: no throw");
    } catch (e) {
        console.log("nullish:" + (e instanceof TypeError));
    }
}

// 7. PROTOTYPE ACCESSOR (poisons the guard expectation process-wide). The
//    own field still shadows it. Must stay LAST: it closes the inline path.
Object.defineProperty(Pt.prototype, "y", { get: () => -1, configurable: true });
const after = new Pt(21, 22);
console.log("proto-acc:" + readX(after) + "," + after.getX() + "," + after.y);
let s = 0;
for (let i = 0; i < 64; i++) s += readX(warm[i]);
console.log("final-sum:" + s);
