// An Any key on a statically known array must pass the strict flag to the
// runtime, including when the assignment is rejected by a frozen receiver.
function store(array: number[], key: any): string {
  try {
    array[key] = 42;
    return 'ok';
  } catch (error) {
    return error instanceof TypeError ? 'TypeError' : 'other';
  }
}
const writable = [1, 2];
console.log(store(writable, '1'), writable.join(','));
console.log(store(writable, 0), writable.join(','));
const frozen: number[] = Object.freeze([1, 2]) as any;
console.log(store(frozen, '1'), frozen.join(','));
console.log(store(frozen, 0), frozen.join(','));
export {};
