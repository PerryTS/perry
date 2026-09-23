interface Task { id: number; title: string; done: boolean; tags: string[] }
class TaskList {
  private tasks: Task[] = [];
  private next = 1;
  add(title: string, tags: string[] = []): Task {
    const t = { id: this.next++, title, done: false, tags };
    this.tasks.push(t); return t;
  }
  complete(id: number) { const t = this.tasks.find(t => t.id === id); if (!t) throw new Error(`no task ${id}`); t.done = true; }
  byTag(): Map<string, Task[]> {
    const m = new Map<string, Task[]>();
    for (const t of this.tasks) for (const g of t.tags) { if (!m.has(g)) m.set(g, []); m.get(g)!.push(t); }
    return m;
  }
  toJSON() { return this.tasks; }
}
const args = process.argv.slice(2);
const list = new TaskList();
list.add("write compiler", ["work"]); list.add("buy milk", ["home"]); list.add("ship release", ["work", "urgent"]);
list.complete(2);
try { list.complete(42); } catch (e) { console.error(String(e)); }
for (const [tag, ts] of list.byTag()) console.log(`${tag.padEnd(8)} ${ts.map(t => (t.done ? "[x] " : "[ ] ") + t.title).join(", ")}`);
console.log(JSON.stringify(list));
if (args.includes("--verbose")) console.log(`args=${args.length}`);
