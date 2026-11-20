class EventStream {
  queue: string[] = [];
  resolve: (() => void) | null = null;
  done = false;
  push(e: string) {
    this.queue.push(e);
    if (this.resolve) { const resolve = this.resolve; this.resolve = null; resolve(); }
  }
  async *[Symbol.asyncIterator](): AsyncGenerator<string> {
    while (true) {
      if (this.queue.length === 0) {
        if (this.done) return;
        await new Promise<void>((resolve) => { this.resolve = resolve; });
      }
      while (this.queue.length > 0) yield this.queue.shift() as string;
    }
  }
}
const stream = new EventStream();
(async () => {
  for await (const event of stream) {
    console.log("event " + event);
    if (event === "last") { console.log("got last"); process.exit(0); }
  }
})();
setTimeout(() => stream.push("working"), 50);
setTimeout(() => stream.push("token"), 100);
setTimeout(() => stream.push("last"), 150);
setTimeout(() => { console.log("GUARD (deadlocked)"); process.exit(2); }, 3000);
