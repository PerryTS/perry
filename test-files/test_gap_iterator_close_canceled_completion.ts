// Exact transcript regression: a captured exit from finally cancels only the
// completion originating inside its target, preserving inherited outer returns.
function test0(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      while (true) { try { events.push("return"); return 1; } finally { events.push("finally"); break; } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inner-break", events.join(","));
}
test0();
function test1(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      for (let i = 0; i < 2; i++) { try { events.push("return:" + i); return 1; } finally { events.push("finally"); continue; } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inner-for-continue", events.join(","));
}
test1();
function test2(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      let i = 0; do { try { events.push("return:" + i); return 1; } finally { events.push("finally"); i++; continue; } } while (i < 2);
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inner-do-continue", events.join(","));
}
test2();
function test3(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      switch (value) { default: try { events.push("return"); return 1; } finally { events.push("finally"); break; } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("switch-break", events.join(","));
}
test3();
function test4(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      exit: { try { events.push("return"); return 1; } finally { events.push("finally"); break exit; } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("label-block-break", events.join(","));
}
test4();
function test5(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      again: for (let i = 0; i < 2; i++) { switch (i) { default: try { events.push("return:" + i); return 1; } finally { events.push("finally"); continue again; } } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("label-loop-continue", events.join(","));
}
test5();
function test6(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      first: second: while (true) { try { events.push("return"); return 1; } finally { events.push("finally"); break first; } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("label-chain-break", events.join(","));
}
test6();
function test7(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      outer: while (true) { while (true) { try { events.push("return"); return 1; } finally { events.push("finally"); break outer; } } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("nested-target-break", events.join(","));
}
test7();
function test8(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { events.push("return"); return 1; } finally { events.push("finally"); while (true) { events.push("inner"); break; } events.push("cleanup-tail"); }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inherit-loop-break", events.join(","));
}
test8();
function test9(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { events.push("return"); return 1; } finally { events.push("finally"); switch (value) { default: events.push("inner"); break; } events.push("cleanup-tail"); }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inherit-switch-break", events.join(","));
}
test9();
function test10(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { events.push("return"); return 1; } finally { events.push("finally"); first: second: for (let i = 0; i < 2; i++) { events.push("inner:" + i); continue second; } events.push("cleanup-tail"); }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inherit-label-continue", events.join(","));
}
test10();
function test11(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { events.push("return"); return 1; } finally { events.push("finally"); while (true) { try { events.push("replacement"); return 2; } finally { events.push("inner-finally"); break; } } events.push("cleanup-tail"); }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inherit-canceled-replacement", events.join(","));
}
test11();
function test12(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { events.push("return"); return 1; } finally { events.push("finally"); continue consumeLoop; }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("outer-label-continue", events.join(","));
}
test12();
function test13(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { for (let i = fail(); i < 2; i++) { events.push("unreachable"); } } catch (error) { events.push("caught:" + error); }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("for-init-throw", events.join(","));
}
test13();
function test14(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      try { events.push("return"); return 1; } finally { events.push("finally"); first: second: for (let i = 0; i < 2; i++) { events.push("inner:" + i); continue first; } events.push("cleanup-tail"); }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("inherit-chain-outer-continue", events.join(","));
}
test14();
function test15(): void {
  const events: string[] = []; let count = 0;
  const iterable: any = { [Symbol.iterator]() { return {
    next() { events.push("next"); return { done: count++ === 2, value: count }; },
    return() { events.push("close"); return {}; }
  }; } };
  function fail(): number { events.push("init"); throw "init-error"; }
  function consume(): number {
    consumeLoop: for (const value of iterable) {
      first: second: for (let i = 0; i < 2; i++) { switch (i) { default: try { events.push("return:" + i); return 1; } finally { events.push("finally"); continue first; } } }
      events.push("body-tail");
    }
    return 9;
  }
  events.push("result:" + consume()); console.log("cancel-chain-outer-continue", events.join(","));
}
test15();
