// Gap tests for Error / AggregateError parity:
//   #2836 — cause options for Error subclasses + AggregateError
//   #2838 — AggregateError consumes iterable errors / rejects non-iterables
//   #2904 — Error static stack helpers + isError
//   #2882 — AggregateError as a real global constructor

// ---- #2836: cause options across subclasses and dynamic options ----
const cause = { why: 1 };
const opts = { cause };
console.log("error dynamic:", new Error("outer", opts).cause === cause);
console.log("type cause:", new TypeError("bad", { cause }).cause === cause);
console.log("range cause:", new RangeError("bad", { cause: "r" }).cause);
console.log("syntax cause:", new SyntaxError("bad", { cause: 123 }).cause);
console.log("ref cause:", new ReferenceError("bad", { cause: true }).cause);

const agg2836 = new AggregateError([1, 2], "many", { cause });
console.log("agg cause:", agg2836.cause === cause, JSON.stringify(agg2836.errors));

// Subclass kinds still hold under instanceof after the options path
console.log(
  "type instanceof:",
  new TypeError("x", { cause }) instanceof TypeError,
  new TypeError("x", { cause }) instanceof Error,
);

// ---- #2838: AggregateError iterable consumption / validation ----
function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label + ":", err?.name);
  }
}

show("array", () => new AggregateError([1, 2], "m").errors);
show("set", () => new AggregateError(new Set([1, 2]), "m").errors);
show("string", () => new AggregateError("ab" as any, "m").errors);
show("generator", () => {
  function* g() {
    yield "x";
    yield "y";
  }
  return new AggregateError(g(), "m").errors;
});
show("undefined", () => new AggregateError(undefined as any, "m"));
show("number", () => new AggregateError(1 as any, "m"));
show("omitted", () => new (AggregateError as any)());

// ---- #2904: Error static helpers / isError ----
console.log("captureStackTrace typeof:", typeof Error.captureStackTrace);
console.log("isError typeof:", typeof Error.isError);
console.log("isError(new Error):", Error.isError(new Error("x")));
console.log("isError({}):", Error.isError({}));
console.log("isError(TypeError):", Error.isError(new TypeError("x")));
console.log("isError(Agg):", Error.isError(new AggregateError([1], "m")));
console.log("isError(1):", Error.isError(1));
console.log("stackTraceLimit typeof:", typeof Error.stackTraceLimit);
console.log("prepareStackTrace typeof:", typeof Error.prepareStackTrace);

const target: any = {};
Error.captureStackTrace(target);
console.log("captured stack typeof:", typeof target.stack);

// ---- #2882: AggregateError as a real global constructor ----
const e2882 = new AggregateError([1], "m");
console.log("AggregateError typeof:", typeof AggregateError);
console.log("globalThis.AggregateError typeof:", typeof globalThis.AggregateError);
console.log("e instanceof AggregateError:", e2882 instanceof AggregateError);
console.log("e instanceof Error:", e2882 instanceof Error);
console.log("AggregateError.prototype typeof:", typeof AggregateError.prototype);
// NOTE: `AggregateError.prototype.constructor === AggregateError` and the
// rebound `const A = globalThis.AggregateError; new A(...)` construction are
// pre-existing general gaps shared by EVERY Perry builtin Error constructor
// (base Error/TypeError fail them identically), not AggregateError-specific —
// so they are intentionally excluded here.
