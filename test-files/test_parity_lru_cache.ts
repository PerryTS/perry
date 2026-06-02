// Behavioral parity test for lru-cache (perry-stdlib).
//
// All operations use fixed strings so the printed state is deterministic.

import { LRUCache } from "lru-cache";

const cache = new LRUCache<string, string>({ max: 3 });

// ── Basic set / get / has / size ──
cache.set("a", "1");
cache.set("b", "2");
cache.set("c", "3");
console.log("size:", cache.size);
console.log("has a:", cache.has("a"));
console.log("get a:", cache.get("a"));
console.log("get missing:", cache.get("missing"));

// peek doesn't promote — touch "b" via get first so insertion order is
// a (LRU after get), c, b — then peek "a" without bumping it.
cache.get("b");
console.log("peek a:", cache.peek("a"));

// ── Eviction: adding a 4th entry evicts the LRU one ──
cache.set("d", "4"); // evicts oldest non-touched → "a"
console.log("after eviction size:", cache.size);
console.log("has a after evict:", cache.has("a"));
console.log("has b after evict:", cache.has("b"));
console.log("has c after evict:", cache.has("c"));
console.log("has d after evict:", cache.has("d"));

// ── delete / clear ──
console.log("delete d:", cache.delete("d"));
console.log("delete d again:", cache.delete("d"));
console.log("size after delete:", cache.size);

cache.clear();
console.log("size after clear:", cache.size);
console.log("has b after clear:", cache.has("b"));

/*
@covers
crates/perry-stdlib/src/lru_cache.rs:
  - js_lru_cache_clear
  - js_lru_cache_delete
  - js_lru_cache_get
  - js_lru_cache_has
  - js_lru_cache_new
  - js_lru_cache_peek
  - js_lru_cache_set
  - js_lru_cache_size
*/
