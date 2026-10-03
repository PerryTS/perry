function outerFinallyCatch(): void {
  const events: string[] = [];
  function consume(): number {
    try {
      try { events.push("return"); return 1; }
      catch (error) { events.push("inner-catch:" + error); }
      events.push("after-inner");
    } finally { events.push("outer-finally"); throw "outer-error"; }
    return 9;
  }
  try { events.push("result:" + consume()); }
  catch (error) { events.push("escaped:" + error); }
  console.log("catch-no-finally", events.join(","));
}
outerFinallyCatch();
function closeCatch(): void {
  const events: string[] = [];
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return {done:false,value:1}; },
    return() { events.push("close"); throw "close-error"; }
  }; } };
  try {
    for (const value of iterable) {
      try { events.push("body"); break; }
      catch (error) { events.push("inner-catch:" + error); break; }
      events.push("after-inner");
    }
    events.push("after-loop");
  } catch (error) { events.push("escaped:" + error); }
  console.log("close-catch-no-finally", events.join(","));
}
closeCatch();
function handledReturnOperand(): void {
  const events: string[] = [];
  let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return {done:count++ === 2,value:count}; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("operand"); throw "operand-error"; }
  function consume(): number {
    for (const value of iterable) {
      try { events.push("body:" + value); return fail(); }
      catch (error) { events.push("caught:" + error); }
      events.push("after-body");
    }
    return 9;
  }
  events.push("result:" + consume());
  console.log("handled-return-operand", events.join(","));
}
handledReturnOperand();
function handledFinallyOverride(): void {
  const events: string[] = [];
  let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return {done:count++ === 2,value:count}; },
    return() { events.push("close"); return {}; }
  }; } };
  for (const value of iterable) {
    try {
      try { events.push("body:" + value); break; }
      finally { events.push("finally"); throw "override"; }
    } catch (error) { events.push("caught:" + error); }
    events.push("after-body");
  }
  events.push("after-loop");
  console.log("handled-finally-override", events.join(","));
}
handledFinallyOverride();
function inheritedReturn(): void {
  const events: string[] = [];
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return {done:false,value:1}; },
    return() { events.push("close"); return {}; }
  }; } };
  function consume(): number {
    for (const value of iterable) {
      try { events.push("body"); return 7; }
      finally {
        events.push("finally");
        try { throw "local"; } catch (error) { events.push("caught:" + error); }
      }
    }
    return 9;
  }
  events.push("result:" + consume());
  console.log("inherited-return", events.join(","));
}
inheritedReturn();

// Both intervening catch-only scopes must be exited before outer cleanup.
function multipleCatchBoundaries(): void {
  const events: string[] = [];
  function consume(): number {
    try {
      try {
        try { events.push("return"); return 3; }
        catch (error) { events.push("inner:" + error); }
      } catch (error) { events.push("middle:" + error); }
    } finally { events.push("outer"); throw "outside"; }
    return 9;
  }
  try { events.push("result:" + consume()); }
  catch (error) { events.push("escaped:" + error); }
  console.log("multiple-catch-boundaries", events.join(","));
}
multipleCatchBoundaries();

// Each iterator has independent incoming completion; handled operands in
// the inner loop must leave both iterators running to normal exhaustion.
function nestedHandledOperand(): void {
  const events: string[] = [];
  function make(name: string): any {
    let count = 0;
    return { [Symbol.iterator]() { return {
      next() { events.push(name + ":next"); return { done: count++ === 2, value: count }; },
      return() { events.push(name + ":close"); return {}; }
    }; } };
  }
  function fail(): number { events.push("operand"); throw "handled"; }
  function consume(): number {
    for (const outer of make("outer")) {
      for (const inner of make("inner")) {
        try { return fail(); }
        catch (error) { events.push("caught:" + outer + ":" + inner); }
      }
      events.push("outer-body-done");
    }
    return 9;
  }
  events.push("result:" + consume());
  console.log("nested-handled-operand", events.join(","));
}
nestedHandledOperand();

// A switch break remains inside its try, so that catch is still live for
// the following throw. The later loop break exits it before IteratorClose.
function switchCatchRemainsLive(): void {
  const events: string[] = [];
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return {done:false,value:1}; },
    return() { events.push("close"); throw "close-error"; }
  }; } };
  try {
    for (const value of iterable) {
      try {
        switch (value) { case 1: events.push("switch"); break; }
        events.push("after-switch"); throw "body-error";
      } catch (error) { events.push("caught:" + error); break; }
    }
  } catch (error) { events.push("escaped:" + error); }
  console.log("switch-catch-remains-live", events.join(","));
}
switchCatchRemainsLive();

// A failed replacement return caught inside cleanup keeps the inherited
// pending return, including when its operand itself allocates a value.
function inheritedReturnAfterHandledOperand(): void {
  const events: string[] = [];
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return {done:false,value:1}; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): any { events.push("operand"); const x = {value:9}; throw "local"; }
  function consume(): any {
    for (const value of iterable) {
      try { events.push("body"); return {value:7}; }
      finally {
        events.push("finally");
        try { return fail(); } catch (error) { events.push("caught:" + error); }
        events.push("after-catch");
      }
    }
    return {value:9};
  }
  events.push("result:" + consume().value);
  console.log("inherited-return-handled-operand", events.join(","));
}
inheritedReturnAfterHandledOperand();
