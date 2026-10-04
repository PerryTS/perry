// parity-env: PERRY_LL_RS4GC_MAX_INSTRS=1000 PERRY_GC_FORCE_EVACUATE=1
// #11836: a module init whose GC roots move to a shadow frame only after
// lowering (the post-RS4GC budget retry, forced here by the tiny budget)
// must still root its entry-hoisted class-keys caches. The frame push has
// to run before their binds. When it ran after them, the binds rooted
// nothing, an evacuating collection left every cache naming its keys
// array at the from-space address, and the next `new C()` published a
// shape over that dead copy (prettier's typescript plugin segfaulted).
declare function gc(): void;
class C0 { k0a: number = 0; k0b: string = "v0"; }
class C1 { k1a: number = 1; k1b: string = "v1"; }
class C2 { k2a: number = 2; k2b: string = "v2"; }
class C3 { k3a: number = 3; k3b: string = "v3"; }
class C4 { k4a: number = 4; k4b: string = "v4"; }
class C5 { k5a: number = 5; k5b: string = "v5"; }
class C6 { k6a: number = 6; k6b: string = "v6"; }
class C7 { k7a: number = 7; k7b: string = "v7"; }
class C8 { k8a: number = 8; k8b: string = "v8"; }
class C9 { k9a: number = 9; k9b: string = "v9"; }
class C10 { k10a: number = 10; k10b: string = "v10"; }
class C11 { k11a: number = 11; k11b: string = "v11"; }
class C12 { k12a: number = 12; k12b: string = "v12"; }
class C13 { k13a: number = 13; k13b: string = "v13"; }
class C14 { k14a: number = 14; k14b: string = "v14"; }
class C15 { k15a: number = 15; k15b: string = "v15"; }
class C16 { k16a: number = 16; k16b: string = "v16"; }
class C17 { k17a: number = 17; k17b: string = "v17"; }
class C18 { k18a: number = 18; k18b: string = "v18"; }
class C19 { k19a: number = 19; k19b: string = "v19"; }
class C20 { k20a: number = 20; k20b: string = "v20"; }
class C21 { k21a: number = 21; k21b: string = "v21"; }
class C22 { k22a: number = 22; k22b: string = "v22"; }
class C23 { k23a: number = 23; k23b: string = "v23"; }
const warm: string[] = [];
for (let i = 0; i < 1000; i++) warm.push("w" + i);
if (typeof gc === "function") gc();
const out: string[] = [];
const o0: any = new C0();
out.push(Object.keys(o0).join(",") + "=" + o0.k0a + o0.k0b + String(o0.missing));
const o1: any = new C1();
out.push(Object.keys(o1).join(",") + "=" + o1.k1a + o1.k1b + String(o1.missing));
const o2: any = new C2();
out.push(Object.keys(o2).join(",") + "=" + o2.k2a + o2.k2b + String(o2.missing));
const o3: any = new C3();
out.push(Object.keys(o3).join(",") + "=" + o3.k3a + o3.k3b + String(o3.missing));
const o4: any = new C4();
out.push(Object.keys(o4).join(",") + "=" + o4.k4a + o4.k4b + String(o4.missing));
const o5: any = new C5();
out.push(Object.keys(o5).join(",") + "=" + o5.k5a + o5.k5b + String(o5.missing));
const o6: any = new C6();
out.push(Object.keys(o6).join(",") + "=" + o6.k6a + o6.k6b + String(o6.missing));
const o7: any = new C7();
out.push(Object.keys(o7).join(",") + "=" + o7.k7a + o7.k7b + String(o7.missing));
const o8: any = new C8();
out.push(Object.keys(o8).join(",") + "=" + o8.k8a + o8.k8b + String(o8.missing));
const o9: any = new C9();
out.push(Object.keys(o9).join(",") + "=" + o9.k9a + o9.k9b + String(o9.missing));
const o10: any = new C10();
out.push(Object.keys(o10).join(",") + "=" + o10.k10a + o10.k10b + String(o10.missing));
const o11: any = new C11();
out.push(Object.keys(o11).join(",") + "=" + o11.k11a + o11.k11b + String(o11.missing));
const o12: any = new C12();
out.push(Object.keys(o12).join(",") + "=" + o12.k12a + o12.k12b + String(o12.missing));
const o13: any = new C13();
out.push(Object.keys(o13).join(",") + "=" + o13.k13a + o13.k13b + String(o13.missing));
const o14: any = new C14();
out.push(Object.keys(o14).join(",") + "=" + o14.k14a + o14.k14b + String(o14.missing));
const o15: any = new C15();
out.push(Object.keys(o15).join(",") + "=" + o15.k15a + o15.k15b + String(o15.missing));
const o16: any = new C16();
out.push(Object.keys(o16).join(",") + "=" + o16.k16a + o16.k16b + String(o16.missing));
const o17: any = new C17();
out.push(Object.keys(o17).join(",") + "=" + o17.k17a + o17.k17b + String(o17.missing));
const o18: any = new C18();
out.push(Object.keys(o18).join(",") + "=" + o18.k18a + o18.k18b + String(o18.missing));
const o19: any = new C19();
out.push(Object.keys(o19).join(",") + "=" + o19.k19a + o19.k19b + String(o19.missing));
const o20: any = new C20();
out.push(Object.keys(o20).join(",") + "=" + o20.k20a + o20.k20b + String(o20.missing));
const o21: any = new C21();
out.push(Object.keys(o21).join(",") + "=" + o21.k21a + o21.k21b + String(o21.missing));
const o22: any = new C22();
out.push(Object.keys(o22).join(",") + "=" + o22.k22a + o22.k22b + String(o22.missing));
const o23: any = new C23();
out.push(Object.keys(o23).join(",") + "=" + o23.k23a + o23.k23b + String(o23.missing));
console.log(out.join("\n"));
console.log(warm.length);
