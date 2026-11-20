// GH #3582: addition must run ToPrimitive(default) before choosing string,
// BigInt, or numeric addition, and abrupt completions from coercion/spread
// must propagate. Compared byte-for-byte against
// `node --experimental-strip-types`.

const order: string[] = [];

console.log("object-valueOf:", (({ valueOf() { return 1; } } as any) + 1));
console.log(
  "object-toString:",
  (({ valueOf() { return {}; }, toString() { return "ok"; } } as any) + "!"),
);
console.log("string-left-valueOf:", ("a" + ({ valueOf() { return 7; } } as any)));
console.log("array-default:", (([1, 2] as any) + 3));
console.log("boxed-boolean:", ((new Boolean(true) as any) + true));
console.log("boxed-number:", ((new Number(1) as any) + 1));
console.log("boxed-string:", ((new String("1") as any) + "1"));

const date = new Date(0);
console.log("date-default-eq:", date + 0 === date.toString() + "0");

function f1() {
  return 0;
}
function f2() {
  return 0;
}
(f2 as any).valueOf = function() {
  return 1;
};
function f3() {
  return 0;
}
(f3 as any).toString = function() {
  return 1;
};
function f4() {
  return 0;
}
(f4 as any).valueOf = function() {
  return -1;
};
(f4 as any).toString = function() {
  return 1;
};
console.log("function-default-eq:", (f1 as any) + 1 === (f1 as any).toString() + 1);
console.log("function-valueOf:", 1 + (f2 as any));
console.log("function-toString:", 1 + (f3 as any));
console.log("function-valueOf-wins:", (f4 as any) + 1);

const lhs: any = {
  valueOf() {
    order.push("lhs-valueOf");
    return 1;
  },
};
const rhs: any = {
  valueOf() {
    order.push("rhs-valueOf");
    return 2;
  },
};
console.log("order:", (lhs + rhs) + "|" + order.join(","));

const symPrim: any = {
  [Symbol.toPrimitive](hint: string) {
    order.push("hint-" + hint);
    return "p";
  },
};
console.log("symbol-toPrimitive:", symPrim + 1);
console.log("symbol-toPrimitive-hint:", order[order.length - 1]);

try {
  (({ valueOf() { throw "boom"; } } as any) + 1);
} catch (e: any) {
  console.log("throw-valueOf:", e);
}

try {
  (({ valueOf() { return {}; }, toString() { return {}; } } as any) + 1);
} catch (e: any) {
  console.log("nonprimitive:", e.name);
}

try {
  ((Symbol("s") as any) + "");
} catch (e: any) {
  console.log("symbol-add:", e.name);
}

try {
  ((1n as any) + 1);
} catch (e: any) {
  console.log("bigint-number:", e.name);
}

try {
  (1 as any) + (1n as any);
} catch (e: any) {
  console.log("number-bigint:", e.name);
}

const orderThrow: string[] = [];
try {
  (({
    valueOf() {
      orderThrow.push("lhs");
      return Symbol("lhs");
    },
  } as any) + ({
    valueOf() {
      orderThrow.push("rhs");
      throw "rhs-boom";
    },
  } as any));
} catch (e: any) {
  console.log("toprimitive-before-tonumeric:", e, orderThrow.join(","));
}

try {
  [...({ [Symbol.iterator]() { throw new Error("iter"); } } as any)];
} catch (e: any) {
  console.log("spread-iter-throw:", e.message);
}

try {
  [...({ [Symbol.iterator]: 1 } as any)];
} catch (e: any) {
  console.log("spread-iter-noncallable:", e.name);
}

try {
  [...({ [Symbol.iterator]() { return 1; } } as any)];
} catch (e: any) {
  console.log("spread-iter-result:", e.name);
}

try {
  [...({
    [Symbol.iterator]() {
      return {
        next() {
          throw new Error("next");
        },
      };
    },
  } as any)];
} catch (e: any) {
  console.log("spread-next-throw:", e.message);
}
