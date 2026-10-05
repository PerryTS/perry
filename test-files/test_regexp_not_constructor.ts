// Constructor admission follows capability, including ordinary RegExp instances.
function attempt(value: any): void {
  try { new value(); console.log("accepted"); }
  catch (error: any) { console.log(error.name); }
}
class R extends RegExp {}
class C {}
function F() {}
for (const value of [/a/, new R("a"), {}, [], Object.create(null), () => {}, C, F, F.bind(null), RegExp]) {
  attempt(value);
}
