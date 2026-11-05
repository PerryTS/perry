// #6077 part 1 (timing): a rejection with no handler at the end of its
// microtask checkpoint is reported THERE — a `.catch` attached later, from a
// timer callback, is too late and must not suppress it. Node crashes with exit
// code 1 before the timer ever runs; Perry must do the same.
//
// Node's crash output is a V8 stack dump against the source file, so this test
// carries its own expected-output file (test-parity/expected/) instead of
// diffing Node's stderr byte-for-byte. The rejection reason is a plain string
// so the reported line is stable (no stack frames / absolute paths).
const p = Promise.reject("boom");

setTimeout(() => {
  // Never reached: the process is already gone.
  p.catch((e: any) => console.log("late catch ran:", e));
  console.log("timer ran");
}, 10);

console.log("sync end");
