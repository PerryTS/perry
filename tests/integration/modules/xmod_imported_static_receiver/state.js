class PerOwner {
  #factory;
  #values = new WeakMap();
  constructor(factory) { this.#factory = factory; }
  of(owner) {
    const existing = this.#values.get(owner);
    if (existing !== undefined) return existing;
    const value = this.#factory();
    this.#values.set(owner, value);
    return value;
  }
}
class Store { perSource = new Map(); }
const Settings = new PerOwner(() => new Store());
const host = {};
export function context() { return { host }; }
export { Settings };
