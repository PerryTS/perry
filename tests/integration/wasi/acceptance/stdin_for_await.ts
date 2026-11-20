// #13953: `for await` over process.stdin on a WASI 0.2 component. The reader
// used to be a thread, and the spawn panicked ("failed to spawn thread").
async function main(): Promise<void> {
  let total = 0;
  let text = "";
  for await (const chunk of process.stdin) {
    total += chunk.length;
    text += chunk.toString();
  }
  console.log("total", total, JSON.stringify(text));
}
main().catch((e: Error) => console.log("error", e.message));
