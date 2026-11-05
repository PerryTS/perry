// A throwing Worker constructor must not leave an indexed-store CFG behind.
function captured(n: any) {
  var workers: any[] = [];
  function size() { return workers.length; }
  if (n > 5) {
    for (var i = 0; i < n; ++i) workers[i] = new Worker('x');
  }
  return size();
}
console.log('captured', captured(1));

function caught() {
  const workers: any[] = [];
  function size() { return workers.length; }
  try {
    workers[0] = new Worker('missing-worker');
    console.log('unreachable store');
  } catch (_) {
    console.log('caught', size());
  }
}
caught();

const globals: any[] = [];
if (globals.length > 5) globals[0] = new Worker('x');
console.log('global', globals.length);

function normal() {
  const values: any[] = [];
  function size() { return values.length; }
  for (let i = 0; i < 3; i++) values[i] = i + 10;
  console.log('normal', size(), values[0], values[2]);
}
normal();
