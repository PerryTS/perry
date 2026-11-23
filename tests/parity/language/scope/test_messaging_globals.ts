// Two messaging globals. (1) Two `BroadcastChannel`s of one name: `postMessage` on one delivers to the other (it
// delivered nothing: without a worker_threads import the stdlib's channel registry was not linked). (2) A class
// declared in a function scope under a built-in global's name (`class MessageChannel`) is that scope's binding only:
// a top-level `new MessageChannel()` constructs the global (it constructed the nested class, so `mc.port1` was
// undefined). Controls: the nested class inside its scope, a channel of another name receives nothing, a top-level
// class that shadows a global in its own scope.
function f() {
  class MessageChannel {
    x = 1;
  }
  return new MessageChannel();
}
console.log("nested", f().x);
const mc: any = new MessageChannel();
console.log("global ports", typeof mc.port1, typeof mc.port2);
const bc = new BroadcastChannel("x");
const bc2 = new BroadcastChannel("x");
const other = new BroadcastChannel("y");
other.onmessage = (e: any) => console.log("other got", e.data);
bc2.onmessage = (e: any) => {
  console.log("bc2 got", e.data);
  bc.close();
  bc2.close();
  other.close();
  mc.port1.postMessage(9);
};
mc.port2.onmessage = (e: any) => {
  console.log("port2 got", e.data);
  mc.port1.close();
  mc.port2.close();
};
bc.postMessage(8);
function g() {
  class Map {
    size = "shadowed";
  }
  return new Map().size;
}
console.log("shadow", g(), new Map([[1, 2]]).size);
