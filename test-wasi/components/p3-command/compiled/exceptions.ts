function fail(): void {
  const error = new Error("p3-error");
  throw error;
}
try {
  fail();
  console.log("unreachable");
} catch (error) {
  console.log("caught", error instanceof Error, (error as Error).message);
} finally {
  console.log("finally");
}
console.log("survived");
