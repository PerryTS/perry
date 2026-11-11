// `a` holds one plain Array literal of Numbers, and a string is stored at
// index 3 when the program runs with any argument vector (always). Its
// element representation is Boxed, so `a[j] ?? 0` keeps the general nullish
// test. Under PERRY_FACTS_SABOTAGE=arrelem the rep counts only the Numbers
// (`elem` f64) and the read takes the rep-keyed default, which hands a
// present slot on as a Number untested: its witness must report the string
// slot. (17 elements: no scalar replacement; the key is bounded below by the
// loop test only.)
function probe(): number {
  const a: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];
  if (process.argv.length > 0) {
    a[3] = "s";
  }
  let n = 0;
  let j = 16;
  while (j >= 0 && n < 100000) {
    const v = a[j] ?? 0;
    n = n + (typeof v === "number" ? v : 100);
    j--;
  }
  return n;
}
console.log(probe());
