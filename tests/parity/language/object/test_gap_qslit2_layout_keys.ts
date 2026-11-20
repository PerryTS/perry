// Completed ConstFn layouts retain ordinary property semantics for long keys,
// numeric slots, Unicode keys and slots added after birth.
function born(value: string) {
  return {
    abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_long_layout_key: () => value,
    "grüße": () => value + "!",
    fraction: 1.25,
    zero: -0,
    whole: 17,
  };
}
let total = 0;
for (let i = 0; i < 100; i++) {
  const o = born("v" + i);
  (o as any).later = { f: () => i };
  const f = o.abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_long_layout_key;
  total += f().length + (o as any).later.f();
  if (i === 99) {
    console.log(f(), o["grüße"](), o.fraction, Object.is(o.zero, -0), o.whole);
    console.log(Object.keys(o).join(","));
    const d = Object.getOwnPropertyDescriptor(o, "grüße")!;
    console.log(d.writable, d.enumerable, d.configurable);
    delete (o as any)["grüße"];
    (o as any)["grüße"] = () => "again";
    console.log(o["grüße"](), Object.keys(o).join(","));
  }
}
console.log(total);
