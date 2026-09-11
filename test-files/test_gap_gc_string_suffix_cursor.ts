// #10061: the source remains rooted after its original binding is overwritten.
// parity-env: PERRY_GC_SCHEDULE_SEED=10061 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1
function parse(): number {
  let source = "ä中😀Ö".repeat(20);
  let s = source;
  source = "gone";
  let hash = 0;
  while (s.length) {
    const garbage = [s.length, hash, { value: hash }];
    hash = (hash * 31 + s.charCodeAt(0) + garbage[0]) % 1000000007;
    s = s.slice(1);
  }
  return hash;
}
for (let i = 0; i < 5; i++) console.log(parse());

// A retained substring owns its bytes after both the source and other suffixes die.
function keep(): string {
  let source = "ä中😀Ö".repeat(30);
  const kept = source.slice(3, 6);
  source = "released";
  for (let i = 0; i < 50; i++) {
    const garbage = ("noise" + i).repeat(100);
    if (garbage.length < 100) throw new Error("invalid allocation");
  }
  return kept;
}
console.log(JSON.stringify(keep()));
