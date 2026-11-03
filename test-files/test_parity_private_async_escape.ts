async function published(value: number): Promise<number> { await 0; return value; }
const alias = published;
const promise = alias(5);
const values = [promise, published(7)];
Promise.all(values).then((result) => { console.log(result.join(",") + ":" + (promise === values[0])); });
