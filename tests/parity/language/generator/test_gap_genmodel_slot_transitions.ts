// A fixed slot changes its GC classification in both directions across
// suspensions; the object last sent into the generator stays live at a poll.
function* transitions() {
  let slot: any = { value: 1 };
  for (let i = 0; i < 12; i++) {
    yield typeof slot;
    if (i % 4 === 0) slot = i;
    else if (i % 4 === 1) slot = "slot-" + i;
    else if (i % 4 === 2) slot = { value: i };
    else slot = BigInt(i);
    const scratch = [];
    for (let j = 0; j < 100; j++) scratch.push({ value: j });
    console.log("slot", i, typeof slot, String(typeof slot === "object" ? slot.value : slot), scratch.length);
  }
  return slot;
}
const iterator = transitions();
for (let i = 0; i < 13; i++) {
  const result = iterator.next();
  console.log("resume", String(result.value), result.done);
}

// ++ may run user code and collect before storing back into the state record.
function* coercion() {
  let slot: any = {
    valueOf() {
      const scratch = [];
      for (let j = 0; j < 150; j++) scratch.push({ value: j });
      console.log("coerce", scratch.length);
      return 20;
    }
  };
  yield "ready";
  yield slot++;
  yield slot;
}
console.log("coercion", Array.from(coercion()).join(","));

function* reservedNames() {
  let __gen_state: any = { value: 119 };
  let __gen_done: any = "user-done";
  yield __gen_state.value;
  __gen_state = "user-state";
  yield __gen_state;
  yield __gen_done;
  __gen_done = { value: 120 };
  return __gen_done.value;
}
const named = reservedNames();
for (let i = 0; i < 4; i++) { const result = named.next(); console.log("user-name", result.value, result.done); }

// Declared types select lowering; a sent JS value remains dynamically typed.
function* typedSent() {
  let value: number = (yield "typed-ready") as any;
  yield ++value;
  return value;
}
const typed = typedSent();
console.log("typed", typed.next().value);
const sent: any = { valueOf() { console.log("typed-coerce"); return 121; } };
console.log("typed", typed.next(sent).value);
console.log("typed", typed.next().value);

// A binding alias must survive another slot's append after suspension.
function* stringAliases() {
  let accumulator = "seed-" + 37;
  let snapshot = accumulator;
  yield snapshot;
  accumulator += ":local";
  yield snapshot;
  let received: string = (yield "send") as any;
  let receivedSnapshot = received;
  yield receivedSnapshot;
  received += ":resumed";
  yield receivedSnapshot;
  return accumulator + "/" + received;
}
const aliases = stringAliases();
console.log("alias", aliases.next().value);
console.log("alias", aliases.next().value);
console.log("alias", aliases.next().value);
let externalString = "sent-" + 38;
console.log("alias", aliases.next(externalString).value);
externalString += ":caller";
console.log("alias", aliases.next().value, externalString);
console.log("alias", aliases.next().value, externalString);

// Accessor execution can move the frame and another local before the next read.
function* getterReads() {
  const kept = { value: 123 };
  const record: any = {
    get payload() {
      const scratch = [];
      for (let i = 0; i < 150; i++) scratch.push({ value: i });
      return { value: 122 + scratch.length };
    }
  };
  yield record.payload.value + kept.value;
  return record.payload.value + kept.value;
}
console.log("getter-reads", Array.from(getterReads()).join(","));
