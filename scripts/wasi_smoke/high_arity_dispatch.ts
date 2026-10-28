// A constructor reached through an untyped class value is called by the
// runtime with `this` plus every parameter, however many there are. Here that
// is 17 and 63 parameters, i.e. 18 arguments (the arity a Fastify HTTP service
// reaches at startup) and 64, the widest the wasm fallback dispatches. The sum
// is position weighted, so a dropped, swapped or uninitialised argument changes
// the printed value.

class Wide17 {
  total: number;
  constructor(
    a0: number, a1: number, a2: number, a3: number, a4: number, a5: number,
    a6: number, a7: number, a8: number, a9: number, a10: number, a11: number,
    a12: number, a13: number, a14: number, a15: number, a16: number,
  ) {
    this.total =
      a0 * 1 + a1 * 2 + a2 * 3 + a3 * 4 + a4 * 5 +
      a5 * 6 + a6 * 7 + a7 * 8 + a8 * 9 + a9 * 10 +
      a10 * 11 + a11 * 12 + a12 * 13 + a13 * 14 + a14 * 15 +
      a15 * 16 + a16 * 17;
  }
}

class Wide63 {
  total: number;
  constructor(
    a0: number, a1: number, a2: number, a3: number, a4: number, a5: number,
    a6: number, a7: number, a8: number, a9: number, a10: number, a11: number,
    a12: number, a13: number, a14: number, a15: number, a16: number, a17: number,
    a18: number, a19: number, a20: number, a21: number, a22: number, a23: number,
    a24: number, a25: number, a26: number, a27: number, a28: number, a29: number,
    a30: number, a31: number, a32: number, a33: number, a34: number, a35: number,
    a36: number, a37: number, a38: number, a39: number, a40: number, a41: number,
    a42: number, a43: number, a44: number, a45: number, a46: number, a47: number,
    a48: number, a49: number, a50: number, a51: number, a52: number, a53: number,
    a54: number, a55: number, a56: number, a57: number, a58: number, a59: number,
    a60: number, a61: number, a62: number,
  ) {
    this.total =
      a0 * 1 + a1 * 2 + a2 * 3 + a3 * 4 + a4 * 5 +
      a5 * 6 + a6 * 7 + a7 * 8 + a8 * 9 + a9 * 10 +
      a10 * 11 + a11 * 12 + a12 * 13 + a13 * 14 + a14 * 15 +
      a15 * 16 + a16 * 17 + a17 * 18 + a18 * 19 + a19 * 20 +
      a20 * 21 + a21 * 22 + a22 * 23 + a23 * 24 + a24 * 25 +
      a25 * 26 + a26 * 27 + a27 * 28 + a28 * 29 + a29 * 30 +
      a30 * 31 + a31 * 32 + a32 * 33 + a33 * 34 + a34 * 35 +
      a35 * 36 + a36 * 37 + a37 * 38 + a38 * 39 + a39 * 40 +
      a40 * 41 + a41 * 42 + a42 * 43 + a43 * 44 + a44 * 45 +
      a45 * 46 + a46 * 47 + a47 * 48 + a48 * 49 + a49 * 50 +
      a50 * 51 + a51 * 52 + a52 * 53 + a53 * 54 + a54 * 55 +
      a55 * 56 + a56 * 57 + a57 * 58 + a58 * 59 + a59 * 60 +
      a60 * 61 + a61 * 62 + a62 * 63;
  }
}

function construct(name: string, size: number, Ctor: any): void {
  const args: number[] = [];
  for (let i = 0; i < size; i++) args.push(i + 1);
  console.log(name, new Ctor(...args).total);
}

construct("17 params", 17, Wide17);
construct("63 params", 63, Wide63);
