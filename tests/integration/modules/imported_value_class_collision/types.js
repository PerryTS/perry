class Shared {
  static label = 'class';
  static state(id) { return { namespace: 'wrong-class', id }; }
  value() { return 42; }
}
export { Shared as OtherClass };
export function touch() { return 'loaded'; }
export function makeInstance() { return new Shared(); }
export const ClassValue = Shared;
