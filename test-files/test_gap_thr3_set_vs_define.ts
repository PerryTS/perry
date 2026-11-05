// THR3: one key defined in one branch and stored in the other; and a key
// stored by a method before the constructor's definition (the [[Set]] add
// of a key the layout carries with attributes is a plain property).
class SV {
  constructor(flag: boolean) {
    if (flag) Object.defineProperty(this, "p", { value: 1 });
    else (this as any).p = 2;
  }
}
class Pre {
  constructor(early: boolean) {
    if (early) this.init();
    Object.defineProperty(this, "q", { value: 1 });
  }
  init(): void {
    (this as any).q = 5;
  }
}
for (const f of [true, false]) {
  console.log(f, JSON.stringify(Object.getOwnPropertyDescriptor(new SV(f), "p")));
}
for (const e of [false, true]) {
  try {
    const o: any = new Pre(e);
    console.log(e, JSON.stringify(Object.getOwnPropertyDescriptor(o, "q")), Object.keys(o).join(","));
  } catch (err) {
    console.log(e, (err as Error).constructor.name);
  }
}
