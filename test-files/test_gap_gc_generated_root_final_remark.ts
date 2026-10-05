// Generated locals and module globals are mutable roots. Incremental GC's
// final root remark, rather than a per-store shading call, discovers their
// latest values; closure captures remain ordinary heap slots with barriers.

let globalRoot: any = { i: -1, seed: "start", prior: null };

function makeWriter(seed: string) {
  let captured: any = { i: -1, seed, prior: null };

  return function writeMany(): string {
    let local: any = { i: -1, seed, prior: null };
    const maybeGc = (globalThis as any).gc;

    for (let i = 0; i < 1024; i++) {
      const next = {
        i,
        seed,
        prior: (i & 1) === 0 ? local : globalRoot,
      };
      local = next;
      captured = next;
      globalRoot = next;

      // Node normally has no exposed gc(); Perry does. Forced-evacuation runs
      // use this to make the generated roots move and be rewritten repeatedly.
      if ((i & 31) === 0 && typeof maybeGc === "function") maybeGc();
    }

    return `${local.i}:${captured.i}:${globalRoot.i}:${local.seed}`;
  };
}

console.log(makeWriter("remark")());
