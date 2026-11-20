import { getLevelInfo } from './test_labels';

// Module-level call during init — triggers getLevelInfo before main runs
const result = getLevelInfo(50);
console.log(result);

const result2 = getLevelInfo(150);
console.log(result2);

const result3 = getLevelInfo(0);
console.log(result3);
