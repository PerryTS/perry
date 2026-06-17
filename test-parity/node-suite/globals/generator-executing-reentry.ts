function show(label: string, action: () => unknown) {
  try {
    console.log(label, "result", JSON.stringify(action()));
  } catch (err: any) {
    console.log(label, "throw", err instanceof TypeError, err.name);
  }
}

let nextUncaughtIter: any;
function* nextUncaught() {
  nextUncaughtIter.next("reenter");
  yield "next:after";
}
nextUncaughtIter = nextUncaught();
show("next uncaught first", () => nextUncaughtIter.next());
show("next uncaught post", () => nextUncaughtIter.next());

let returnUncaughtIter: any;
function* returnUncaught() {
  returnUncaughtIter.return("reenter");
  yield "return:after";
}
returnUncaughtIter = returnUncaught();
show("return uncaught first", () => returnUncaughtIter.next());
show("return uncaught post", () => returnUncaughtIter.next());

let throwUncaughtIter: any;
function* throwUncaught() {
  throwUncaughtIter.throw("reenter");
  yield "throw:after";
}
throwUncaughtIter = throwUncaught();
show("throw uncaught first", () => throwUncaughtIter.next());
show("throw uncaught post", () => throwUncaughtIter.next());

let nextCaughtIter: any;
function* nextCaught() {
  try {
    nextCaughtIter.next("reenter");
    console.log("next caught inner no throw");
  } catch (err: any) {
    console.log("next caught inner throw", err instanceof TypeError, err.name);
  }
  yield "next:after";
}
nextCaughtIter = nextCaught();
show("next caught first", () => nextCaughtIter.next());
show("next caught post", () => nextCaughtIter.next());

let returnCaughtIter: any;
function* returnCaught() {
  try {
    returnCaughtIter.return("reenter");
    console.log("return caught inner no throw");
  } catch (err: any) {
    console.log("return caught inner throw", err instanceof TypeError, err.name);
  }
  yield "return:after";
}
returnCaughtIter = returnCaught();
show("return caught first", () => returnCaughtIter.next());
show("return caught post", () => returnCaughtIter.next());

let throwCaughtIter: any;
function* throwCaught() {
  try {
    throwCaughtIter.throw("reenter");
    console.log("throw caught inner no throw");
  } catch (err: any) {
    console.log("throw caught inner throw", err instanceof TypeError, err.name);
  }
  yield "throw:after";
}
throwCaughtIter = throwCaught();
show("throw caught first", () => throwCaughtIter.next());
show("throw caught post", () => throwCaughtIter.next());
