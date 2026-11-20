// #11297 x #11250: a class EXPRESSION in a function body keeps its captures
// in the class environment (guarded: the first evaluation owns the
// environment slots, later ones read their own capture array). When such a
// class closes over a `for (let …)` head binding, a refresh that runs outside
// the loop body (a write to another capture, the loop head, a `return`) must
// keep each evaluation's own `i` — in the environment slots as well as in the
// class object's capture array. Every function runs twice: the first call's
// first class is the environment owner, the second call's classes are all
// non-owner evaluations.

function writeAfterLoop(): string {
  const classes: Array<new () => { get(): string }> = [];
  let x = 0;
  for (let i = 0; i < 3; i++) {
    classes.push(
      class {
        get(): string {
          return i + ":" + x;
        }
      },
    );
  }
  x = 5;
  return classes.map((C) => new C().get()).join(",");
}
console.log("write after loop:", writeAfterLoop(), writeAfterLoop());

function headWritesOther(): string {
  const classes: Array<{ s(): string }> = [];
  let x = 0;
  for (let i = 0; i < 3; i++, x++) {
    classes.push(
      class {
        static s(): string {
          return i + ":" + x;
        }
      },
    );
  }
  return classes.map((C) => C.s()).join(",");
}
console.log("head writes other, static:", headWritesOther(), headWritesOther());

// A single iteration: the last class IS the environment owner, so the
// post-loop refresh republishes into the slots its members read directly.
function ownerMemberWrite(): string {
  let x = 0;
  let K: any;
  for (let i = 0; i < 1; i++) {
    K = class {
      bump(): void {
        i += 10;
      }
      get(): string {
        return i + ":" + x;
      }
    };
  }
  new K().bump();
  x = 5;
  return new K().get();
}
console.log("owner, member write:", ownerMemberWrite(), ownerMemberWrite());

function memberWrites(): string {
  let x = 0;
  const ks: any[] = [];
  for (let i = 0; i < 3; i++) {
    ks.push(
      class {
        bump(): void {
          i += 10;
        }
        get(): string {
          return i + ":" + x;
        }
      },
    );
  }
  for (const K of ks) new K().bump();
  x = 7;
  return ks.map((K) => new K().get()).join(",");
}
console.log("member writes:", memberWrites(), memberWrites());

// No head update: only the member writes `i`, then ends the loop.
function noUpdate(): string {
  let x = 0;
  let K: any;
  for (let i = 0; i < 1; ) {
    K = class {
      bump(): void {
        i += 10;
      }
      get(): string {
        return i + ":" + x;
      }
    };
    new K().bump();
  }
  x = 5;
  return new K().get();
}
console.log("no update:", noUpdate(), noUpdate());

// A `const` declared after the loop is published by the post-loop refresh,
// which must keep the last class's own `i`. (Only the most recent class
// object is refreshed, so earlier iterations are not asserted.)
function forwardCapture(): string {
  const classes: Array<new () => { get(): string }> = [];
  for (let i = 0; i < 3; i++) {
    classes.push(
      class {
        get(): string {
          return i + ":" + later;
        }
      },
    );
  }
  const later = "L";
  return new classes[2]().get();
}
console.log("forward capture, last class:", forwardCapture(), forwardCapture());
