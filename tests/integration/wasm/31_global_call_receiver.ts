// `fetch(url)` from WASM code lowers to `GlobalGet(0).fetch(url)`, a method
// call on the unresolved-global sentinel. Its receiver is globalThis; storing
// the sentinel itself (undefined) as the receiver made the call return
// undefined, so no request was made and the `.then` chain hung off undefined
// with no error. A `data:` URL keeps the case offline.
fetch("data:text/plain,hello")
  .then((r) => r.text())
  .then((t) => {
    console.log("fetched " + t);
  });
console.log("sync done");
