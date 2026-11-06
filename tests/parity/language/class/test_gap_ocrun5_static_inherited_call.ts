// A static call must resolve the same constructor chain as a member read.
// Fresh factory evaluations keep their heritage on the class object.
function parent(key: string) {
  function Tag() {}
  Object.assign(Tag, { key })
  Object.setPrototypeOf(Tag, {
    relay(value: any) { return value },
    read() { return this.key },
  })
  return Tag
}
function factory(key: string) {
  class Middle extends parent(key) {}
  return Middle
}
class First extends factory("first") {}
class Second extends factory("second") {}
const value = () => 42
console.log(typeof First.relay)
console.log(First.relay(value) === value)
console.log(First.read(), Second.read())
