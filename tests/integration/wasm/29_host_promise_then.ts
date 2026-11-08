async function make(): Promise<number> {
  return 41;
}

make().then((v) => {
  console.log("then " + (v + 1));
});
console.log("sync done");
