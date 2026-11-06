// Positive subnormals can have the bit pattern of a raw low heap address.
// A class dispatch candidate must prove a boxed pointer before dereferencing.
class ValueBox {
  m(_ignored?: any): number { return 7; }
  relay(): number { return this.m(); }
}
function invoke(o: any): void {
  try { console.log(o.m()); }
  catch (e) { console.log(e instanceof TypeError); }
}
function declared(o: ValueBox): void {
  try { console.log(o.m()); }
  catch (e) { console.log(e instanceof TypeError); }
}
const effects: string[] = [];
function arg(): number { effects.push("arg"); return 0; }
function withArgument(o: ValueBox): void {
  try { console.log(o.m(arg())); }
  catch (e) { console.log(e instanceof TypeError); }
}
const real = new ValueBox();
invoke(real); declared(real); withArgument(real); console.log(real.relay());
const relay = ValueBox.prototype.relay;
for (const value of [Number.MIN_VALUE * 2097152, Number.MIN_VALUE * 2097153,
                     Number.MIN_VALUE * 140737488355327, 0, -0, NaN, Infinity,
                     -Infinity, 0.25, -0.25, "short", null, undefined]) {
  invoke(value); declared(value as any); withArgument(value as any);
  try { console.log(relay.call(value)); }
  catch (e) { console.log(e instanceof TypeError); }
}
console.log(effects.join(","));
