// Test: statement-level `yield* inner` must read the delegated iterator's final
// `.value` exactly once (spec `IteratorValue(innerResult)`, step 6.a.vi), even
// though the value is discarded — so a throwing `value` getter on the final
// `{ done: true }` result fires and rejects/throws (test262
// language/.../async-gen-method-static/yield-star-next-call-value-get-abrupt and
// the sync `class/.../gen-method` analogues). Perry previously skipped the read
// at statement level, so the getter never ran and execution wrongly continued
// past the `yield*`.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- sync: statement-level yield* surfaces a throwing final value getter ---
const reason = { tag: "reason" };
const syncObj: any = {
  [Symbol.iterator]() {
    return {
      next() {
        return {
          done: true,
          get value() {
            throw reason;
          },
        };
      },
    };
  },
};
function* sg() {
  yield* syncObj;
  yield "unreachable";
}
try {
  for (const _ of sg()) {
    /* drain */
  }
  console.log("sync: NO THROW (wrong)");
} catch (e) {
  console.log("sync threw:", e === reason ? "reason (correct)" : "wrong");
}

// --- async: same, in a STATIC async generator method (the #5592 cluster) ---
const asyncObj: any = {
  [Symbol.asyncIterator]() {
    return {
      next() {
        return {
          done: true,
          get value() {
            throw reason;
          },
        };
      },
    };
  },
};
class C {
  static async *gen() {
    yield* asyncObj;
    throw new Error("unreachable");
  }
}

// --- the completion value still propagates when the getter does NOT throw ---
function* innerVal() {
  yield 1;
  yield 2;
  return "RET";
}
function* letPos() {
  const x = yield* innerVal() as any;
  yield "x=" + x;
}
function* retPos() {
  return yield* innerVal() as any;
}
function drain(g: any) {
  const v: any[] = [];
  let r = g.next();
  while (!r.done) {
    v.push(r.value);
    r = g.next();
  }
  return JSON.stringify({ v, ret: r.value });
}

async function main() {
  const iter = C.gen();
  try {
    await iter.next();
    console.log("async: NO THROW (wrong)");
  } catch (e) {
    console.log("async threw:", e === reason ? "reason (correct)" : "wrong");
  }
  const r = await iter.next();
  console.log("async after:", r.done, r.value);

  console.log("let:", drain(letPos()));
  console.log("ret:", drain(retPos()));
  console.log("ALL YIELD-STAR-VALUE-GET TESTS PASSED");
}
main();
