try { console.log("early new", new C()); } catch(e) { console.log("early new", e.name); }
try { console.log("early typeof", typeof C); } catch(e) { console.log("early typeof", e.name); }
const C = class {};
console.log("name", C.name);
console.log("after", new C() instanceof C, typeof C);
const D = class Named {};
console.log("named", D.name, typeof Named);
function local() {
  try { console.log(typeof L); } catch(e) { console.log("local", e.name); }
  const L = class {};
  console.log("local name", L.name);
}
local();
