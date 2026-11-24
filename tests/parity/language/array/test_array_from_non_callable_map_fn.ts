// Array.from(items, mapFn): a mapFn that is not callable is a TypeError, even for an
// empty source, in a synchronous and an async body; an undefined one maps nothing.
const bad: any[] = [42, {}, "x", null];
function sync(): string {
  const out: string[] = [];
  for (const f of bad) {
    try {
      out.push(JSON.stringify(Array.from([1, 2], f)));
    } catch (e: any) {
      out.push((e instanceof TypeError) + ":" + e.name);
    }
  }
  try {
    out.push(JSON.stringify(Array.from([], bad[0])));
  } catch (e: any) {
    out.push((e instanceof TypeError) + ":" + e.name);
  }
  out.push(JSON.stringify(Array.from([1, 2], undefined)));
  return out.join(" | ");
}
async function inAsync(): Promise<string> {
  const src = await Promise.resolve([1, 2]);
  try {
    return JSON.stringify(Array.from(src, bad[0]));
  } catch (e: any) {
    return (e instanceof TypeError) + ":" + e.name;
  }
}
console.log(sync());
inAsync().then((v) => console.log(v), (e) => console.log("rejected " + e.message));
