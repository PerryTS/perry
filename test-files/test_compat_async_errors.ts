// Behavioral parity coverage for promises, async, errors, exceptions,
// and microtask ordering. Output is deterministic and byte-comparable.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// Promise: resolve / reject / then / catch / finally.
async function main(): Promise<void> {
  const resolved = await Promise.resolve(42);
  line("resolve", resolved);

  try {
    await Promise.reject(new Error("boom"));
    line("reject", "unreachable");
  } catch (err) {
    line("reject", (err as Error).message);
  }

  let finallyHit = false;
  const finallyResult = await Promise.resolve("done").finally(() => {
    finallyHit = true;
  });
  line("finally-hit", finallyHit);
  line("finally-pass", finallyResult);

  // Chained then.
  const chained = await Promise.resolve(1)
    .then((n) => n + 1)
    .then((n) => n * 10);
  line("chain", chained);

  // catch handler.
  const caught = await Promise.reject(new Error("err"))
    .catch((e: Error) => "caught:" + e.message);
  line("catch-handler", caught);

  // Promise.all.
  const all = await Promise.all([
    Promise.resolve(1),
    Promise.resolve(2),
    Promise.resolve(3),
  ]);
  line("all", all.join(","));

  // Promise.allSettled.
  const settled = await Promise.allSettled([
    Promise.resolve("ok"),
    Promise.reject(new Error("bad")),
    Promise.resolve("yes"),
  ]);
  const statuses = settled.map((r) => r.status).join(",");
  line("allSettled-status", statuses);
  const settledValues = settled.map((r) =>
    r.status === "fulfilled" ? r.value : (r.reason as Error).message,
  );
  line("allSettled-values", settledValues.join(","));

  // Promise.race.
  const raced = await Promise.race([
    Promise.resolve("first"),
    Promise.resolve("second"),
  ]);
  line("race", raced);

  // Promise.any.
  const any1 = await Promise.any([
    Promise.reject(new Error("e1")),
    Promise.resolve("yes"),
    Promise.reject(new Error("e2")),
  ]);
  line("any-ok", any1);

  try {
    await Promise.any([
      Promise.reject(new Error("e1")),
      Promise.reject(new Error("e2")),
    ]);
  } catch (e) {
    line("any-all-fail-name", (e as Error).name);
  }

  // Promise.withResolvers.
  const wr = Promise.withResolvers<number>();
  wr.resolve(7);
  line("withResolvers", await wr.promise);

  // Promise constructor.
  const built = await new Promise<string>((resolve) => {
    resolve("built");
  });
  line("constructor-resolve", built);

  try {
    await new Promise<string>((_resolve, reject) => {
      reject(new Error("constructor-err"));
    });
  } catch (e) {
    line("constructor-reject", (e as Error).message);
  }

  // queueMicrotask + ordering vs awaited promises.
  const order: string[] = [];
  order.push("sync-start");
  queueMicrotask(() => order.push("micro1"));
  await Promise.resolve().then(() => order.push("then1"));
  order.push("after-await");
  line("microtask-order", order.join("|"));

  // Async error catch and rethrow.
  async function fails() {
    throw new RangeError("range bad");
  }
  try {
    await fails();
  } catch (e) {
    line("err-name", (e as Error).name);
    line("err-msg", (e as Error).message);
  }

  // Error subclasses + cause + instanceof.
  const base = new Error("base", { cause: "underlying" });
  line("error-name", base.name);
  line("error-msg", base.message);
  line("error-cause", base.cause);
  line("is-error", base instanceof Error);

  const te = new TypeError("not callable");
  line("typeerror-name", te.name);
  line("typeerror-is-error", te instanceof Error);
  const re = new RangeError("out of range");
  line("rangeerror-name", re.name);
  const se = new SyntaxError("bad syntax");
  line("syntaxerror-name", se.name);
  const refe = new ReferenceError("not defined");
  line("referenceerror-name", refe.name);

  // AggregateError.
  const agg = new AggregateError(
    [new Error("e1"), new Error("e2")],
    "two errors",
  );
  line("aggregate-name", agg.name);
  line("aggregate-msg", agg.message);
  line("aggregate-count", agg.errors.length);

  // Try / catch / finally.
  let finallyRan = false;
  try {
    throw new Error("inner");
  } catch (e) {
    line("try-catch", (e as Error).message);
  } finally {
    finallyRan = true;
  }
  line("finally-ran", finallyRan);

  // Throw / rethrow.
  function rethrow(): never {
    try {
      throw new TypeError("from try");
    } catch (e) {
      throw e;
    }
  }
  try {
    rethrow();
  } catch (e) {
    line("rethrow", (e as Error).name + ":" + (e as Error).message);
  }

  // structuredClone.
  const original = { a: 1, b: { c: [1, 2, 3] } };
  const clone = structuredClone(original);
  line("structuredClone-deep", clone.b.c.join(","));
  line("structuredClone-different", clone !== original);

  // Async generator + for-await.
  async function* gen() {
    yield 1;
    yield 2;
    yield 3;
  }
  const collected: number[] = [];
  for await (const v of gen()) {
    collected.push(v);
  }
  line("async-gen", collected.join(","));

}

await main();
console.log("compat-async-errors: ok");

/*
@covers
crates/perry-runtime/src/promise.rs:
  - js_assimilate_thenable
  - js_async_step_chain
  - js_async_step_done
  - js_await_any_promise
  - js_is_promise
  - js_iter_result_get_done
  - js_iter_result_get_value
  - js_iter_result_set
  - js_microtasks_pending
  - js_promise_all
  - js_promise_all_settled
  - js_promise_any
  - js_promise_catch
  - js_promise_finally
  - js_promise_free
  - js_promise_new
  - js_promise_new_with_executor
  - js_promise_race
  - js_promise_reason
  - js_promise_reject
  - js_promise_rejected
  - js_promise_resolve
  - js_promise_resolve_with_promise
  - js_promise_resolved
  - js_promise_resolved_then
  - js_promise_result
  - js_promise_run_microtasks
  - js_promise_schedule_resolve
  - js_promise_state
  - js_promise_then
  - js_promise_value
  - js_promise_with_resolvers
  - js_value_is_promise
crates/perry-runtime/src/error.rs:
  - js_aggregateerror_new
  - js_error_get_cause
  - js_error_get_errors
  - js_error_get_kind
  - js_error_get_message
  - js_error_get_name
  - js_error_get_stack
  - js_error_new
  - js_error_new_with_cause
  - js_error_new_with_message
  - js_rangeerror_new
  - js_referenceerror_new
  - js_syntaxerror_new
  - js_throw_type_error_immutable_write
  - js_throw_type_error_not_a_function
  - js_throw_type_error_property_access
  - js_typeerror_new
crates/perry-runtime/src/exception.rs:
  - js_clear_exception
  - js_enter_finally
  - js_get_exception
  - js_has_exception
  - js_leave_finally
  - js_throw
  - js_try_end
  - js_try_push
*/
