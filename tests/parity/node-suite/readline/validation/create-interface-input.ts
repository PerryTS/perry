import * as readline from "node:readline";

function probe(label: string, value: any) {
  try {
    const rl = readline.createInterface(value);
    console.log(`${label}: ok ${typeof rl}`);
    if (rl && typeof rl.close === "function") {
      rl.close();
    }
  } catch (error: any) {
    console.log(`${label}: ${error.name}:${error.message}`);
  }
}

probe("empty options", {});
probe("undefined input", { input: undefined });
