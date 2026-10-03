// IteratorClose runs after nested finallies, reads return once, and invokes
// that method with the iterator receiver without reading its .call property.
function completionCase(mode: string): void {
  const events: string[] = [];
  let count = 0;
  let reads = 0;
  const iter: any = {
    next() {
      events.push("next");
      if (mode === "next-throw") throw "next-error";
      const done = count++ === 2;
      return {
        get done() {
          events.push("done");
          if (mode === "done-throw") throw "done-error";
          return done;
        },
        get value() {
          events.push("value");
          if (mode === "value-throw") throw "value-error";
          return count;
        }
      };
    },
    get return() {
      events.push("get-return");
      reads++;
      if (mode === "break-get-throw" || mode === "throw-get-throw") throw "get-error";
      if (mode === "null") return null;
      if (mode === "undefined") return undefined;
      if (mode === "break-noncallable" || mode === "throw-noncallable") return 1;
      const method: any = function() {
        events.push(this === iter ? "receiver-ok" : "receiver-bad");
        events.push(reads === 1 ? "first-method" : "second-method");
        if (mode === "break-call-throw" || mode === "throw-call-throw") throw "call-error";
        if (mode === "break-nonobject" || mode === "throw-nonobject") return 1;
        return {};
      };
      Object.defineProperty(method, "call", {
        get() { events.push("poison-call"); throw "call-property-error"; }
      });
      return method;
    }
  };
  const iterable: any = { [Symbol.iterator]() { return iter; } };
  function result(): number {
    events.push("return-operand");
    if (mode === "return-operand-throw") throw "operand-error";
    return 17;
  }
  function consume(): number {
    for (const value of iterable) {
      try {
        events.push("body:" + value);
        if (mode === "continue") continue;
        if (mode === "return" || mode === "return-operand-throw" || mode === "return-finally-break") return result();
        if (mode.indexOf("throw-") === 0) throw "body-error";
        break;
      } finally {
        events.push("inner-finally");
        if (mode === "break-finally-continue") continue;
        if (mode === "break-finally-return") return result();
        if (mode === "return-finally-break") break;
        if (mode === "break-finally-throw") throw "finally-error";
      }
    }
    events.push("after-loop");
    return 23;
  }
  try { events.push("result:" + consume()); }
  catch (error) { events.push(error instanceof TypeError ? "TypeError" : String(error)); }
  console.log(mode, events.join(","));
}
for (const mode of ["break", "return", "continue", "null", "undefined",
  "break-get-throw", "break-call-throw", "break-noncallable", "break-nonobject",
  "throw-get-throw", "throw-call-throw", "throw-noncallable", "throw-nonobject",
  "return-operand-throw", "break-finally-continue", "break-finally-return",
  "return-finally-break", "break-finally-throw", "next-throw", "done-throw", "value-throw"]) {
  completionCase(mode);
}

function nestedCase(mode: string): void {
  const events: string[] = [];
  function iterable(name: string): any {
    let count = 0;
    return { [Symbol.iterator]() { return {
      next() { events.push(name + ":next"); return { done: count++ === 2, value: count }; },
      return() { events.push(name + ":close"); return {}; }
    }; } };
  }
  outer: for (const a of iterable("outer")) {
    for (const b of iterable("inner")) {
      try {
        events.push("body");
        if (mode === "outer-continue") continue outer;
        if (mode === "outer-break") break outer;
        // A switch break is captured locally; its continue targets this loop.
        switch (b) { case 1: break; default: continue; }
        events.push("after-switch");
      } finally { events.push("finally"); }
    }
  }
  console.log(mode, events.join(","));
}
for (const mode of ["outer-continue", "outer-break", "switch"]) nestedCase(mode);

// Close errors occur after the inner try has completed; its catch must not
// intercept them, nor may the preserved finally execute a second time.
function closeOutsideCatch(): void {
  const events: string[] = [];
  const iterable: any = { [Symbol.iterator]() { return {
    next() { return { done: false, value: 1 }; },
    return() { events.push("close"); return 1; }
  }; } };
  try {
    for (const value of iterable) {
      try { events.push("body"); break; }
      catch (error) { events.push("inner-catch"); }
      finally { events.push("finally"); }
    }
  } catch (error) { events.push(error instanceof TypeError ? "TypeError" : String(error)); }
  console.log("close-outside-catch", events.join(","));
}
closeOutsideCatch();

const topEvents: string[] = [];
const topIterable: any = { [Symbol.iterator]() { return {
  next() { return { done: false, value: 1 }; },
  get return() { topEvents.push("get-return"); return function() { topEvents.push("close"); return {}; }; }
}; } };
for (const value of topIterable) {
  try { topEvents.push("body"); break; }
  finally { topEvents.push("finally"); }
}
console.log("top-level", topEvents.join(","));
