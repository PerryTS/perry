let trace = '';
const target: any = { x: 4 };
const key: any = { toString() { trace += 'key;'; return 'x'; } };
console.log(target[key]++, trace, target.x); trace = '';
console.log(target[key] += 2, trace, target.x); trace = '';
console.log(target[key] &&= 9, trace, target.x);
trace = '';
let nil: any = null;
try { nil[key]++; } catch { console.log('null-update', trace); }
trace = '';
try { nil[key] += 2; } catch { console.log('null-compound', trace); }
