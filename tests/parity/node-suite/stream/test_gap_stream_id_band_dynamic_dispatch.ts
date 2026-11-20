// gscmaster request-12 SIGSEGV: Web Streams handle ids are raw numeric f64s
// allocated from 0x100000 (one shared counter across the five stream
// registries). A dynamic method call on a stream handle — e.g. the
// `@@asyncIterator` -> bound `values` re-dispatch a `for await` performs —
// reached the URLSearchParams fast-path in `js_native_call_method`, which
// reinterpreted the receiver double's low 48 bits as a heap address. For a
// stream id `0x100000 + k` those bits decode to `k * 2^32`; once k >= 512
// the address crosses the macOS 2 TB heap floor, passes the plausibility
// check, and the shape probe dereferences unmapped memory (on Linux the
// floor is 0x1000, so low ids probe low memory immediately). Requests 1-11
// of a Next.js app each burn ~48 ids; request 12's render stream was the
// first with k >= 512.

async function main() {
  const decoder = new TextDecoder();

  // A plain number receiver with a URLSearchParams-list method name must
  // throw TypeError, not have its double bits probed as a pointer.
  // 1049102.0 = bits 0x4130_020E_0000_0000; low 48 bits = 0x20E00000000
  // (2.26 TB) — the exact crashing "address". Run before any stream exists
  // so the value cannot alias a live stream id.
  const n: any = 1049102;
  try {
    n.entries();
    console.log("number entries: no throw");
  } catch (e) {
    console.log("number entries throws TypeError:", e instanceof TypeError);
  }

  // Zero and booleans share dangerous bit shapes (top16 == 0 → "raw
  // pointer" null; 0x7FFC payloads 3/4 → tiny "pointers") — all three
  // iterator names must throw TypeError, matching Node (#6599 review).
  // Direct `.entries()` syntax so the #597 any-typed fold fires.
  const zero: any = 0;
  const t: any = true;
  const f: any = false;
  const check = (label: string, fn: () => void) => {
    try {
      fn();
      console.log(`${label}: no throw`);
    } catch (e) {
      console.log(`${label} throws TypeError:`, e instanceof TypeError);
    }
  };
  check("0.entries()", () => zero.entries());
  check("0.keys()", () => zero.keys());
  check("0.values()", () => zero.values());
  check("true.entries()", () => t.entries());
  check("true.keys()", () => t.keys());
  check("true.values()", () => t.values());
  check("false.entries()", () => f.entries());
  check("false.keys()", () => f.keys());
  check("false.values()", () => f.values());

  function makeStream(i: number): ReadableStream {
    return new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode("chunk-" + i + ";"));
        controller.close();
      },
    });
  }

  // Burn stream-family ids well past the k = 512 threshold.
  const burned: ReadableStream[] = [];
  for (let i = 0; i < 700; i++) burned.push(makeStream(i));

  // Type-erased for-await: resolves @@asyncIterator to a bound `values`
  // dynamic dispatch with the numeric handle as receiver.
  const erased: any = makeStream(9999);
  let text = "";
  for await (const chunk of erased) text += decoder.decode(chunk);
  console.log("for-await:", text);

  // Direct dynamic `.values()` — the method name that collided with the
  // URLSearchParams fast-path list.
  const erased2: any = makeStream(4242);
  const iter = erased2.values();
  const first = await iter.next();
  console.log("values():", decoder.decode(first.value), first.done);
  const last = await iter.next();
  console.log("done:", last.done, last.value === undefined);
}

main();
