function show(label: string, result: IteratorResult<string>) {
  console.log(label, JSON.stringify(result));
}

async function immediateTwo() {
  async function* immediateTwoGenerator() {
    console.log("immediate-two:body:start");
    const sent = yield "first";
    console.log("immediate-two:body:after-yield", sent);
    yield "second";
  }

  const it = immediateTwoGenerator();
  const p1 = it.next("ignored");
  const p2 = it.next("two");
  p1.then((result) => show("immediate-two:p1", result));
  p2.then((result) => show("immediate-two:p2", result));
  console.log("immediate-two:after calls");
  await p1;
  await p2;
}

async function immediateThree() {
  async function* immediateThreeGenerator() {
    console.log("immediate-three:body:start");
    const a = yield "one";
    console.log("immediate-three:body:a", a);
    const b = yield "two";
    console.log("immediate-three:body:b", b);
    yield "three";
  }

  const it = immediateThreeGenerator();
  const p1 = it.next();
  const p2 = it.next("A");
  const p3 = it.next("B");
  p1.then((result) => show("immediate-three:p1", result));
  p2.then((result) => show("immediate-three:p2", result));
  p3.then((result) => show("immediate-three:p3", result));
  console.log("immediate-three:after calls");
  await p1;
  await p2;
  await p3;
}

await immediateTwo();
await immediateThree();
