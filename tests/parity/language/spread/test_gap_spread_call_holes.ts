// Holes spread as undefined; an inherited index property is read through the hole.
function f(a, b, c, d) { return [a, b, c, d].map(String).join(","); }
class V { set(a, b, c, d) { return [a, b, c, d].map(String).join(","); } }
const h = [1, , 3];
console.log(f(...h, 9), new V().set(...h, 9), f(9, ...h), new V().set(9, ...h));
// DEC 250: a program cannot add an index to Array.prototype; node prints the inherited reads.
try {
  Array.prototype[1] = "P";
  console.log(f(...h, 9), new V().set(...h, 9));
} catch (e) { console.log("DEC 250: " + (e as Error).message); }
delete Array.prototype[1];
console.log(f(...h, 9));
