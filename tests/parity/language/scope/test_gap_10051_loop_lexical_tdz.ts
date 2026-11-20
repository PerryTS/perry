// #10051: each block entry creates fresh captured lexical cells, including TDZ.
function test() {
  const results: string[] = [];
  for (let index = 0; index < 2; index++) {
    let read = () => value;
    try { read(); results.push('missing-tdz'); }
    catch (error) { results.push(error instanceof ReferenceError ? 'tdz' : 'wrong-error'); }
    let value: number;
    results.push(String(read()));
    value = index;
    results.push(String(read()));
    let recurse = (n: number): number => n === 0 ? value : recurse(n - 1);
    results.push(String(recurse(2)));
  }
  return results.join(',');
}
console.log(test());

function probe(read: () => unknown): string {
  let result: string;
  try { result = String(read()); }
  catch (error) { result = error instanceof ReferenceError ? 'tdz' : 'wrong-error'; }
  return result;
}

function retained() {
  'use strict';
  const reads: (() => unknown)[] = [];
  const results: string[] = [];
  for (let index = 0; index < 3; index++) {
    const uninitializedRead = () => uninitialized;
    const initializedRead = () => initialized;
    const constantRead = () => constant;
    results.push(probe(uninitializedRead), probe(initializedRead), probe(constantRead));
    let uninitialized: number;
    results.push(probe(uninitializedRead));
    uninitialized = index;
    let initialized = index + 10;
    const constant = index + 20;
    const recurse = (n: number): number => n === 0 ? initialized : recurse(n - 1);
    reads.push(uninitializedRead, initializedRead, constantRead, () => recurse(2));
    initialized += 100;
  }
  console.log('retained-tdz:' + results.join(','));
  console.log('retained-values:' + reads.map(probe).join(','));
}
retained();

function sharedVar() {
  const reads: (() => unknown)[] = [];
  const results: string[] = [];
  for (let index = 0; index < 3; index++) {
    const read = () => value;
    results.push(probe(read));
    var value = index;
    reads.push(read);
  }
  console.log('var-before:' + results.join(','));
  console.log('var-retained:' + reads.map(probe).join(','));
}
sharedVar();

function otherBlocks() {
  'use strict';
  const reads: (() => unknown)[] = [];
  const results: string[] = [];
  let index = 0;
  while (index < 2) {
    {
      const read = () => value;
      results.push(probe(read));
      const value = index;
      reads.push(read);
    }
    try {
      const read = () => value;
      results.push(probe(read));
      let value = index + 10;
      reads.push(read);
    } finally {
      const read = () => value;
      results.push(probe(read));
      let value = index + 20;
      reads.push(read);
    }
    switch (index) {
      case 0:
      default:
        const read = () => value;
        results.push(probe(read));
        let value = index + 30;
        reads.push(read);
      case 99:
        // Fallthrough stays in the same lexical environment.
        results.push(probe(read));
    }
    index++;
  }
  do {
    const read = () => value;
    results.push(probe(read));
    const value = index + 40;
    reads.push(read);
    index--;
  } while (index > 0);
  console.log('blocks-tdz:' + results.join(','));
  console.log('blocks-retained:' + reads.map(probe).join(','));
}
otherBlocks();

function hoistedBlockFunctions() {
  'use strict';
  const reads: (() => unknown)[] = [];
  const results: string[] = [];
  for (let index = 0; index < 2; index++) {
    results.push(probe(read));
    function read() { return value; }
    let value = index;
    reads.push(read);
  }
  console.log('hoisted-tdz:' + results.join(','));
  console.log('hoisted-retained:' + reads.map(probe).join(','));
}
hoistedBlockFunctions();

// Function expressions use a separate function-body lowering path.
const expression = function () {
  const reads: (() => unknown)[] = [];
  const results: string[] = [];
  for (let index = 0; index < 2; index++) {
    const read = () => value;
    results.push(probe(read));
    let value = index;
    reads.push(read);
  }
  return results.join(',') + ':' + reads.map(probe).join(',');
};
console.log('expression:' + expression());

function abruptEntries() {
  const reads: (() => unknown)[] = [];
  const results: string[] = [];
  for (let index = 0; index < 3; index++) {
    const read = () => value;
    reads.push(read);
    if (index === 1) continue;
    let value = index;
  }
  console.log('skipped-declaration:' + reads.map(probe).join(','));
  for (let index = 0; index < 3; index++) {
    try {
      try {
        if (index === 1) throw new Error('body');
      } finally {
        const read = () => value;
        results.push(probe(read));
        let value = index + 10;
        reads.push(read);
      }
    } catch (error) {
      results.push('caught');
    }
  }
  console.log('finally-paths:' + results.join(','));
  console.log('abrupt-retained:' + reads.map(probe).join(','));
}
abruptEntries();
