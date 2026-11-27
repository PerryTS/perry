// ToPrimitive throws a TypeError when Symbol.toPrimitive is neither undefined,
// null nor callable, and when the method returns an object. Every coercion
// path (ToNumber, ToString, ToPropertyKey, the + operator) runs the same
// steps.

function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e) {
    console.log(label, (e as Error).constructor.name, (e as Error).message);
  }
}

const notCallable: unknown[] = [42, "", true, Symbol.toPrimitive, {}, [1]];
for (const method of notCallable) {
  const o: any = { [Symbol.toPrimitive]: method };
  const name = typeof method === "symbol" ? "symbol" : JSON.stringify(method);
  show(`not callable ${name}: +`, () => +o);
  show(`not callable ${name}: isNaN`, () => isNaN(o));
  show(`not callable ${name}: String`, () => String(o));
  show(`not callable ${name}: template`, () => `${o}`);
  show(`not callable ${name}: concat`, () => o + "");
  show(`not callable ${name}: key`, () => ({ a: 1 } as any)[o]);
}

const returnsObject: any = { [Symbol.toPrimitive]: () => [42] };
const returnsSelf: any = {
  [Symbol.toPrimitive]() {
    return this;
  },
};
for (const [name, o] of [["array result", returnsObject], ["self result", returnsSelf]] as const) {
  show(`${name}: +`, () => +o);
  show(`${name}: Number`, () => Number(o));
  show(`${name}: isFinite`, () => isFinite(o));
  show(`${name}: Math.abs`, () => Math.abs(o));
  show(`${name}: String`, () => String(o));
  show(`${name}: template`, () => `${o}`);
  show(`${name}: multiply`, () => o * 1);
  show(`${name}: key`, () => ({ a: 1 } as any)[o]);
}

// undefined and null mean "absent": OrdinaryToPrimitive runs.
const nullMethod: any = { [Symbol.toPrimitive]: null, valueOf: () => 7 };
const undefinedMethod: any = { [Symbol.toPrimitive]: undefined, toString: () => "s" };
show("null method", () => +nullMethod);
show("undefined method", () => `${undefinedMethod}`);
show("primitive result", () => +{ [Symbol.toPrimitive]: (hint: string) => (hint === "number" ? 3 : "x") });

// OrdinaryToPrimitive calls each method once: a valueOf that returns an
// object on an object with no toString is a TypeError after one call.
for (const proto of ["plain", "null"]) {
  let calls = 0;
  const o: any = proto === "plain" ? {} : Object.create(null);
  o.valueOf = () => {
    calls++;
    return {};
  };
  show(`${proto} prototype valueOf object`, () => `${String(+o)} after ${calls}`);
  show(`${proto} prototype valueOf calls`, () => calls);
}
show("array", () => +[5]);
show("map", () => String(+new Map()));
show("date", () => typeof +new Date(0));
