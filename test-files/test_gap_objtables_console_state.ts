import { Console } from 'node:console';
const lines: string[] = [];
const sink = { write(s: string) { lines.push(s.trimEnd()); }, on() {}, once() {}, removeListener() {}, listenerCount() { return 0; } };
const a = new Console(sink as any);
const b = new Console(sink as any);
a.count('kept');
a.group();
for (let i = 0; i < 50000; i++) { const x = { i, s: 'pressure' + i }; if (x.i < 0) console.log(x); }
a.count('kept');
a.groupEnd();
a.countReset('kept');
a.count('kept');
b.count('kept');
console.log(JSON.stringify(lines));
console.log(Object.keys(a).filter(k => k === 'native_state' || k === 'counters').length);
