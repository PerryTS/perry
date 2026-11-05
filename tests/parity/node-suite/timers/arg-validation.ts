// Issue #2013 — `setTimeout`/`setInterval`/`setImmediate` reject a
// non-callable first argument with `TypeError [ERR_INVALID_ARG_TYPE]`.
// Perry pre-fix silently scheduled a timer whose downstream dispatch
// would deref the unboxed-as-pointer value and segfault. Each probe
// prints the thrown error's `.code` and `.name`; Node and Perry must
// produce the exact same lines.

function probe(label: string, fn: () => any) {
  try {
    const r = fn();
    // Schedule-by-id values — clear if the call accidentally
    // succeeded — so an unintentional miss doesn't queue a real timer.
    if (typeof r === "object" && r) {
      try {
        clearTimeout(r as any);
      } catch {
        /* ignore */
      }
    }
    console.log(label, "no-throw");
  } catch (e: any) {
    console.log(label, e.name, e.code);
  }
}

// Bare-string callback — the issue's exact failing shape.
probe("setTimeout('abc',0)", () => setTimeout("abc" as any, 0));
probe("setTimeout({},0)", () => setTimeout({} as any, 0));
probe("setTimeout(null,0)", () => setTimeout(null as any, 0));
probe("setTimeout(123,0)", () => setTimeout(123 as any, 0));

probe("setInterval('abc',0)", () => setInterval("abc" as any, 0));
probe("setInterval({},0)", () => setInterval({} as any, 0));
probe("setInterval(null,0)", () => setInterval(null as any, 0));

probe("setImmediate('abc')", () => setImmediate("abc" as any));
probe("setImmediate({})", () => setImmediate({} as any));
probe("setImmediate(null)", () => setImmediate(null as any));

// Trailing-arg forms (#665) also fall through the same validator.
probe("setTimeout('abc',0,1,2)", () => setTimeout("abc" as any, 0, 1, 2));
probe("setInterval('abc',0,1,2)", () => setInterval("abc" as any, 0, 1, 2));
probe("setImmediate('abc',1,2)", () => setImmediate("abc" as any, 1, 2));
