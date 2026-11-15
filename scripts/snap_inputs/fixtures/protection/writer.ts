import { cells } from './holder';
cells[1500].value = -1;
let sum = 0;
for (let i = 0; i < cells.length; i++) sum += cells[i].value;
export const result = sum;
