class Session {
  steering: string[] = [];
  handle = async (event: any) => {
    console.log("handle-enter " + event.type);
    if (event.type === "msg" && this.steering.length > 0) this.steering.pop();
    await Promise.resolve(1);
    console.log("handle-after-await " + event.type);
  };
}
(async () => {
  const session = new Session();
  await session.handle({ type: "a" });
  const ref = session.handle;
  await ref({ type: "b" });
  const listeners = new Set<any>([session.handle]);
  for (const listener of listeners) await listener({ type: "msg" });
  console.log("done");
  process.exit(0);
})();
