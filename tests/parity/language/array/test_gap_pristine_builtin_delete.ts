// #11915: deleting a builtin method, then restoring the original.
// DEC 250 locks Array.prototype: the array adopts a user prototype AP that
// holds its indexOf, and the delete and restore land there. A primitive
// string's indexOf exists only on String.prototype (intrinsic-only): that
// define is refused (expected file).
const AP: any = Object.create(Array.prototype);
AP.indexOf = Array.prototype.indexOf;
Object.setPrototypeOf(AP, Object.prototype);
const a: number[] = Object.setPrototypeOf([3, 1, 2], AP);
function run() {
  try {
    console.log(a.indexOf(1), 'abc'.indexOf('c'));
  } catch (e) {
    console.log('TypeError', e instanceof TypeError);
  }
}
run();
const saved = AP.indexOf;
delete AP.indexOf;
run();
AP.indexOf = saved;
run();
const sp: any = String.prototype;
const savedS = sp.indexOf;
try { Object.defineProperty(sp, 'indexOf', { value: function () { return 'defined'; }, writable: true, configurable: true }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
run();
try { sp.indexOf = savedS; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
run();
