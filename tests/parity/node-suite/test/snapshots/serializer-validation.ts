import { snapshot } from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}

console.log("not array:", codeOf(() => snapshot.setDefaultSnapshotSerializers("x" as any)));
console.log("bad entry:", codeOf(() => snapshot.setDefaultSnapshotSerializers([() => "ok", 1 as any])));
console.log("valid:", codeOf(() => snapshot.setDefaultSnapshotSerializers([(value) => String(value)])));
snapshot.setDefaultSnapshotSerializers([]);
