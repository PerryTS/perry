// A config-driven record-processing pipeline with generic containers,
// closure-valued stages, and data crossing a dynamic JSON boundary.

class Registry<K, V> {
  private keys: K[] = [];
  private vals: V[] = [];

  set(k: K, v: V): void {
    for (let i = 0; i < this.keys.length; i++) {
      if (this.keys[i] === k) {
        this.vals[i] = v;
        return;
      }
    }
    this.keys.push(k);
    this.vals.push(v);
  }

  get(k: K): V | null {
    for (let i = 0; i < this.keys.length; i++) {
      if (this.keys[i] === k) return this.vals[i];
    }
    return null;
  }

  size(): number {
    return this.keys.length;
  }
}

function identity<T>(x: T): T {
  return x;
}

type Record = { id: number; kind: string; amount: number; tag: string };
type Stage = (r: Record) => Record;

const CONFIG = '{"scale":3,"offset":7,"kinds":["alpha","beta","gamma"],"limit":900}';

function makeScale(factor: number): Stage {
  return (r: Record) => ({ id: r.id, kind: r.kind, amount: r.amount * factor, tag: r.tag });
}

function makeOffset(delta: number): Stage {
  return (r: Record) => ({ id: r.id, kind: r.kind, amount: r.amount + delta, tag: r.tag });
}

function makeTagger(prefix: string): Stage {
  return (r: Record) => ({ id: r.id, kind: r.kind, amount: r.amount, tag: prefix + r.kind });
}

function main(): void {
  const cfg: any = JSON.parse(CONFIG);
  const scale: number = cfg.scale;
  const offset: number = cfg.offset;
  const limit: number = cfg.limit;
  const kinds: string[] = cfg.kinds;

  const stages: Stage[] = [makeScale(scale), makeOffset(offset), makeTagger("t:")];
  const stats = new Registry<Stage, number>();
  for (let i = 0; i < stages.length; i++) {
    stats.set(stages[i], 0);
  }

  const byKind = new Registry<string, number>();
  const idf = identity;
  let checksum = 0;

  for (let round = 0; round < 400; round++) {
    for (let i = 0; i < limit; i++) {
      const kind = kinds[i % kinds.length];
      let rec: Record = { id: i, kind: kind, amount: i % 97, tag: "" };
      for (let s = 0; s < stages.length; s++) {
        const stage = stages[s];
        rec = stage(rec);
        const prev = stats.get(stage);
        stats.set(stage, (prev === null ? 0 : prev) + 1);
      }
      const seen = byKind.get(rec.kind);
      byKind.set(rec.kind, (seen === null ? 0 : seen) + 1);
      checksum = checksum + idf(rec.amount) + rec.tag.length;
    }
  }

  console.log(checksum + " " + stats.size() + " " + byKind.size());
}

main();
