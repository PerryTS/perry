function truth(flag: boolean) {
  try { return flag ? "yes" : "no"; }
  finally { console.log("finally"); }
}
for (const value of ["nonempty", 0, {}, null] as any[]) {
  console.log(truth(value as any));
}
function cleanupThrows() {
  const input: any = [1];
  input[Symbol.iterator] = function() {
    return {
      next() { return {value: 1, done: false}; },
      return() { console.log("close"); throw "close-error"; }
    };
  };
  try {
    for (const v of input) {
      try { if (v === 1) break; }
      finally { console.log("inner"); }
    }
  } finally { console.log("outer"); }
}
try { cleanupThrows(); }
catch (e) { console.log("caught", e); }
