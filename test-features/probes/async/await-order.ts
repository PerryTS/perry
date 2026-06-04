async function task(label: string, value: number): Promise<string> {
  const doubled = await Promise.resolve(value * 2);
  return label + ":" + doubled;
}

(async () => {
  const out = await Promise.all([task("a", 2), task("b", 3)]);
  console.log("async/await-order:" + out.join("|"));
})();
