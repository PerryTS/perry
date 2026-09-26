import { getBackend, list } from "perry/container";
async function main() {
  console.log("backend is string:", typeof getBackend() === "string");
  try {
    const s = await list(true);
    console.log("list settled: resolved", typeof s);
  } catch (e) {
    console.log("list settled: rejected");
  }
}
main();
