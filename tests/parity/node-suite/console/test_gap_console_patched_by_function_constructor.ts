// new Function code replaces console.log through its parameter: the direct call must read it.
const f: any = new Function("c", "c.log = function (s) { process.stdout.write(\"patched \" + s + \"\\n\"); };");
f(console);
console.log("hello");
