// Regression test for Discord-style BigInt permission reducers.
// Uses unannotated locals so Perry must preserve bigint result types across
// shift / and / or / not, matching Node byte-for-byte.

function run(label: string, fn: () => unknown): void {
  try {
    const value = fn();
    console.log(label + " ok " + String(value) + " typeof " + typeof value);
  } catch (e) {
    const err = e as Error;
    console.log(label + " throw " + err.name + ": " + err.message);
  }
}

run("manageMessages", () => {
  const manageMessages = 1n << 13n;
  return manageMessages;
});

run("sendMessages", () => {
  const sendMessages = 1n << 11n;
  return sendMessages;
});

run("andResult", () => {
  const bitfield = BigInt("9216");
  const manageMessages = 1n << 13n;
  return bitfield & manageMessages;
});

run("orResult", () => {
  const sendMessages = 1n << 11n;
  return 0n | sendMessages;
});

run("notResult", () => {
  const sendMessages = 1n << 11n;
  return ~sendMessages;
});
