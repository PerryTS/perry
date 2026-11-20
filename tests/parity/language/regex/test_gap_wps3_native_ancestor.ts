// WPS3 class A: an instance of a class whose ANCESTOR has a native parent is born by the
// native constructor (an exotic RegExp), so its layout is the native one, not a fixed class layout.
class Own extends RegExp {}
class Deep extends Own {}
class Deeper extends Deep {}
function show(r: any) {
  console.log(r.flags, r.source, r instanceof RegExp, "abc".split(r).join("|"), r.test("xbx"));
}
show(new Own("b"));
show(new Deep("b"));
show(new Deeper("b", "g"));
