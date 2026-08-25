export class Foreign {
  id: number;
  components: number[];

  constructor(id: number) {
    this.id = id;
    this.components = [1, 2, 3];
  }
}

// These mutations deliberately live across an import/re-export boundary.
// The caller module may still emit an argument clone, but the runtime exact-
// shape/descriptor guard must send the changed object to its generic body.
export function reshape(value: any): void {
  const id = value.id;
  delete value.id;
  value.id = id + 1;
}

// If `value` aliases an argument being read by an exact-shape clone, the
// inserted field changes its offset after the clone's entry guard.
export function reshapeAliasedId(value: any): void {
  const id = value.id;
  delete value.id;
  value.aliasPadding = 40;
  value.id = id + 1;
}

export function installIdAccessor(value: any): void {
  const id = value.id;
  Object.defineProperty(value, "id", {
    configurable: true,
    enumerable: true,
    get() {
      return id + 2;
    },
  });
}

export function makeProxy(value: any, counter: any): any {
  return new Proxy(value, {
    get(target: any, key: any): any {
      counter.hits++;
      return target[key];
    },
  });
}
