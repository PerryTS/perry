// A local's declared or initializer-refined type is not proof about the value
// currently in its slot. Exercise both ways that proof becomes invalid: an
// erased annotation can lie at initialization, and a later assignment can
// replace an honestly initialized value with another kind.

const declaredNumberHoldsObject: number = { kind: "object" } as any;
console.log(
  "declared-number-object",
  declaredNumberHoldsObject ? "truthy" : "falsy",
  !declaredNumberHoldsObject,
  Boolean(declaredNumberHoldsObject),
);

const declaredBooleanHoldsNumber: boolean = 7 as any;
console.log(
  "declared-boolean-number",
  declaredBooleanHoldsNumber ? "truthy" : "falsy",
  !declaredBooleanHoldsNumber,
  Boolean(declaredBooleanHoldsNumber),
);

const declaredNumberHoldsEmptyString: number = "" as any;
console.log(
  "declared-number-empty-string",
  declaredNumberHoldsEmptyString ? "truthy" : "falsy",
  !declaredNumberHoldsEmptyString,
  Boolean(declaredNumberHoldsEmptyString),
);

let refinedNumber: any = 0;
refinedNumber = { after: "write" };
console.log(
  "refined-number-reassigned",
  refinedNumber ? "truthy" : "falsy",
  !refinedNumber,
  Boolean(refinedNumber),
);

let refinedBoolean: any = false;
refinedBoolean = "now a string";
console.log(
  "refined-boolean-reassigned",
  refinedBoolean ? "truthy" : "falsy",
  !refinedBoolean,
  Boolean(refinedBoolean),
);

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

// Typed closure clones may use an annotation only as a guarded candidate.
// The capture is immutable but its erased declaration lies immediately, so
// both public and direct typed entry paths must take the generic fallback.
const declaredNumericCapture: number = "capture" as any;
const appendCaptured = (value: number): number =>
  declaredNumericCapture + value;
console.log("guarded-capture", appendCaptured(7));

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
