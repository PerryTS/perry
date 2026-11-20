// `JSON.parse` of an array text of 1 KiB or more returns a lazy JSON array
// header (`json/parse_api.rs`, `tape_route_eligible`), which is no
// `GC_TYPE_ARRAY`. Its result is no plain Array, so `a` is Boxed. Under
// PERRY_FACTS_SABOTAGE=arrlazy `JSON.parse` returns a plain Array of
// Numbers, so `a` is `Arr`, and the witness must report the header type at
// the first read.
let text = "[0";
for (let i = 1; i < 400; i++) {
  text = text + "," + i;
}
text = text + "]";
function probe(): number {
  const a = JSON.parse(text);
  let s = 0;
  for (let i = 0; i < 8; i++) {
    const k = i;
    s = s + a[k];
  }
  return s;
}
console.log(probe(), text.length >= 1024);
