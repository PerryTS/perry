// `a` holds one plain Array literal of Numbers, and a string is stored at
// index 3 when the program runs with any argument vector (always). Its
// element representation is Boxed, so the stores below keep the guarded
// store. Under PERRY_FACTS_SABOTAGE=arrelem the rep counts only the Numbers
// (`elem` f64) and the stores take the rep-keyed store, which leans on
// every present slot holding a Number: its witness must report the string
// slot before it is overwritten. (17 elements: no scalar replacement; `j`
// starts below a length nothing bounds and is bounded below by the loop
// test only, so it is no proven index; no element of `a` is read.)
function probe(): number {
  const a: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];
  if (process.argv.length > 0) {
    a[3] = "s";
  }
  const b: number[] = [];
  for (let i = 0; i < (process.argv.length > 100 ? 3 : 9); i++) b.push(i);
  let n = 0;
  for (let i = 1; i < b.length; i++) {
    let j = i - 1;
    while (j >= 0 && n < 100000) {
      a[j] = j * 2;
      n = n + j;
      j--;
    }
  }
  return n + a.length;
}
console.log(probe());
