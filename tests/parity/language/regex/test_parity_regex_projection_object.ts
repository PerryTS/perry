// No RegExp construction: the projection must retain exact ordinary calls,
// including in a runtime built without the optional matching engine.
function run(receiver: any): void {
  let iteration = 0;
  while (iteration < 1) {
    const m = receiver.exec("argument");
    if (m !== null) console.log(m[1]);
    else console.log("null");
    iteration++;
  }
}
const receiver: any = { exec(input: string) { console.log(this === receiver, input); return ["", "ordinary"]; } };
run(receiver);
let gets = 0;
const getter: any = {};
Object.defineProperty(getter, "exec", { get() { gets++; return function(input: string) { console.log(this === getter, input); return ["", "getter"]; }; } });
run(getter); console.log(gets);
const proxied: any = {};
proxied.exec = new Proxy(function() {}, { apply(target, self, args) { console.log(self === proxied, args[0]); return ["", "proxy"]; } });
run(proxied);
const nil: any = { exec() { return null; } }; run(nil);
const primitive: any = { exec() { return "ab"; } }; run(primitive);
const noncallable: any = { exec: 5 };
try { run(noncallable); } catch (e) { console.log(e instanceof TypeError); }
