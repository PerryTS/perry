const events: string[] = [];

function show(value: unknown): string {
  if (typeof value === "object" && value && "marker" in value) {
    return `object:${(value as { marker: string }).marker}`;
  }
  return String(value);
}

let raceCalls = 0;
const raceThenable = {
  marker: "race-thenable",
  then(resolve: (value: string) => void) {
    raceCalls++;
    events.push(`race.then:${raceCalls}`);
    resolve("race-value");
    resolve("race-ignored");
  },
};

async function observeRace() {
  try {
    const value = await Promise.race([raceThenable]);
    events.push(`race.value:${show(value)}`);
  } catch (reason) {
    events.push(`race.reason:${show(reason)}`);
  }
}

const raceDone = observeRace();
events.push(`race.calls.after:${raceCalls}`);

let anyRejectCalls = 0;
const anyRejectThenable = {
  marker: "any-reject-thenable",
  then(resolve: (value: string) => void, reject: (reason: string) => void) {
    anyRejectCalls++;
    events.push(`any.reject.then:${anyRejectCalls}`);
    reject("thenable-reject");
    resolve("thenable-ignored");
  },
};

async function observeAnyReject() {
  try {
    const value = await Promise.any([Promise.reject("native-reject"), anyRejectThenable]);
    events.push(`any.reject.value:${show(value)}`);
  } catch (reason: any) {
    events.push(`any.reject.reason:${reason.name}:${reason.errors.join("|")}`);
  }
}

const anyRejectDone = observeAnyReject();
events.push(`any.reject.calls.after:${anyRejectCalls}`);

let anyFulfillCalls = 0;
const anyFulfillThenable = {
  marker: "any-fulfill-thenable",
  then(resolve: (value: string) => void, reject: (reason: string) => void) {
    anyFulfillCalls++;
    events.push(`any.fulfill.then:${anyFulfillCalls}`);
    resolve("thenable-fulfill");
    reject("thenable-ignored");
  },
};

async function observeAnyFulfill() {
  try {
    const value = await Promise.any([Promise.reject("native-reject-2"), anyFulfillThenable]);
    events.push(`any.fulfill.value:${show(value)}`);
  } catch (reason: any) {
    events.push(`any.fulfill.reason:${reason.name}`);
  }
}

const anyFulfillDone = observeAnyFulfill();
events.push(`any.fulfill.calls.after:${anyFulfillCalls}`);

await raceDone;
await anyRejectDone;
await anyFulfillDone;

console.log(events.join("\n"));
