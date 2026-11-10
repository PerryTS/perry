// An int32 number whose value equals a class id is a number, not a class.
// Compiled code names a class by its function object, so typeof, String,
// templates, concatenation, join, Object.prototype.toString, instanceof and
// method calls on an integer never ask which classes exist. An async
// function that awaits and then returns a small integer resolves with the
// compact int32 form, which used to read as the class with that id.
class A {}
class B extends A {}
class C { static make(): C { return new C(); } }
class D { pipe(): number { return 1; } }
class E {}
class F {}

async function lit(): Promise<number> { await null; return 4; }
async function after(i: number): Promise<number> { await null; return i; }

async function main(): Promise<void> {
  const vals: any[] = [await lit()];
  for (let i = 0; i < 8; i++) vals.push(await after(i));
  const kinds: string[] = [];
  for (const v of vals) kinds.push(typeof v);
  console.log("typeof:", kinds.join(","));
  const strs: string[] = [];
  for (const v of vals) strs.push(String(v) + "|" + `${v}` + "|" + ("" + v) + "|" + [v].join());
  console.log("string:", strs.join(" "));
  console.log("join:", vals.join("-"));
  const tags: string[] = [];
  for (const v of vals) tags.push(Object.prototype.toString.call(v));
  console.log("tag:", Array.from(new Set(tags)).join(","));
  let thrown = 0;
  for (const v of vals) { try { (v as any).pipe(); } catch (e) { thrown++; } }
  console.log("pipe throws:", thrown, "of", vals.length);
  let inst = 0;
  for (const v of vals) { if (v instanceof Object || v instanceof D) inst++; }
  console.log("instanceof:", inst);
  console.log("sum:", vals.reduce((a: number, b: number) => a + b * 2, 0));
  const four = await lit();
  console.log("direct:", typeof four, String(four), four + 1, four === 4);
  lit().then((v) => console.log("then:", typeof v, `${v}`, Object.prototype.toString.call(v)));
  console.log("classes:", typeof A, typeof B, typeof C, typeof D, typeof E, typeof F, new B() instanceof A);
}

main();
