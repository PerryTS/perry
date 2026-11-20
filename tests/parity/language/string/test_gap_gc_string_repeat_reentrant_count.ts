// #8427: `String.prototype.repeat` used to borrow the receiver's inline WTF-8
// payload BEFORE coercing `count`, and ToNumber on an object count runs user
// JS. A moving collection inside that `valueOf` relocated the receiver, so the
// borrow named retired from-space and the result was copied out of garbage
// ("repeat [invalid utf8]" under the instruments below). Keep the reentrant
// count shapes under moving-GC pressure so the ordering contract stays gated.
// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=8427 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1

// Allocate hard enough inside the callback to reach loop back-edge safepoints
// while the receiver is still young.
function churn(): number {
  let junk = "";
  for (let i = 0; i < 25; i++) {
    junk = junk + "x";
  }
  return junk.length;
}

let badRepeat = 0;
let callbackRuns = 0;
for (let i = 0; i < 3; i++) {
  // A fresh, joined receiver is a young heap string, not SSO and not interned.
  const subject = ["abc", "def", String(i % 10)].join("");
  const repeated = subject.repeat({
    valueOf() {
      callbackRuns++;
      churn();
      return 3;
    },
  } as unknown as number);
  if (repeated !== subject + subject + subject) badRepeat++;
}
console.log("repeat under reentrant count:", badRepeat === 0, callbackRuns);

// `Symbol.toPrimitive` takes precedence over `valueOf` and is the same window.
let badPrimitive = 0;
for (let i = 0; i < 3; i++) {
  const subject = ["uvw", "xyz", String(i % 10)].join("");
  const repeated = subject.repeat({
    [Symbol.toPrimitive]() {
      churn();
      return 2;
    },
  } as unknown as number);
  if (repeated !== subject + subject) badPrimitive++;
}
console.log("repeat under toPrimitive count:", badPrimitive === 0);

// A count of 0 returns "" without ever reading the receiver — the callback
// still runs, and still allocates, exactly once per call.
let zeroRuns = 0;
const zeroSubject = ["mno", "pqr"].join("");
const zeroResult = zeroSubject.repeat({
  valueOf() {
    zeroRuns++;
    churn();
    return 0;
  },
} as unknown as number);
console.log("zero count:", JSON.stringify(zeroResult), zeroRuns);

// ToIntegerOrInfinity is observable even when the receiver is empty, and a
// negative count must throw before repeat's empty-string return.
let emptyCoercions = 0;
try {
  "".repeat({
    valueOf() {
      emptyCoercions++;
      churn();
      throw new Error("empty-count");
    },
  } as unknown as number);
} catch (error) {
  console.log("empty throw:", emptyCoercions, (error as Error).message);
}

try {
  "".repeat({
    valueOf() {
      emptyCoercions++;
      churn();
      return -1;
    },
  } as unknown as number);
} catch (error) {
  console.log("empty negative:", emptyCoercions, error instanceof RangeError);
}

// padStart/padEnd take the same shape of reentrant arguments, but their
// coercions (`js_number_coerce` for maxLength, `js_string_pad_fill` for the
// fill) are emitted by codegen BEFORE the receiver handle is re-read, so no
// user code runs inside the runtime helper while its payload is borrowed.
// Pin that ordering here so a future move of either coercion into the helper
// reintroduces #8427's window with a test already watching.
let badPadStart = 0;
let badPadEnd = 0;
for (let i = 0; i < 3; i++) {
  const subject = ["pad", "me", String(i % 10)].join("");
  const started = subject.padStart(
    {
      valueOf() {
        churn();
        return 12;
      },
    } as unknown as number,
    {
      toString() {
        churn();
        return "-";
      },
    } as unknown as string,
  );
  const ended = subject.padEnd(
    {
      valueOf() {
        churn();
        return 12;
      },
    } as unknown as number,
    {
      toString() {
        churn();
        return "+";
      },
    } as unknown as string,
  );
  // Literal expectations: the receiver is 6 units, so 6 fill units are added.
  // (Spelling these out keeps the pad assertions independent of `repeat`.)
  if (started !== "------" + subject) badPadStart++;
  if (ended !== subject + "++++++") badPadEnd++;
}
console.log("padStart under reentrant args:", badPadStart === 0);
console.log("padEnd under reentrant args:", badPadEnd === 0);
