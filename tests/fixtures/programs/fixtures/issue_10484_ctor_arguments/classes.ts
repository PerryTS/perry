// #10484: classes whose constructors read `arguments`, imported by
// test_gap_10484_class_constructor_arguments.ts through ESM bindings.

export function fmtArgs(a: ArrayLike<any>): string {
  return a.length + ":" + JSON.stringify(Array.from(a));
}

export class ExpTwo {
  r: string;
  constructor(p?: any, q?: any) {
    this.r = fmtArgs(arguments);
  }
}

export class ExpDefault {
  r: string;
  constructor(p: any, q: any = "dq") {
    this.r = fmtArgs(arguments) + " q=" + q;
  }
}

export class ExpRest {
  r: string;
  constructor(first?: any, ...rest: any[]) {
    this.r = fmtArgs(arguments) + " rest=" + JSON.stringify(rest);
  }
}

export class ExpLength {
  n: number;
  constructor(p?: any, q?: any) {
    this.n = arguments.length;
  }
}

export class ExpDerived extends ExpTwo {
  constructor(x?: any) {
    super(...arguments);
  }
}

export class ExpNoCtorDerived extends ExpTwo {}

export class ExpNoCtorRest extends ExpRest {}

const tag = "cap";
export function makeCapturing() {
  const local = tag + "-" + "tured";
  return class Capturing {
    r: string;
    constructor(p?: any) {
      this.r = fmtArgs(arguments) + " " + local;
    }
  };
}
