// E18: JSON round-trip used where the class is expected.
// WPFACTS: site .j off
class Js {
  j: number = 3;
}
const parsed = JSON.parse(JSON.stringify(new Js())) as Js;
function rd(x: Js) { return x.j; }
console.log(rd(new Js()), rd(parsed), parsed instanceof Js);
