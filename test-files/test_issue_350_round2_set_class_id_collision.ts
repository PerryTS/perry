// Issue #350 round 2: `a.componentTypeSet.has(c)` SIGSEGV'd because the
// idispatch tower for unknown-receiver method calls compared the receiver's
// "class id" against every user class implementing `has` — and a `SetHeader`
// from `std::alloc` has no GcHeader, so reading the second u32 of its layout
// returned `capacity` (defaults to 4 for `new Set()`), which happened to
// equal `WorldLike`'s class id. The dispatch then called `WorldLike.has`
// with the SetHeader as `this` and segfaulted.

class EntityIdManager {
  nextId = 1024;
  allocate(): number {
    return this.nextId++;
  }
}

class CommandBuffer {
  callback: (entityId: number, commands: unknown[]) => void;
  constructor(callback: (entityId: number, commands: unknown[]) => void) {
    this.callback = callback;
  }
}

class Archetype {
  public readonly componentTypes: number[];
  public readonly componentTypeSet: ReadonlySet<number>;

  constructor(componentTypes: number[]) {
    this.componentTypes = [...componentTypes].sort((a, b) => a - b);
    this.componentTypeSet = new Set(this.componentTypes);
  }
}

function getOrCompute<K, V>(cache: Map<K, V>, key: K, compute: () => V): V {
  let value = cache.get(key);
  if (value === undefined) {
    value = compute();
    cache.set(key, value);
  }
  return value;
}

class WorldLike {
  private entityIdManager = new EntityIdManager();
  private archetypeBySignature = new Map<string, Archetype>();
  private entityToArchetype = new Map<number, Archetype>();
  private commandBuffer = new CommandBuffer((entityId, commands) => this.executeEntityCommands(entityId, commands));

  private ensureArchetype(componentTypes: Iterable<number>): Archetype {
    const sorted = [...componentTypes].sort((a, b) => a - b);
    const key = sorted.join(",");
    return getOrCompute(this.archetypeBySignature, key, () => new Archetype(sorted));
  }

  new_(): number {
    const entityId = this.entityIdManager.allocate();
    const a = this.ensureArchetype([]);
    this.entityToArchetype.set(entityId, a);
    return entityId;
  }

  has(id: number, c: number): boolean {
    const a = this.entityToArchetype.get(id);
    if (!a) return false;
    return a.componentTypeSet.has(c);
  }

  private executeEntityCommands(_entityId: number, _commands: unknown[]): void {}
}

const w = new WorldLike();
const e = w.new_();
console.log("entity:", e);
console.log("has 1:", w.has(e, 1));
console.log("has 0:", w.has(e, 0));
console.log("has 999:", w.has(e, 999));

// Direct check: a Set populated with values still routes Set.has correctly
// even when a user class in scope declares its own `has`.
const s = new Set<number>([10, 20, 30]);
console.log("set has 10:", s.has(10));
console.log("set has 99:", s.has(99));

// Map.get also goes through the dispatch tower (multiple user methods named
// `get` exist if any class declares one) — verify it still works after the
// guard.
const m = new Map<string, number>();
m.set("k", 42);
console.log("map get k:", m.get("k"));
console.log("map get x:", m.get("x"));
