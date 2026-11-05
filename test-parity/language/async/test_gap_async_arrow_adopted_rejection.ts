// A promise returned from an async function that throws after an await, and one
// adopted by an outer async arrow (a request handler wrapper), must reach the
// handler attached to it, and must not also be reported to the uncaught-exception
// hook. Each variant prints how many uncaught reports it caused.
let uncaught = 0;
process.on('uncaughtException', () => { uncaught++; });

const lookup = async (id) => (id === 'none' ? undefined : { id });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const inner = {
  throwNow: async () => { throw new Error('x'); },
  throwAfterAwaitNull: async () => { await null; throw new Error('x'); },
  throwAfterAwaitCall: async () => { const b = await lookup('none'); if (!b) throw new Error('x'); },
  throwAfterAwaitValue: async () => { const b = await lookup('b'); if (b) throw new Error('x'); },
  returnRejected: async () => Promise.reject(new Error('x')),
};
const outer = {
  direct: (f) => f(),
  asyncReturn: (f) => (async () => f())(),
  asyncReturnAwait: (f) => (async () => { return await f(); })(),
  asyncNoReturn: (f) => (async () => { await f(); })(),
};

(async () => {
  for (const [iname, f] of Object.entries(inner)) {
    for (const [oname, wrap] of Object.entries(outer)) {
      const before = uncaught;
      let outcome;
      await new Promise((resolve) => {
        wrap(f).then(() => { outcome = 'resolved'; resolve(); }, (e) => { outcome = 'rejected ' + e.message; resolve(); });
      });
      await sleep(2);
      console.log(iname, oname, outcome, 'uncaught', uncaught - before);
    }
  }
})();
