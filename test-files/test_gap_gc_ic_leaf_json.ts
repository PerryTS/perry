// #12435: run with PERRY_GC_SCHEDULE_SEED=12435,
// PERRY_GC_SCHEDULE_RATE=1, PERRY_GC_SCHEDULE_ALLOC_KB=0,
// PERRY_GC_PROTECT_FROMSPACE=1 and PERRY_GC_VERIFY_EVACUATION=1.
// The same IC sees an ordinary array, a lazy JSON array, and an own-key
// override. Keep young objects live across the property read and loop polls.
function read(receiver: any, live: any): string {
  const absent = receiver.icLeafJson;
  const ctor = receiver.constructor;
  return live.text + ":" + String(absent) + ":" + (ctor === Array);
}

let good = 0;
for (let i = 0; i < 120; i++) {
  const live = { text: "live-" + i, nested: { n: i } };
  const ordinary: any = [i];
  const lazy: any = JSON.parse('[{"text":"json","n":7},[1,2],"tail"]');
  const control: any = [i];
  control.icLeafJson = "own";
  if (read(ordinary, live) === "live-" + i + ":undefined:true") good++;
  if (read(lazy, live) === "live-" + i + ":undefined:true") good++;
  // Negative control: a named own key leaves the canonical array shape.
  if (read(control, live) === "live-" + i + ":own:true") good++;
  if (lazy[0].text === "json" && live.nested.n === i) good++;
}
console.log(good);
