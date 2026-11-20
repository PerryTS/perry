// Indirect eval of a const source replaces console.log: the direct call must read it.
const src = "console.log = function (s) { process.stdout.write(\"patched \" + s + \"\\n\"); }";
(0, eval)(src);
console.log("hello");
