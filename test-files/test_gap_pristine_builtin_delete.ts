// #11915: deleting a builtin method, then restoring the original.
const a = [3, 1, 2];
function run() {
  try {
    console.log(a.indexOf(1), 'abc'.indexOf('c'));
  } catch (e) {
    console.log('TypeError', e instanceof TypeError);
  }
}
run();
const saved = Array.prototype.indexOf;
delete (Array.prototype as any).indexOf;
run();
(Array.prototype as any).indexOf = saved;
run();
const sp: any = String.prototype;
const savedS = sp.indexOf;
Object.defineProperty(sp, 'indexOf', { value: function () { return 'defined'; }, writable: true, configurable: true });
run();
sp.indexOf = savedS;
run();
