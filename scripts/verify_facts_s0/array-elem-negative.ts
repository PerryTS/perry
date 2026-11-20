// `a` holds one plain Array literal of Numbers, and a string is stored at
// index 3 when the program runs with any argument vector (always). Its
// element representation is Boxed. Under PERRY_FACTS_SABOTAGE=arrelem the
// rep counts only the Numbers (`elem` f64), and the witness must report the
// string slot. (17 elements: no scalar replacement.)
function probe(): number {
  const a: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];
  if (process.argv.length > 0) {
    a[3] = "s";
  }
  let n = 0;
  for (let i = 0; i < 17; i++) {
    const k = i;
    const v = a[k];
    n = n + (typeof v === "number" ? v : 100);
  }
  return n;
}
console.log(probe());
