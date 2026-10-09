// A for-of over an array must stop keeping its source reachable once the
// loop completes, by exhaustion, break or throw, even while the frame that ran
// it stays live. Each source is reachable only through the loop; the frame
// then collects and asks the source's WeakRef. Node cannot force a collection
// without --expose-gc, so there the released check is vacuous.
declare function gc(): void;
const canCollect = typeof gc === "function";

const box: { [k: string]: any[] | undefined } = {};
const probes: { [k: string]: WeakRef<any[]> } = {};
for (const kind of ["normal", "break", "throw", "labeled"]) {
  const source: any[] = [];
  for (let i = 0; i < 2000; i++) source.push({ i, kind });
  box[kind] = source;
  probes[kind] = new WeakRef(source);
}

function take(kind: string): any[] {
  const source = box[kind]!;
  box[kind] = undefined;
  return source;
}

function churn() {
  let junk: any[] = [];
  for (let i = 0; i < 20000; i++) {
    junk.push({ i });
    if (junk.length > 100) junk = [];
  }
}

function released(kind: string): boolean {
  churn();
  if (canCollect) gc();
  return canCollect ? probes[kind].deref() === undefined : true;
}

function consume(kind: string): string {
  let sum = 0;
  if (kind === "normal") {
    for (const v of take(kind)) sum += v.i;
  } else if (kind === "break") {
    for (const v of take(kind)) {
      if (v.i === 7) break;
      sum += v.i;
    }
  } else if (kind === "throw") {
    try {
      for (const v of take(kind)) {
        if (v.i === 11) throw new Error("stop");
        sum += v.i;
      }
    } catch (e) {
      sum += 1000;
    }
  } else {
    outer: for (let round = 0; round < 3; round++) {
      for (const v of take(kind)) {
        if (v.i === 3) break outer;
        sum += v.i;
      }
    }
  }
  // This frame is still live and the loop is done.
  return kind + " " + sum + " released: " + released(kind);
}

async function main() {
  await new Promise((r) => setTimeout(r, 0));
  console.log(consume("normal"));
  console.log(consume("break"));
  console.log(consume("throw"));
  console.log(consume("labeled"));
}
main();
