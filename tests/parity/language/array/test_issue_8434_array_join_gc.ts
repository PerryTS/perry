// parity-env: PERRY_GC_SCHEDULE_SEED=8434 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1

const show = (label: string, value: string): void => {
  console.log(`${label}:${value}|${value.length}`);
};

const showCodeUnits = (label: string, value: string): void => {
  let units = "";
  for (let i = 0; i < value.length; i++) {
    if (i > 0) units += ",";
    units += value.charCodeAt(i).toString(16);
  }
  console.log(`${label}:${units}|${value.length}`);
};

show("holes", new Array(3).join("|"));
show("empty", ["", "", ""].join(""));
show("unicode", ["A", "😀", "é"].join("·"));

const high = String.fromCharCode(0xd83d);
const low = String.fromCharCode(0xde00);
show("surrogate-pair", [high, , low].join(""));
showCodeUnits("lone-surrogates", [high, low].join("|"));

let calls = 0;
const values: unknown[] = [];
const churn = (): number => {
  const live: unknown[] = [];
  for (let i = 0; i < 128; i++) {
    live.push({ i, text: ("allocation-" + i).repeat(32) });
  }
  return live.length;
};
const allocating = {
  toString(): string {
    calls++;
    churn();
    values[1] = "after";
    return "before";
  },
};
values.push(allocating, "original", "tail");
const movingSeparator = ("x" + "/").slice(1);
show("coerce", values.join(movingSeparator));
console.log(`calls:${calls}`);

const growthPayload = "0123456789abcdef".repeat(256);
const growthValues: unknown[] = [];
for (let i = 0; i < 32; i++) growthValues.push("");
let growthCalls = 0;
growthValues.push({
  toString(): string {
    growthCalls++;
    churn();
    return growthPayload;
  },
});
const grown = growthValues.join("");
console.log(
  `growth:${grown.length}:${grown.slice(0, 4)}:${grown.slice(-4)}:${growthCalls}`,
);
