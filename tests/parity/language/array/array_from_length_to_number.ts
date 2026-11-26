// LengthOfArrayLike is ToLength(ToNumber(Get(O, "length"))): ToNumber throws
// for a BigInt or Symbol length in every reader of an array-like's length.
function show(label: string, run: () => unknown): void {
  try {
    console.log(label + ":", JSON.stringify(run()));
  } catch (err) {
    console.log(label + ": threw", (err as Error).constructor.name);
  }
}

type Loose = ArrayLike<unknown> & Record<string, unknown>;
const like = (length: unknown, ...items: unknown[]): Loose =>
  Object.assign({ length }, items) as unknown as Loose;
const proto = Array.prototype as unknown as Record<string, (...args: unknown[]) => unknown>;
const spreadable = (length: unknown) =>
  new Proxy({ length, 0: "c", [Symbol.isConcatSpreadable]: true }, {}) as unknown as unknown[];

for (const [label, length] of [["bigint", 1n], ["symbol", Symbol("n")], ["number", 1]] as const) {
  show(`Array.from ${label}`, () => Array.from(like(length, "a")));
  show(`Array.from proxy ${label}`, () => Array.from(new Proxy(like(length, "p"), {})));
  show(`concat proxy ${label}`, () => ([] as unknown[]).concat(spreadable(length)));
  show(`map.call ${label}`, () => proto.map.call(like(length, "m"), (v: unknown) => v));
  show(`indexOf.call ${label}`, () => proto.indexOf.call(like(length, "i"), "i"));
  show(`fill.call ${label}`, () => Array.from(proto.fill.call(like(length, "f"), "F") as Loose));
  show(`reverse.call ${label}`, () => Array.from(proto.reverse.call(like(length, "r")) as Loose));
}

show("string length", () => Array.from(like("2", "a", "b")));
show("valueOf length", () => Array.from(like({ valueOf: () => 2 }, "a", "b")));
show("negative length", () => Array.from(like(-3)));
