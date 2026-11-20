// `inline_finally_into_returns` clones the `finally` body before each `return`
// that escapes a try. It found closures by walking a statement's expressions —
// but that walk ALSO descended into nested statement lists, which `process_stmts`
// recurses into itself. A closure nested N blocks deep was therefore processed
// N+1 times, and each pass inlined another copy of its `finally`:
//
//   let __ret1 = await f(); g(); let __ret2 = __ret1; g(); return __ret2;
//
// So `async () => { try { return await f() } finally { g() } }` sitting inside an
// `if` ran `g()` TWICE. Next.js closes a per-request CloseController in exactly
// such a finally, so every page render threw "Cannot close a CloseController
// multiple times" and streamed an empty body.

let runs = 0;
async function body(): Promise<string> {
  return "BODY";
}

// control: a top-level async function (never nested) — always ran once
async function topLevel(): Promise<string> {
  try {
    return await body();
  } finally {
    runs++;
  }
}

// the closure lives inside an `if` block
function inIf(flag: boolean): Promise<string> | null {
  if (flag) {
    const f = async () => {
      try {
        return await body();
      } finally {
        runs++;
      }
    };
    return f();
  }
  return null;
}

// two levels of nesting
function inIfInIf(flag: boolean): Promise<string> | null {
  if (flag) {
    if (flag) {
      const g = async () => {
        try {
          return await body();
        } finally {
          runs++;
        }
      };
      return g();
    }
  }
  return null;
}

// nested inside a loop and a try
function inLoopInTry(): string {
  let out = "";
  try {
    for (let i = 0; i < 2; i++) {
      const h = () => {
        try {
          return "v" + i;
        } finally {
          runs++;
        }
      };
      out += h();
    }
  } finally {
    out += "|outer";
  }
  return out;
}

(async () => {
  runs = 0;
  await topLevel();
  console.log("top-level    :", runs);

  runs = 0;
  await inIf(true);
  console.log("in if        :", runs);

  runs = 0;
  await inIfInIf(true);
  console.log("in if x2     :", runs);

  runs = 0;
  const loop = inLoopInTry();
  console.log("in loop/try  :", runs, loop);

  // the pass must still do its job: finally runs on every abrupt completion,
  // and the return operand is evaluated BEFORE the finally body (#536).
  const order: string[] = [];
  const withOperand = () => {
    try {
      return (order.push("operand"), "ret");
    } finally {
      order.push("finally");
    }
  };
  console.log("ordering     :", withOperand(), order.join(","));

  const log: string[] = [];
  for (let i = 0; i < 3; i++) {
    try {
      if (i === 1) break;
    } finally {
      log.push("b" + i);
    }
  }
  for (let i = 0; i < 2; i++) {
    try {
      continue;
    } finally {
      log.push("c" + i);
    }
  }
  const caught = (() => {
    try {
      throw new Error("x");
    } catch {
      return "caught";
    } finally {
      log.push("t");
    }
  })();
  console.log("abrupt paths :", caught, log.join(","));
})();
