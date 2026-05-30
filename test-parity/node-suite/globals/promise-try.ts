function show(label: string, promise: Promise<unknown>) {
  promise.then(
    (value) => console.log(label + ":fulfilled:" + String(value)),
    (error: any) => console.log(label + ":rejected:" + error.name + ":" + error.message),
  );
}

show("return", Promise.try(() => 42));
show("args", Promise.try((a: number, b: number) => a + b, 2, 3));
show("throw", Promise.try(() => {
  throw new TypeError("boom");
}));
show("nonfn", Promise.try(123 as any));
console.log("after scheduling");

await Promise.resolve();
