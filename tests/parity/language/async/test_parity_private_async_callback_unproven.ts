async function callbackOwner(callback: () => number): Promise<number> { await 0; return callback(); }
async function check(): Promise<void> {
  const value = await callbackOwner(() => 17);
  console.log("callback " + value);
}
check();
