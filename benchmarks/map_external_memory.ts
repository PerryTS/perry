// Default GC pacing: each new cohort drops the previous Map.
// 80 cohorts, each with 20k long string keys and 20k object keys.
let activeMap = new Map<any, number>();
let total = 0;
for (let round = 0; round < 80; round++) {
  activeMap = new Map<any, number>();
  let sum = 0;
  for (let i = 0; i < 20000; i++) {
    const text = 'map-external-key-' + i;
    const object = { id: i };
    activeMap.set(text, i);
    activeMap.set(object, i + 1);
    if (i % 1024 === 0) sum += activeMap.get(text)! + activeMap.get(object)!;
  }
  sum += activeMap.size;
  if (round % 2 === 0) activeMap.clear();
  total += sum;
}
console.log('map-heavy', total);
