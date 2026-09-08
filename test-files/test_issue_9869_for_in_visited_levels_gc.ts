// parity-env: PERRY_GC_SCHEDULE_SEED=9869 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1

// #9869: `for-in` defers building its shadow set until a prototype level has
// an enumerable key to filter. The retained earlier levels must remain rooted
// while key-array construction allocates and triggers a moving collection.
const ancestor = {
  own: "shadowed",
  ancestor: "visible",
};
const emptyMiddle = Object.create(ancestor);
const receiver = Object.create(emptyMiddle);
receiver.own = "receiver";

// Level zero records `receiver`; the empty middle level allocates a key array
// but keeps the shadow set deferred; the ancestor's keys then trigger a rebuild
// that reads both retained heap objects after evacuation.
const keys: string[] = [];
for (const key in receiver) {
  keys.push(key);
}

console.log(keys.join(","));
