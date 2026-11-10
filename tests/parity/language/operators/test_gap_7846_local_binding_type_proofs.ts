// A local's declared or initializer-refined type is not proof about the value
// currently in its slot. Exercise both ways that proof becomes invalid: an
// erased annotation can lie at initialization, and a later assignment can
// replace an honestly initialized value with another kind.

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

const declaredNumberHoldsObject: number = { kind: "object" } as any;
console.log(
  "declared-number-object",
  declaredNumberHoldsObject ? "truthy" : "falsy",
  !declaredNumberHoldsObject,
  Boolean(declaredNumberHoldsObject),
);
assert(Boolean(declaredNumberHoldsObject), "declared number object lost truthiness");
assert(!(!declaredNumberHoldsObject), "declared number object negation is wrong");

const declaredBooleanHoldsNumber: boolean = 7 as any;
console.log(
  "declared-boolean-number",
  declaredBooleanHoldsNumber ? "truthy" : "falsy",
  !declaredBooleanHoldsNumber,
  Boolean(declaredBooleanHoldsNumber),
);
assert(Boolean(declaredBooleanHoldsNumber), "declared boolean number lost truthiness");
assert(!(!declaredBooleanHoldsNumber), "declared boolean number negation is wrong");

const declaredNumberHoldsEmptyString: number = "" as any;
console.log(
  "declared-number-empty-string",
  declaredNumberHoldsEmptyString ? "truthy" : "falsy",
  !declaredNumberHoldsEmptyString,
  Boolean(declaredNumberHoldsEmptyString),
);
assert(!Boolean(declaredNumberHoldsEmptyString), "empty string became truthy");
assert(!declaredNumberHoldsEmptyString, "empty string negation is wrong");

let refinedNumber: any = 0;
refinedNumber = { after: "write" };
console.log(
  "refined-number-reassigned",
  refinedNumber ? "truthy" : "falsy",
  !refinedNumber,
  Boolean(refinedNumber),
);
assert(Boolean(refinedNumber), "reassigned object lost truthiness");
assert(!(!refinedNumber), "reassigned object negation is wrong");

let refinedBoolean: any = false;
refinedBoolean = "now a string";
console.log(
  "refined-boolean-reassigned",
  refinedBoolean ? "truthy" : "falsy",
  !refinedBoolean,
  Boolean(refinedBoolean),
);
assert(Boolean(refinedBoolean), "reassigned string lost truthiness");
assert(!(!refinedBoolean), "reassigned string negation is wrong");

// Pin the #7844 directions beside the broader truthiness cases: the same
// whole-region write set must invalidate positive and negative array folds.
let arrayToNumber: any = [1, 2, 3];
arrayToNumber = 42;
let numberToArray: any = 0;
numberToArray = [numberToArray];
console.log(
  "array-folds",
  Array.isArray(arrayToNumber),
  Array.isArray(numberToArray),
);
assert(!Array.isArray(arrayToNumber), "reassigned array still folded true");
assert(Array.isArray(numberToArray), "reassigned number still folded false");

// Typed closure clones may use an annotation only as a guarded candidate.
// The capture is immutable but its erased declaration lies immediately, so
// both public and direct typed entry paths must take the generic fallback.
const declaredNumericCapture: number = "capture" as any;
const appendCaptured = (value: number): number =>
  declaredNumericCapture + value;
console.log("guarded-capture", appendCaptured(7));
assert(appendCaptured(7) === ("capture7" as any), "typed capture skipped fallback");

// Scalar-replaced constructors inline their parameter bindings into the
// caller. The parameter's `number` annotation still cannot turn the live
// string argument into raw-f64 field evidence.
class Counter {
  value: number;

  constructor(value: number) {
    this.value = value;
  }

  bump(): number {
    this.value = this.value + 1;
    return this.value;
  }
}

const declaredNumericCtorArg: number = "counter" as any;
const counter = new Counter(declaredNumericCtorArg);
console.log("scalar-constructor-arg", counter.bump());
assert(counter.value === ("counter1" as any), "constructor argument became raw f64");

// A metadata-selected numeric `+` push must retain its live number guard even
// when typed-feedback instrumentation is disabled. Pointer-only bookkeeping is
// insufficient because false/undefined are non-number values too.
const numericLayoutArray: number[] = [0, 1];
const declaredPushNumber: number = "push" as any;
numericLayoutArray.push(declaredPushNumber + 2);
console.log("guarded-array-push", numericLayoutArray[2]);
assert(
  (numericLayoutArray[2] as any) === "push2",
  "array push stored an annotation lie as raw f64",
);

// Declared class metadata may select the guarded plain-field IC, but it must
// not directly call an accessor or bind a method from the declared class.
class DeclaredReceiver {
  get label(): string {
    return "declared";
  }

  method(): string {
    return "declared-method";
  }
}

class LiveReceiver {
  get label(): string {
    return "live";
  }

  method(): string {
    return "live-method";
  }
}

function readDeclaredReceiver(value: DeclaredReceiver): string {
  const bound = value.method;
  return value.label + ":" + bound();
}

const declaredReceiverResult = readDeclaredReceiver(new LiveReceiver() as any);
console.log("declared-class-fallback", declaredReceiverResult);
assert(
  declaredReceiverResult === "live:live-method",
  "declared class selected direct behavior for another live class",
);

// Eight static-window reads trigger the straight-line masked-region matcher.
// The first assignment must not refine `maskedAccum` from a lying annotation
// on its other operand; otherwise later `+` operations lose concatenation.
const maskedValues: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
const declaredMaskedNumber: number = "m" as any;
let maskedAccum: any = 0;
maskedAccum = declaredMaskedNumber + maskedValues[0];
maskedAccum = maskedAccum + maskedValues[1];
maskedAccum = maskedAccum + maskedValues[2];
maskedAccum = maskedAccum + maskedValues[3];
maskedAccum = maskedAccum + maskedValues[4];
maskedAccum = maskedAccum + maskedValues[5];
maskedAccum = maskedAccum + maskedValues[6];
maskedAccum = maskedAccum + maskedValues[7];
console.log("masked-window-annotation", maskedAccum);
assert(maskedAccum === "m12345678", "masked-window refinement trusted metadata");

// Nested arithmetic over `any` operands can still be BigInt throughout. The
// initializer must not publish Number evidence for the result local.
const bigintA: any = 9n;
const bigintB: any = 4n;
const bigintC: any = 7n;
const bigintD: any = 2n;
const nestedBigInt = (bigintA - bigintB) * (bigintC - bigintD);
const negatedNestedBigInt = -nestedBigInt;
console.log("nested-bigint-proof", typeof nestedBigInt, negatedNestedBigInt);
assert(nestedBigInt === 25n, "nested BigInt arithmetic was refined as Number");
assert(negatedNestedBigInt === -25n, "BigInt local used raw numeric negation");
