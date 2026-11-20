// A `class X extends Promise` that overrides then/catch/finally (the Anthropic
// SDK's APIPromise shape): calls and `await` must go through the override.
class Lazy<T> extends Promise<T> {
  private inner: Promise<T>;
  calls = 0;
  constructor(inner: Promise<T>) {
    super((resolve) => {
      resolve(null as any);
    });
    this.inner = inner;
  }
  then(a?: any, b?: any) {
    this.calls++;
    return this.inner.then(a, b);
  }
  catch(b?: any) {
    return this.inner.catch(b);
  }
  finally(b?: any) {
    return this.inner.finally(b);
  }
}
async function main() {
  const ok = new Lazy<number>(Promise.resolve(5));
  console.log(await ok, await ok.then((x: number) => x + 1), ok.calls);
  const bad = new Lazy<number>(Promise.reject(new Error("boom")));
  console.log(await bad.catch((e: any) => "caught " + e.message));
  try {
    await bad;
  } catch (e: any) {
    console.log("await rejects", e.message);
  }
  console.log(await new Lazy<string>(Promise.resolve("f")).finally(() => console.log("fin")));
  const plain = Promise.resolve(7);
  console.log(await plain, await Promise.resolve(plain));
}
main();
