// Bounded regions must guard after the left GetValue and retain the single
// existing try/iterator handler tree. Run with regions on/off and seeded
// moving GC; compare the exact event transcript against the pinned Node.
function makeCheck(value: any): any {
  const check: any = {};
  check.value = value;
  return check;
}

function numericChecks(check: any, left: any): string {
  let out = "";
  for (let i = 0; i < 4; i++) {
    try {
      out += String(left < check.value) + ",";
      out += String(left <= check.value) + ",";
      out += String(left > check.value) + ",";
      out += String(left >= check.value) + ";";
      out += String(check.value < 166) + ",";
      out += String(check.value <= 166) + ",";
      out += String(check.value > 166) + ",";
      out += String(check.value >= 166) + "|";
    } catch (e) { out += "caught|"; }
  }
  return out;
}
for (const n of [2, NaN, -0, Infinity, -Infinity]) {
  console.log("number", String(n), numericChecks(makeCheck(n), 1));
}
for (const n of [true, "2", undefined, null, { valueOf() { return 2; } }]) {
  console.log("refusal", typeof n, numericChecks(makeCheck(n), 1));
}
const spill: any = {};
for (let i = 0; i < 40; i++) spill["key" + i] = i;
spill.value = 2;
console.log("spill", numericChecks(spill, 1));
console.log("special-function", numericChecks(makeCheck(function() { return 2; }), 1));

function leftChangesRight(mode: string): void {
  const events: string[] = [];
  const check = makeCheck(2);
  const input: any = {};
  Object.defineProperty(input, "data", {
    get() {
      events.push("left-data");
      if (mode === "string") check.value = "0";
      if (mode === "delete") delete check.value;
      if (mode === "accessor") {
        Object.defineProperty(check, "value", {
          configurable: true,
          get() { events.push("right-get"); return 0; }
        });
      }
      if (mode === "prototype") {
        delete check.value;
        Object.setPrototypeOf(check, { value: 0 });
      }
      return { get length() { events.push("left-length"); return 1; } };
    }
  });
  const values: boolean[] = [];
  for (let i = 0; i < 3; i++) {
    try { values.push(input.data.length < check.value); }
    catch (e) { events.push("caught"); }
  }
  console.log("mutation", mode, values.join(","), events.join(","));
}
for (const mode of ["none", "string", "delete", "accessor", "prototype"]) leftChangesRight(mode);

function coercionOrder(): void {
  const events: string[] = [];
  const input: any = {};
  const check: any = {};
  Object.defineProperty(input, "length", { get() {
    events.push("left-get");
    return { valueOf() { events.push("left-valueOf"); return 1; } };
  }});
  Object.defineProperty(check, "value", { get() {
    events.push("right-get");
    return { valueOf() { events.push("right-valueOf"); return 2; } };
  }});
  for (let i = 0; i < 2; i++) {
    try { console.log("coerce", input.length < check.value); }
    catch (e) { events.push("caught"); }
  }
  console.log("order", events.join(","));
}
coercionOrder();

function iteratorClose(mode: string): void {
  const events: string[] = [];
  const iterable: any = {
    [Symbol.iterator]() {
      let count = 0;
      return {
        next() {
          events.push("next");
          if (mode === "next-throw") throw "next-error";
          const done = count++ === 2;
          return {
            get done() { events.push("done"); if (mode === "done-throw") throw "done-error"; return done; },
            get value() { events.push("value"); if (mode === "value-throw") throw "value-error"; return makeCheck(2); }
          };
        },
        get return() {
          events.push("return-get");
          if (mode === "getter-throw") throw "close-get";
          return function() {
            events.push("return-call");
            if (mode === "call-throw") throw "close-call";
            if (mode === "return-nonobject") return 1;
            return {};
          };
        }
      };
    }
  };
  try {
    for (const check of iterable) {
      try {
        events.push(String(check.value >= 1));
        if (mode === "continue") continue;
        if (mode === "break" || mode === "return-nonobject") break;
        throw "body";
      } finally { events.push("finally"); }
    }
  } catch (e) { events.push(e instanceof TypeError ? "TypeError" : String(e)); }
  console.log("close", mode, events.join(","));
}
for (const mode of ["body", "getter-throw", "call-throw", "continue", "break", "next-throw", "done-throw", "value-throw", "return-nonobject"]) iteratorClose(mode);

function iteratorReturn(): string {
  const events: string[] = [];
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: false, value: makeCheck(2) }; },
    return() { events.push("return"); return {}; }
  }; } };
  function consume(): void {
    for (const check of iterable) {
      try { events.push(String(check.value > 1)); return; }
      finally { events.push("finally"); }
    }
  }
  consume();
  return events.join(",");
}
console.log("close-return", iteratorReturn());

function capturedReceiver(): void {
  let check: any = makeCheck(2);
  const callback = () => { check = makeCheck(0); };
  const out: boolean[] = [];
  for (let i = 0; i < 2; i++) {
    try { callback(); out.push(check.value >= 1); }
    catch (e) { out.push(true); }
  }
  console.log("captured", out.join(","));
}
capturedReceiver();
