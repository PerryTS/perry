// Regression coverage for #4000 / #3566: async declarations, function
// expressions, and arrows must all resolve awaited returns and route rejected
// awaits to the catch region that was active at suspension time.

async function declReturn(): Promise<string> {
  return "decl-return";
}

const exprReturn = async function(): Promise<string> {
  return "expr-return";
};

const arrowReturn = async (): Promise<string> => {
  return "arrow-return";
};

async function declAfterAwait(): Promise<string> {
  await Promise.resolve("tick");
  return "decl-after-await";
}

const exprAfterAwait = async function(): Promise<string> {
  await Promise.resolve("tick");
  return "expr-after-await";
};

const arrowAfterAwait = async (): Promise<string> => {
  await Promise.resolve("tick");
  return "arrow-after-await";
};

async function declThrowAfterAwait(): Promise<string> {
  await Promise.resolve("tick");
  throw "decl-throw-after-await";
}

const exprThrowAfterAwait = async function(): Promise<string> {
  await Promise.resolve("tick");
  throw "expr-throw-after-await";
};

const arrowThrowAfterAwait = async (): Promise<string> => {
  await Promise.resolve("tick");
  throw "arrow-throw-after-await";
};

async function main(): Promise<void> {
  console.log(await declReturn());
  console.log(await exprReturn());
  console.log(await arrowReturn());

  console.log(await declAfterAwait());
  console.log(await exprAfterAwait());
  console.log(await arrowAfterAwait());

  try {
    await declThrowAfterAwait();
  } catch (e: any) {
    console.log("caught " + e);
  }

  try {
    await exprThrowAfterAwait();
  } catch (e: any) {
    console.log("caught " + e);
  }

  try {
    await arrowThrowAfterAwait();
  } catch (e: any) {
    console.log("caught " + e);
  }

  console.log("done");
}

main();
