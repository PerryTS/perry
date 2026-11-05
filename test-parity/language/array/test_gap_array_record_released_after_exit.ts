// A for-of over an array must run to the same sums and control flow in every
// exit kind: exhaustion, break, throw and labeled break, each from a live
// frame. Whether the source is released afterwards is a collector property,
// not an output the engines share (V8 may keep it in the frame), so that
// check lives in crates/perry/tests/array_record_released_after_exit.rs.
const box: { [k: string]: any[] | undefined } = {};
for (const kind of ["normal", "break", "throw", "labeled"]) {
  const source: any[] = [];
  for (let i = 0; i < 2000; i++) source.push({ i, kind });
  box[kind] = source;
}

function take(kind: string): any[] {
  const source = box[kind]!;
  box[kind] = undefined;
  return source;
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
  return kind + " " + sum + " taken: " + (box[kind] === undefined);
}

async function main() {
  await new Promise((r) => setTimeout(r, 0));
  console.log(consume("normal"));
  console.log(consume("break"));
  console.log(consume("throw"));
  console.log(consume("labeled"));
}
main();
