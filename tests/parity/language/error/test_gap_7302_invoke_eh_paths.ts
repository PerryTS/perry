// Every structural try path: caught, finally-on-both-edges, nested rethrow,
// return-inside-try, catch-with-finally fail path, loop counter mutation.
function basic(): string {
  try {
    throw new Error("boom");
  } catch (e) {
    return "caught:" + (e as Error).message;
  }
}
console.log(basic());

function finBoth(x: boolean): string {
  let log = "";
  try {
    log += "T";
    if (x) throw new Error("x");
    log += "t";
  } catch {
    log += "C";
  } finally {
    log += "F";
  }
  return log;
}
console.log(finBoth(true), finBoth(false));

function nestedRethrow(): string {
  try {
    try {
      throw new Error("inner");
    } catch (e) {
      throw new Error("outer:" + (e as Error).message);
    }
  } catch (e2) {
    return (e2 as Error).message;
  }
}
console.log(nestedRethrow());

function retInTry(): string {
  let log = "";
  try {
    log += "T";
    return log + "|ret";
  } finally {
    log += "F";
    console.log("finally-ran:" + log);
  }
}
console.log(retInTry());

function catchFinallyFail(): string {
  try {
    try {
      throw new Error("a");
    } catch {
      throw new Error("b");
    } finally {
      console.log("cf-finally");
    }
  } catch (e) {
    return "outer-caught:" + (e as Error).message;
  }
}
console.log(catchFinallyFail());

function volatileHazard(): number {
  let acc = 0;
  for (let i = 0; i < 100; i++) {
    acc += i;
  }
  try {
    acc = 4141;
    acc += 800;
    throw new Error("bump");
  } catch {
    acc += 1;
  }
  return acc;
}
console.log(volatileHazard());

function tryFinallyRepropagates(): string {
  try {
    try {
      throw new Error("keep-me");
    } finally {
      console.log("tf-finally");
    }
  } catch (e) {
    return "repropagated:" + (e as Error).message;
  }
}
console.log(tryFinallyRepropagates());
