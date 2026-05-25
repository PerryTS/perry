const thing: any = {
  setInterval() {
    return [Promise.resolve("local-a"), Promise.resolve("local-b")];
  },
};

const values: string[] = [];
for await (const value of thing.setInterval(1)) {
  values.push(String(value));
}
console.log("local member values:", values.join(","));
