function show(label: string, value: unknown): void {
  console.log(label + ": " + String(value));
}

function hasOwn(obj: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

const u8: any = new Uint8Array([10, 20, 30]);
show("u8 literal read", u8[1.5] === undefined);
u8[1.5] = 99;
show("u8 literal element preserved", u8[1] === 20);
show("u8 literal no own", !hasOwn(u8, "1.5"));

const u8Key = 1.5;
u8[u8Key] = 77;
show("u8 variable read", u8[u8Key] === undefined);
show("u8 variable element preserved", u8[1] === 20);
show("u8 variable no own", !hasOwn(u8, "1.5"));

const f64: any = new Float64Array([1.25, 2.5, 3.75]);
show("f64 literal read", f64[1.5] === undefined);
f64[1.5] = 42;
show("f64 literal element preserved", f64[1] === 2.5);
show("f64 literal no own", !hasOwn(f64, "1.5"));

const f64Key = 1.5;
f64[f64Key] = 11;
show("f64 variable read", f64[f64Key] === undefined);
show("f64 variable element preserved", f64[1] === 2.5);
show("f64 variable no own", !hasOwn(f64, "1.5"));
