// #10420 / #10425: calls with more than 16 arguments.
//
// A function VALUE called with 17+ arguments failed to compile (`closure call
// with 18 args (max 16)`), and apply/call/spread/object-literal methods passed
// `0` for every parameter past the 16th (the `__perry_wrap_*` value wrappers
// were capped at 16 params). `Reflect.apply` forwarded at most four arguments.
// qs 6.15.3's recursive 18-argument `stringify` is the package-audit shape.
// Sizes 3 and 16 are the fixed-arity fast-path controls; 17/18/32/64 take the
// variadic path.

import {
  sig,
  importedRest,
  importedArrow3,
  importedArrow16,
  importedArrow17,
  importedArrow18,
  importedArrow32,
  importedArrow64,
  importedFexpr3,
  importedFexpr16,
  importedFexpr17,
  importedFexpr18,
  importedFexpr32,
  importedFexpr64,
} from "./_helpers/call_arity_10420.ts";

function range(n: number): number[] {
  const out: number[] = [];
  for (let i = 1; i <= n; i++) out.push(i);
  return out;
}

function show(label: string, value: any): void {
  console.log(label + " " + String(value));
}

// ---- 3 parameters ----
const arrow3 = (
  p0: any, p1: any, p2: any,
): string => sig([
  p0, p1, p2,
]);

var fexpr3 = function fexpr3(
  p0: any, p1: any, p2: any,
): string {
  if (p0 > 1) {
    return fexpr3(
      p0 - 1, p1, p2,
    );
  }
  return sig([
    p0, p1, p2,
  ]);
};

function decl3(
  p0: any, p1: any, p2: any,
): string {
  return arguments.length + "/" + sig([
    p0, p1, p2,
  ]);
}

function Ctor3(
  this: any, p0: any, p1: any, p2: any,
) {
  this.value = sig([
    p0, p1, p2,
  ]);
}

class Klass3 {
  value: string;
  constructor(
    p0: any, p1: any, p2: any,
  ) {
    this.value = sig([
      p0, p1, p2,
    ]);
  }
  method(
    p0: any, p1: any, p2: any,
  ): string {
    return "klass:" + sig([
      p0, p1, p2,
    ]);
  }
}

const objlit3 = {
  tag: "obj",
  method(
    this: any, p0: any, p1: any, p2: any,
  ): string {
    return this.tag + ":" + sig([
      p0, p1, p2,
    ]);
  },
};

{
  const args = range(3);
  const values: any[] = [arrow3, fexpr3, decl3, importedArrow3, importedFexpr3];
  show("arrow3 direct", arrow3(
    1, 2, 3,
  ));
  show("fexpr3 recursive", fexpr3(
    4, 2, 3,
  ));
  show("decl3 direct", decl3(
    1, 2, 3,
  ));
  show("importedArrow3 direct", importedArrow3(
    1, 2, 3,
  ));
  show("importedFexpr3 direct", importedFexpr3(
    1, 2, 3,
  ));
  show("importedRest 3 direct", importedRest(
    1, 2, 3,
  ));
  const rest = (...values: any[]): string => "rest:" + sig(values);
  show("rest 3 direct", rest(
    1, 2, 3,
  ));
  show("rest 3 apply", rest.apply(null, args));
  for (let i = 0; i < values.length; i++) {
    const fn = values[i];
    show("value3[" + i + "] call", fn(
      1, 2, 3,
    ));
    show("value3[" + i + "] apply", fn.apply(null, args));
    show("value3[" + i + "] .call", fn.call(null, ...args));
    show("value3[" + i + "] spread", fn(...args));
    show("value3[" + i + "] Reflect.apply", Reflect.apply(fn, null, args));
    show("value3[" + i + "] bind", fn.bind(null, 1, 2)(...args.slice(2)));
    show("value3[" + i + "] short", fn(1, 2, 3));
  }
  show("decl3 underapplied", decl3.apply(null, args.slice(0, 2)));
  show("decl3 overapplied", decl3.apply(null, range(6)));
  show("Reflect.construct Ctor3", Reflect.construct(Ctor3, args).value);
  show("Reflect.construct Klass3", Reflect.construct(Klass3, args).value);
  show("new Klass3 spread", new Klass3(...args).value);
  const k = new Klass3(...args);
  show("Klass3 method direct", k.method(
    1, 2, 3,
  ));
  show("Klass3 method apply", k.method.apply(k, args));
  show("Klass3 method spread", k.method(...args));
  show("Klass3 method Reflect.apply", Reflect.apply(k.method, k, args));
  show("objlit3 method direct", objlit3.method(
    1, 2, 3,
  ));
  show("objlit3 method apply", objlit3.method.apply(objlit3, args));
  show("objlit3 method .call", objlit3.method.call(objlit3, ...args));
  show("objlit3 method spread", objlit3.method(...args));
  show("objlit3 method Reflect.apply", Reflect.apply(objlit3.method, objlit3, args));
}

// ---- 16 parameters ----
const arrow16 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
): string => sig([
  p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
]);

var fexpr16 = function fexpr16(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
): string {
  if (p0 > 1) {
    return fexpr16(
      p0 - 1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
    );
  }
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
  ]);
};

function decl16(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
): string {
  return arguments.length + "/" + sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
  ]);
}

function Ctor16(
  this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
  p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
) {
  this.value = sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
  ]);
}

class Klass16 {
  value: string;
  constructor(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
  ) {
    this.value = sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
    ]);
  }
  method(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
  ): string {
    return "klass:" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
    ]);
  }
}

const objlit16 = {
  tag: "obj",
  method(
    this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
    p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any,
  ): string {
    return this.tag + ":" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15,
    ]);
  },
};

{
  const args = range(16);
  const values: any[] = [arrow16, fexpr16, decl16, importedArrow16, importedFexpr16];
  show("arrow16 direct", arrow16(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("fexpr16 recursive", fexpr16(
    4, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("decl16 direct", decl16(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("importedArrow16 direct", importedArrow16(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("importedFexpr16 direct", importedFexpr16(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("importedRest 16 direct", importedRest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  const rest = (...values: any[]): string => "rest:" + sig(values);
  show("rest 16 direct", rest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("rest 16 apply", rest.apply(null, args));
  for (let i = 0; i < values.length; i++) {
    const fn = values[i];
    show("value16[" + i + "] call", fn(
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    ));
    show("value16[" + i + "] apply", fn.apply(null, args));
    show("value16[" + i + "] .call", fn.call(null, ...args));
    show("value16[" + i + "] spread", fn(...args));
    show("value16[" + i + "] Reflect.apply", Reflect.apply(fn, null, args));
    show("value16[" + i + "] bind", fn.bind(null, 1, 2)(...args.slice(2)));
    show("value16[" + i + "] short", fn(1, 2, 3));
  }
  show("decl16 underapplied", decl16.apply(null, args.slice(0, 15)));
  show("decl16 overapplied", decl16.apply(null, range(19)));
  show("Reflect.construct Ctor16", Reflect.construct(Ctor16, args).value);
  show("Reflect.construct Klass16", Reflect.construct(Klass16, args).value);
  show("new Klass16 spread", new Klass16(...args).value);
  const k = new Klass16(...args);
  show("Klass16 method direct", k.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("Klass16 method apply", k.method.apply(k, args));
  show("Klass16 method spread", k.method(...args));
  show("Klass16 method Reflect.apply", Reflect.apply(k.method, k, args));
  show("objlit16 method direct", objlit16.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
  ));
  show("objlit16 method apply", objlit16.method.apply(objlit16, args));
  show("objlit16 method .call", objlit16.method.call(objlit16, ...args));
  show("objlit16 method spread", objlit16.method(...args));
  show("objlit16 method Reflect.apply", Reflect.apply(objlit16.method, objlit16, args));
}

// ---- 17 parameters ----
const arrow17 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
): string => sig([
  p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
]);

var fexpr17 = function fexpr17(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
): string {
  if (p0 > 1) {
    return fexpr17(
      p0 - 1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
    );
  }
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
  ]);
};

function decl17(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
): string {
  return arguments.length + "/" + sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
  ]);
}

function Ctor17(
  this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
  p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
) {
  this.value = sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
  ]);
}

class Klass17 {
  value: string;
  constructor(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
  ) {
    this.value = sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
    ]);
  }
  method(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
  ): string {
    return "klass:" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
    ]);
  }
}

const objlit17 = {
  tag: "obj",
  method(
    this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
    p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
  ): string {
    return this.tag + ":" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
    ]);
  },
};

{
  const args = range(17);
  const values: any[] = [arrow17, fexpr17, decl17, importedArrow17, importedFexpr17];
  show("arrow17 direct", arrow17(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("fexpr17 recursive", fexpr17(
    4, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("decl17 direct", decl17(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("importedArrow17 direct", importedArrow17(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("importedFexpr17 direct", importedFexpr17(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("importedRest 17 direct", importedRest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  const rest = (...values: any[]): string => "rest:" + sig(values);
  show("rest 17 direct", rest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("rest 17 apply", rest.apply(null, args));
  for (let i = 0; i < values.length; i++) {
    const fn = values[i];
    show("value17[" + i + "] call", fn(
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
    ));
    show("value17[" + i + "] apply", fn.apply(null, args));
    show("value17[" + i + "] .call", fn.call(null, ...args));
    show("value17[" + i + "] spread", fn(...args));
    show("value17[" + i + "] Reflect.apply", Reflect.apply(fn, null, args));
    show("value17[" + i + "] bind", fn.bind(null, 1, 2)(...args.slice(2)));
    show("value17[" + i + "] short", fn(1, 2, 3));
  }
  show("decl17 underapplied", decl17.apply(null, args.slice(0, 16)));
  show("decl17 overapplied", decl17.apply(null, range(20)));
  show("Reflect.construct Ctor17", Reflect.construct(Ctor17, args).value);
  show("Reflect.construct Klass17", Reflect.construct(Klass17, args).value);
  show("new Klass17 spread", new Klass17(...args).value);
  const k = new Klass17(...args);
  show("Klass17 method direct", k.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("Klass17 method apply", k.method.apply(k, args));
  show("Klass17 method spread", k.method(...args));
  show("Klass17 method Reflect.apply", Reflect.apply(k.method, k, args));
  show("objlit17 method direct", objlit17.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
  ));
  show("objlit17 method apply", objlit17.method.apply(objlit17, args));
  show("objlit17 method .call", objlit17.method.call(objlit17, ...args));
  show("objlit17 method spread", objlit17.method(...args));
  show("objlit17 method Reflect.apply", Reflect.apply(objlit17.method, objlit17, args));
}

// ---- 18 parameters ----
const arrow18 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any,
): string => sig([
  p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
]);

var fexpr18 = function fexpr18(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any,
): string {
  if (p0 > 1) {
    return fexpr18(
      p0 - 1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
    );
  }
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
  ]);
};

function decl18(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any,
): string {
  return arguments.length + "/" + sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
  ]);
}

function Ctor18(
  this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
  p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
  p17: any,
) {
  this.value = sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
  ]);
}

class Klass18 {
  value: string;
  constructor(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any,
  ) {
    this.value = sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
    ]);
  }
  method(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any,
  ): string {
    return "klass:" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
    ]);
  }
}

const objlit18 = {
  tag: "obj",
  method(
    this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
    p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any,
  ): string {
    return this.tag + ":" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
    ]);
  },
};

{
  const args = range(18);
  const values: any[] = [arrow18, fexpr18, decl18, importedArrow18, importedFexpr18];
  show("arrow18 direct", arrow18(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("fexpr18 recursive", fexpr18(
    4, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("decl18 direct", decl18(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("importedArrow18 direct", importedArrow18(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("importedFexpr18 direct", importedFexpr18(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("importedRest 18 direct", importedRest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  const rest = (...values: any[]): string => "rest:" + sig(values);
  show("rest 18 direct", rest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("rest 18 apply", rest.apply(null, args));
  for (let i = 0; i < values.length; i++) {
    const fn = values[i];
    show("value18[" + i + "] call", fn(
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
    ));
    show("value18[" + i + "] apply", fn.apply(null, args));
    show("value18[" + i + "] .call", fn.call(null, ...args));
    show("value18[" + i + "] spread", fn(...args));
    show("value18[" + i + "] Reflect.apply", Reflect.apply(fn, null, args));
    show("value18[" + i + "] bind", fn.bind(null, 1, 2)(...args.slice(2)));
    show("value18[" + i + "] short", fn(1, 2, 3));
  }
  show("decl18 underapplied", decl18.apply(null, args.slice(0, 17)));
  show("decl18 overapplied", decl18.apply(null, range(21)));
  show("Reflect.construct Ctor18", Reflect.construct(Ctor18, args).value);
  show("Reflect.construct Klass18", Reflect.construct(Klass18, args).value);
  show("new Klass18 spread", new Klass18(...args).value);
  const k = new Klass18(...args);
  show("Klass18 method direct", k.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("Klass18 method apply", k.method.apply(k, args));
  show("Klass18 method spread", k.method(...args));
  show("Klass18 method Reflect.apply", Reflect.apply(k.method, k, args));
  show("objlit18 method direct", objlit18.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
  ));
  show("objlit18 method apply", objlit18.method.apply(objlit18, args));
  show("objlit18 method .call", objlit18.method.call(objlit18, ...args));
  show("objlit18 method spread", objlit18.method(...args));
  show("objlit18 method Reflect.apply", Reflect.apply(objlit18.method, objlit18, args));
}

// ---- 32 parameters ----
const arrow32 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any,
): string => sig([
  p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18, p19,
  p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
]);

var fexpr32 = function fexpr32(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any,
): string {
  if (p0 > 1) {
    return fexpr32(
      p0 - 1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
      p18, p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
    );
  }
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
  ]);
};

function decl32(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any,
): string {
  return arguments.length + "/" + sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
  ]);
}

function Ctor32(
  this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
  p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
  p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any,
  p26: any, p27: any, p28: any, p29: any, p30: any, p31: any,
) {
  this.value = sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
  ]);
}

class Klass32 {
  value: string;
  constructor(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any,
    p25: any, p26: any, p27: any, p28: any, p29: any, p30: any, p31: any,
  ) {
    this.value = sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
      p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
    ]);
  }
  method(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any,
    p25: any, p26: any, p27: any, p28: any, p29: any, p30: any, p31: any,
  ): string {
    return "klass:" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
      p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
    ]);
  }
}

const objlit32 = {
  tag: "obj",
  method(
    this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
    p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any,
    p25: any, p26: any, p27: any, p28: any, p29: any, p30: any, p31: any,
  ): string {
    return this.tag + ":" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
      p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31,
    ]);
  },
};

{
  const args = range(32);
  const values: any[] = [arrow32, fexpr32, decl32, importedArrow32, importedFexpr32];
  show("arrow32 direct", arrow32(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("fexpr32 recursive", fexpr32(
    4, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("decl32 direct", decl32(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("importedArrow32 direct", importedArrow32(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("importedFexpr32 direct", importedFexpr32(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("importedRest 32 direct", importedRest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  const rest = (...values: any[]): string => "rest:" + sig(values);
  show("rest 32 direct", rest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("rest 32 apply", rest.apply(null, args));
  for (let i = 0; i < values.length; i++) {
    const fn = values[i];
    show("value32[" + i + "] call", fn(
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
      24, 25, 26, 27, 28, 29, 30, 31, 32,
    ));
    show("value32[" + i + "] apply", fn.apply(null, args));
    show("value32[" + i + "] .call", fn.call(null, ...args));
    show("value32[" + i + "] spread", fn(...args));
    show("value32[" + i + "] Reflect.apply", Reflect.apply(fn, null, args));
    show("value32[" + i + "] bind", fn.bind(null, 1, 2)(...args.slice(2)));
    show("value32[" + i + "] short", fn(1, 2, 3));
  }
  show("decl32 underapplied", decl32.apply(null, args.slice(0, 31)));
  show("decl32 overapplied", decl32.apply(null, range(35)));
  show("Reflect.construct Ctor32", Reflect.construct(Ctor32, args).value);
  show("Reflect.construct Klass32", Reflect.construct(Klass32, args).value);
  show("new Klass32 spread", new Klass32(...args).value);
  const k = new Klass32(...args);
  show("Klass32 method direct", k.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("Klass32 method apply", k.method.apply(k, args));
  show("Klass32 method spread", k.method(...args));
  show("Klass32 method Reflect.apply", Reflect.apply(k.method, k, args));
  show("objlit32 method direct", objlit32.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32,
  ));
  show("objlit32 method apply", objlit32.method.apply(objlit32, args));
  show("objlit32 method .call", objlit32.method.call(objlit32, ...args));
  show("objlit32 method spread", objlit32.method(...args));
  show("objlit32 method Reflect.apply", Reflect.apply(objlit32.method, objlit32, args));
}

// ---- 64 parameters ----
const arrow64 = (
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any, p32: any, p33: any, p34: any, p35: any, p36: any,
  p37: any, p38: any, p39: any, p40: any, p41: any, p42: any, p43: any, p44: any, p45: any,
  p46: any, p47: any, p48: any, p49: any, p50: any, p51: any, p52: any, p53: any, p54: any,
  p55: any, p56: any, p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
): string => sig([
  p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18, p19,
  p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35, p36, p37,
  p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52, p53, p54, p55,
  p56, p57, p58, p59, p60, p61, p62, p63,
]);

var fexpr64 = function fexpr64(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any, p32: any, p33: any, p34: any, p35: any, p36: any,
  p37: any, p38: any, p39: any, p40: any, p41: any, p42: any, p43: any, p44: any, p45: any,
  p46: any, p47: any, p48: any, p49: any, p50: any, p51: any, p52: any, p53: any, p54: any,
  p55: any, p56: any, p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
): string {
  if (p0 > 1) {
    return fexpr64(
      p0 - 1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17,
      p18, p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34,
      p35, p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51,
      p52, p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
    );
  }
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
    p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
    p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
  ]);
};

function decl64(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, p17: any, p18: any,
  p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any, p26: any, p27: any,
  p28: any, p29: any, p30: any, p31: any, p32: any, p33: any, p34: any, p35: any, p36: any,
  p37: any, p38: any, p39: any, p40: any, p41: any, p42: any, p43: any, p44: any, p45: any,
  p46: any, p47: any, p48: any, p49: any, p50: any, p51: any, p52: any, p53: any, p54: any,
  p55: any, p56: any, p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
): string {
  return arguments.length + "/" + sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
    p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
    p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
  ]);
}

function Ctor64(
  this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
  p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
  p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any, p25: any,
  p26: any, p27: any, p28: any, p29: any, p30: any, p31: any, p32: any, p33: any, p34: any,
  p35: any, p36: any, p37: any, p38: any, p39: any, p40: any, p41: any, p42: any, p43: any,
  p44: any, p45: any, p46: any, p47: any, p48: any, p49: any, p50: any, p51: any, p52: any,
  p53: any, p54: any, p55: any, p56: any, p57: any, p58: any, p59: any, p60: any, p61: any,
  p62: any, p63: any,
) {
  this.value = sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
    p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
    p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
    p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
  ]);
}

class Klass64 {
  value: string;
  constructor(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any,
    p25: any, p26: any, p27: any, p28: any, p29: any, p30: any, p31: any, p32: any,
    p33: any, p34: any, p35: any, p36: any, p37: any, p38: any, p39: any, p40: any,
    p41: any, p42: any, p43: any, p44: any, p45: any, p46: any, p47: any, p48: any,
    p49: any, p50: any, p51: any, p52: any, p53: any, p54: any, p55: any, p56: any,
    p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
  ) {
    this.value = sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
      p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
      p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
      p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
    ]);
  }
  method(
    p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any,
    p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any,
    p25: any, p26: any, p27: any, p28: any, p29: any, p30: any, p31: any, p32: any,
    p33: any, p34: any, p35: any, p36: any, p37: any, p38: any, p39: any, p40: any,
    p41: any, p42: any, p43: any, p44: any, p45: any, p46: any, p47: any, p48: any,
    p49: any, p50: any, p51: any, p52: any, p53: any, p54: any, p55: any, p56: any,
    p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
  ): string {
    return "klass:" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
      p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
      p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
      p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
    ]);
  }
}

const objlit64 = {
  tag: "obj",
  method(
    this: any, p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any,
    p8: any, p9: any, p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any,
    p17: any, p18: any, p19: any, p20: any, p21: any, p22: any, p23: any, p24: any,
    p25: any, p26: any, p27: any, p28: any, p29: any, p30: any, p31: any, p32: any,
    p33: any, p34: any, p35: any, p36: any, p37: any, p38: any, p39: any, p40: any,
    p41: any, p42: any, p43: any, p44: any, p45: any, p46: any, p47: any, p48: any,
    p49: any, p50: any, p51: any, p52: any, p53: any, p54: any, p55: any, p56: any,
    p57: any, p58: any, p59: any, p60: any, p61: any, p62: any, p63: any,
  ): string {
    return this.tag + ":" + sig([
      p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18,
      p19, p20, p21, p22, p23, p24, p25, p26, p27, p28, p29, p30, p31, p32, p33, p34, p35,
      p36, p37, p38, p39, p40, p41, p42, p43, p44, p45, p46, p47, p48, p49, p50, p51, p52,
      p53, p54, p55, p56, p57, p58, p59, p60, p61, p62, p63,
    ]);
  },
};

{
  const args = range(64);
  const values: any[] = [arrow64, fexpr64, decl64, importedArrow64, importedFexpr64];
  show("arrow64 direct", arrow64(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("fexpr64 recursive", fexpr64(
    4, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("decl64 direct", decl64(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("importedArrow64 direct", importedArrow64(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("importedFexpr64 direct", importedFexpr64(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("importedRest 64 direct", importedRest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  const rest = (...values: any[]): string => "rest:" + sig(values);
  show("rest 64 direct", rest(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("rest 64 apply", rest.apply(null, args));
  for (let i = 0; i < values.length; i++) {
    const fn = values[i];
    show("value64[" + i + "] call", fn(
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
      24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
      45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
    ));
    show("value64[" + i + "] apply", fn.apply(null, args));
    show("value64[" + i + "] .call", fn.call(null, ...args));
    show("value64[" + i + "] spread", fn(...args));
    show("value64[" + i + "] Reflect.apply", Reflect.apply(fn, null, args));
    show("value64[" + i + "] bind", fn.bind(null, 1, 2)(...args.slice(2)));
    show("value64[" + i + "] short", fn(1, 2, 3));
  }
  show("decl64 underapplied", decl64.apply(null, args.slice(0, 63)));
  show("decl64 overapplied", decl64.apply(null, range(67)));
  show("Reflect.construct Ctor64", Reflect.construct(Ctor64, args).value);
  show("Reflect.construct Klass64", Reflect.construct(Klass64, args).value);
  show("new Klass64 spread", new Klass64(...args).value);
  const k = new Klass64(...args);
  show("Klass64 method direct", k.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("Klass64 method apply", k.method.apply(k, args));
  show("Klass64 method spread", k.method(...args));
  show("Klass64 method Reflect.apply", Reflect.apply(k.method, k, args));
  show("objlit64 method direct", objlit64.method(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
  ));
  show("objlit64 method apply", objlit64.method.apply(objlit64, args));
  show("objlit64 method .call", objlit64.method.call(objlit64, ...args));
  show("objlit64 method spread", objlit64.method(...args));
  show("objlit64 method Reflect.apply", Reflect.apply(objlit64.method, objlit64, args));
}

// #10425's own repro: Reflect.apply with a native callee and a rest callee.
show("Reflect.apply Math.max", Reflect.apply(Math.max, null, [1, 2, 3, 4, 5, 6]));
function restJoin(...a: number[]): string {
  return a.length + ":" + a.join(",");
}
show("Reflect.apply rest", Reflect.apply(restJoin, null, [1, 2, 3, 4, 5, 6, 7]));
show("Reflect.construct Array", Reflect.construct(Array, [1, 2, 3, 4, 5, 6]).length);
show("apply rest", restJoin.apply(null, [1, 2, 3, 4, 5, 6, 7]));
function thisProbe(this: any, a: any, b: any, c: any, d: any, e: any): string {
  return this.name + ":" + [a, b, c, d, e].join(",");
}
show("Reflect.apply this+5", Reflect.apply(thisProbe, { name: "recv" }, [1, 2, 3, 4, 5]));

// A rest parameter after 17 fixed parameters.
function fixedRest(
  p0: any, p1: any, p2: any, p3: any, p4: any, p5: any, p6: any, p7: any, p8: any, p9: any,
  p10: any, p11: any, p12: any, p13: any, p14: any, p15: any, p16: any, ...rest: any[]
): string {
  return sig([
    p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16,
  ]) + "+" + sig(rest);
}
show("fixedRest direct", fixedRest(
  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
));
show("fixedRest apply", fixedRest.apply(null, range(20)));
show("fixedRest Reflect.apply", Reflect.apply(fixedRest, null, range(20)));
const fixedRestValue: any = fixedRest;
show("fixedRest value spread", fixedRestValue(...range(20)));

// A callee declaring one parameter still sees every argument via `arguments`.
function countArgs(a: any): string {
  return arguments.length + ":" + a + ":" + arguments[arguments.length - 1];
}
show("arguments 100 apply", countArgs.apply(null, range(100)));
show("arguments 100 Reflect.apply", Reflect.apply(countArgs, null, range(100)));
show("Math.max 200 apply", Math.max.apply(null, range(200)));

// Timer and nextTick callbacks forward their extra arguments too.
setTimeout(
  (...values: any[]) => {
    show("setTimeout 12 args", sig(values));
    const interval = setInterval(
      (...more: any[]) => {
        clearInterval(interval);
        show("setInterval 10 args", sig(more));
      },
      1,
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    );
  },
  0,
  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
);
process.nextTick(
  (...values: any[]) => show("nextTick 11 args", sig(values)),
  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
);
