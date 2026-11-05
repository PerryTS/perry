// A function declaration is hoisted: it can run before a `let`/`const` it
// reads is initialized. That read throws a ReferenceError; it never yields a
// value. Once the declarator has run, the same call sees the initialized
// value. The read is a callee (`f()`), a string receiver (`pending.length`)
// and an object receiver (`items.length`), in a function body, in an async
// body and at module level.
function errName(fn: () => unknown): string {
  try {
    return "value:" + String(fn());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}

function inBody(): string {
  const out: string[] = [];
  out.push(errName(() => g()));
  out.push(errName(() => receive("x")));
  out.push(errName(() => count()));
  const f = () => 1;
  let pending = "";
  const items: number[] = [4, 5];
  function g() { return f() + 1; }
  function receive(s: string) { pending += s; return pending.indexOf("x") + pending.length; }
  function count() { return items.length; }
  out.push(errName(() => g()));
  out.push(errName(() => receive("ax")));
  out.push(errName(() => receive("b")));
  out.push(errName(() => count()));
  return out.join(",");
}
console.log("body", inBody());

// Direct calls of the declarations (no wrapper arrow), so a proven callee
// is called directly.
function direct(): string {
  let r = "";
  try { r += h(); } catch (e) { r += (e as Error).constructor.name; }
  try { r += ";" + len(); } catch (e) { r += ";" + (e as Error).constructor.name; }
  const k = () => 7;
  let word = "abc";
  function h() { return k(); }
  function len() { return word.length; }
  r += ";" + h() + ";" + len();
  word = "abcd";
  return r + ";" + len();
}
console.log("direct", direct());

async function inAsync(): Promise<string> {
  let r = "";
  try { r += ag(); } catch (e) { r += (e as Error).constructor.name; }
  await 0;
  const af = () => 3;
  function ag() { return af(); }
  return r + ";" + ag();
}

let moduleOut = errName(() => mg()) + "," + errName(() => mlen());
const mf = () => 2;
let mword = "hello";
function mg() { return mf() * 10; }
function mlen() { return mword.length + mword.indexOf("l"); }
moduleOut += "," + errName(() => mg()) + "," + errName(() => mlen());
console.log("module", moduleOut);
inAsync().then((s) => console.log("async", s));
