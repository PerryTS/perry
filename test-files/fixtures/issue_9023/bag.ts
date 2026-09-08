export class Bag<K, V> {
 private values = new Map<K, Set<V>>();
 add(key: K, value: V) {
  let group = this.values.get(key);
  if (!group) { group = new Set<V>(); this.values.set(key, group); }
  group.add(value);
 }
 *entries(): IterableIterator<[K, V]> {
  for (const [key, group] of this.values) for (const value of group) yield [key, value];
 }
 [Symbol.iterator]() { return this.entries(); }
}
