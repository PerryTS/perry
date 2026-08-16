// An async request-handling pipeline: the shape of a small service.
// Batches of "requests" flow through validate -> enrich -> aggregate, with
// awaits between stages, Promise.all fan-out per batch, a Map-backed index,
// template-literal log lines, and a timer yield at each batch boundary.
//
// Deliberately covers four things the benchmark corpus never touched:
// async/await + promise combinators, Map/Set, template literals, and
// closures created inside loops (a documented weak spot in the
// async-to-generator transform: per-iteration `let` bindings collapsing).
//
// Fully deterministic: no clock reads, no randomness, fixed inputs.

type Req = { id: number; user: string; kind: string; amount: number };
type Ok = { id: number; user: string; amount: number; note: string };

const KINDS: string[] = ["order", "refund", "adjust"];
const USERS: string[] = ["ana", "bo", "cyd", "dee", "eli", "fay"];
const BATCHES = 1200;
const SIZE = 200;
const PROFILE_MIN_MILLISECONDS = 12000;

function makeReq(i: number): Req {
  return {
    id: i,
    user: USERS[i % USERS.length],
    kind: KINDS[i % KINDS.length],
    amount: (i % 97) + 1,
  };
}

function tick(): Promise<number> {
  return new Promise<number>((resolve) => {
    setTimeout(() => resolve(0), 0);
  });
}

async function validate(r: Req): Promise<Req> {
  if (r.amount <= 0) throw new Error(`bad amount for ${r.id}`);
  return r;
}

async function enrich(r: Req, rates: Map<string, number>): Promise<Ok> {
  const rate = rates.has(r.kind) ? (rates.get(r.kind) as number) : 1;
  const scaled = r.amount * rate;
  return {
    id: r.id,
    user: r.user,
    amount: scaled,
    note: `${r.kind}:${r.user}#${r.id}=${scaled}`,
  };
}

async function handle(r: Req, rates: Map<string, number>): Promise<Ok> {
  const v = await validate(r);
  const e = await enrich(v, rates);
  return e;
}

async function runBatch(base: number, size: number, rates: Map<string, number>): Promise<Ok[]> {
  // Closures created inside a loop, each capturing a per-iteration binding.
  const jobs: Promise<Ok>[] = [];
  for (let k = 0; k < size; k++) {
    const req = makeReq(base + k);
    jobs.push(handle(req, rates));
  }
  const done: Ok[] = await Promise.all(jobs);
  return done;
}

async function runPipeline(): Promise<string> {
  const rates = new Map<string, number>();
  rates.set("order", 3);
  rates.set("refund", 5);
  rates.set("adjust", 2);

  const seenUsers = new Set<string>();
  const perUser = new Map<string, number>();

  let total = 0;
  let noteLen = 0;
  let errors = 0;

  for (let b = 0; b < BATCHES; b++) {
    let batch: Ok[] = [];
    try {
      batch = await runBatch(b * SIZE, SIZE, rates);
    } catch (e) {
      errors = errors + 1;
      batch = [];
    }
    for (let i = 0; i < batch.length; i++) {
      const ok = batch[i];
      total = total + ok.amount;
      noteLen = noteLen + ok.note.length;
      seenUsers.add(ok.user);
      const prev = perUser.has(ok.user) ? (perUser.get(ok.user) as number) : 0;
      perUser.set(ok.user, prev + ok.amount);
    }
    if (b % 40 === 0) {
      await tick();
    }
  }

  let userSum = 0;
  for (let u = 0; u < USERS.length; u++) {
    const name = USERS[u];
    userSum = userSum + (perUser.has(name) ? (perUser.get(name) as number) : 0);
  }

  return `${total} ${noteLen} ${seenUsers.size} ${userSum} ${errors}`;
}

async function main(): Promise<void> {
  // The gate checks one pipeline's exact result, then profiles this mode. Keep
  // the sampled process busy with the same async/Map/Set/template-literal work
  // instead of padding its lifetime with a timer or a narrow spin loop. Each
  // repetition starts from the same fixed inputs and must produce the same
  // result, so profiling cannot weaken the correctness oracle. Twelve seconds
  // covers the one-second attach delay plus the eight-second sample with a
  // fixed margin, while a slow host completes its current pipeline and exits.
  const profileMode =
    process.argv.length > 2 && process.argv[2] === "--tls-budget-profile";
  const profileDeadline = profileMode ? Date.now() + PROFILE_MIN_MILLISECONDS : 0;
  let result = "";
  let repetition = 0;
  do {
    const next = await runPipeline();
    if (repetition > 0 && next !== result) {
      throw new Error(`non-deterministic pipeline result at repetition ${repetition}`);
    }
    result = next;
    repetition = repetition + 1;
  } while (profileMode && Date.now() < profileDeadline);
  if (profileMode) {
    console.error(`[tls-budget] completed=${repetition}`);
  }
  console.log(result);
}

main();
