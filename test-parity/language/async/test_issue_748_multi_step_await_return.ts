// Issue #748: native-compiled multi-step await inside async closure
// dropped the explicit return value (body resolved with `0` / undefined
// and downstream awaits silently no-op'd).
//
// Root cause: when an async closure (arrow function `const h: any =
// async (...) => ...`) ran inside an outer top-level async function, the
// closure's busy-wait `await` drained microtasks via
// `js_promise_run_microtasks()`. Each `Task::AsyncStep` dispatch in the
// runner cleared `INLINE_TRAP` to empty after the step returned —
// clobbering the outer async function's
// `{trap_next, current_step: outer_step}` that
// `js_async_first_call` had installed. When the outer step resumed
// after the busy-wait, `CurrentStepClosure` (which reads
// `INLINE_TRAP.current_step` via `js_get_current_step_closure`) returned
// NULL. The outer step then queued a `Task::AsyncStep` with
// `step_closure = NULL`, which the runner's null-step short-circuit
// (promise.rs:1316) propagates straight to `next` without ever calling
// the outer step body's state-1 continuation. Symptom in the original
// repro (skelpo-shop-admin /v1/auth/signup): first DB write commits,
// the explicit `return { ok, user, account, session }` never runs, and
// the Fastify response body is the byte `0` (the propagated value).
//
// Fix: save/restore `INLINE_TRAP` around each `Task::AsyncStep` /
// `Task::Inline` dispatch in `js_promise_run_microtasks` so an outer
// activation's `current_step` survives a re-entrant microtask drain
// triggered from inside a non-transformed async closure's busy-wait
// (the async-to-generator pass skips arrow/function-expression closures
// — see crates/perry-transform/src/async_to_generator.rs:47-49).
//
// This test exercises the minimum reproducer: a top-level async
// function awaits an async closure that itself awaits a separate async
// helper that itself awaits. Pre-fix the post-await statements after
// the closure call never ran; post-fix they do, and the explicit
// return value flows through to the outer call site.

interface RowResult {
  insertId: number;
  affectedRows: number;
}

class Pool {
  private counter: number = 0;
  async exec(_sql: string): Promise<RowResult> {
    this.counter = this.counter + 1;
    await new Promise<void>((resolve) => setTimeout(() => resolve(), 1));
    return { insertId: this.counter, affectedRows: 1 };
  }
}

const pool = new Pool();

async function createUser(email: string) {
  const r = await pool.exec(`INSERT INTO users (email) VALUES ('${email}')`);
  return { id: r.insertId, email };
}

async function createAccount(name: string) {
  const r = await pool.exec(`INSERT INTO accounts (name) VALUES ('${name}')`);
  return { id: r.insertId, name };
}

async function createSession(userId: number) {
  const r = await pool.exec(`INSERT INTO sessions (userId) VALUES (${userId})`);
  return { id: r.insertId, refreshToken: "rt-" + userId.toString() };
}

// Generic-container dispatch path: this is the shape Fastify uses
// (`routes.get(path)(req, reply)`). The handler is stored as `any` in a
// Map and read back out before being awaited. The closure type and
// dispatch path are what disqualify the async-to-generator transform
// (it's a closure, not a top-level function decl).
type Handler = (req: any, reply: any) => Promise<unknown>;

class FakeServer {
  private routes: Map<string, Handler> = new Map();
  register(path: string, h: Handler) {
    this.routes.set(path, h);
  }
  async inject(path: string, body: any): Promise<string> {
    const handler = this.routes.get(path);
    if (!handler) throw new Error("404");
    const reply = {
      code: (_n: number) => {
        /* no-op */
      },
    };
    const req = { body };
    const result = await handler(req, reply);
    return JSON.stringify(result);
  }
}

const app = new FakeServer();

app.register("/v1/auth/signup", async (req: any, _reply: any) => {
  const body = req.body;
  const user = await createUser(body.email);
  const account = await createAccount(body.accountName);
  const session = await createSession(user.id);
  return {
    ok: true,
    user,
    account,
    session,
  };
});

async function main() {
  const out = await app.inject("/v1/auth/signup", {
    email: "foo@bar.com",
    accountName: "Acme",
  });
  console.log(out);
}

main();
