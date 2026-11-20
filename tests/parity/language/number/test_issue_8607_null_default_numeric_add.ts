class Registry<K, V> {
  private keys: K[] = [];
  private values: V[] = [];

  get(key: K): V | null {
    for (let i = 0; i < this.keys.length; i++) {
      if (this.keys[i] === key) return this.values[i];
    }
    return null;
  }

  set(key: K, value: V): void {
    for (let i = 0; i < this.keys.length; i++) {
      if (this.keys[i] === key) {
        this.values[i] = value;
        return;
      }
    }
    this.keys.push(key);
    this.values.push(value);
  }
}

function increment(registry: Registry<string, any>, key: string): any {
  const previous = registry.get(key);
  const next = (previous === null ? 0 : previous) + 1;
  registry.set(key, next);
  return next;
}

// Keep this Registry local and unaliased so codegen may select the
// contained-receiver array-field-cache clone for get/set. The calls above use
// an aliased parameter and therefore exercise the ordinary guarded clone.
function containedCounter(): number | null {
  const counts = new Registry<string, number>();
  for (let i = 0; i < 4; i++) {
    const previous = counts.get("count");
    counts.set("count", (previous === null ? 0 : previous) + 1);
  }
  return counts.get("count");
}

const missing = new Registry<string, any>();
console.log(increment(missing, "count"));
console.log(increment(missing, "count"));

const stringValue = new Registry<string, any>();
stringValue.set("count", "4");
console.log(increment(stringValue, "count"));
console.log(containedCounter());
