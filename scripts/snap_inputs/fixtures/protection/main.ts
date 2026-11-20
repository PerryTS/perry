import { cells } from './holder';
console.log('before', cells.length);
const m = await import('./writer');
console.log('boundary complete', m.result);
