// Object throws and Reflect returns false for the same array rejection.
for (const forwarded of [false, true]) {
  for (const name of ["0", "added", Symbol("added")]) {
    const array: any[] = [];
    Object.preventExtensions(array);
    const receiver = forwarded ? new Proxy(array, {}) : array;
    const reflected = Reflect.defineProperty(receiver, name, { value: 1 });
    let threw = false;
    try { Object.defineProperty(receiver, name, { value: 1 }); }
    catch (error) { threw = error instanceof TypeError; }
    if (reflected !== false || !threw) throw new Error("array addition rejection");
    console.log(forwarded, typeof name, reflected, threw);
  }
  const fixed = Symbol("fixed");
  const array: any[] = [];
  Object.defineProperty(array, fixed, { value: 1 });
  const receiver = forwarded ? new Proxy(array, {}) : array;
  const reflected = Reflect.defineProperty(receiver, fixed, { value: 2 });
  let threw = false;
  try { Object.defineProperty(receiver, fixed, { value: 2 }); }
  catch (error) { threw = error instanceof TypeError; }
  if (reflected !== false || !threw) throw new Error("array symbol redefinition");
  console.log(forwarded, "fixed symbol", reflected, threw);
}
