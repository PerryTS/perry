// `.then` on a promise the host created (an async function's result) is a
// method of a plain JS object, not of a Perry class or UI widget. The
// mem_call fallback must call the object's own method, wrapping each Perry
// closure argument as a JS function; otherwise it returns undefined and the
// callback never runs, with no error.
async function make(): Promise<number> {
  return 41;
}

make().then((v) => {
  console.log("then " + (v + 1));
});
console.log("sync done");
