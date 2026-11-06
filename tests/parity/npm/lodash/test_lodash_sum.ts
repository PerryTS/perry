import _ from 'lodash';
console.log(_.sum([1, 2, 3, 4]));        // 10
console.log(_.mean([1, 2, 3, 4]));       // 2.5
console.log(_.sumBy([{n:1},{n:2}], 'n')); // 3
console.log(_.head([1, 2, 3]));          // 1
console.log(_.last([1, 2, 3]));          // 3
