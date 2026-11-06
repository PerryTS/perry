import { COMMANDS as staticCommands, TOTAL as staticTotal } from './values.js';

async function main() {
  const { COMMANDS, LOOKUP, TOTAL, EMPTY, checkArgs } = await import('./values.js');
  if (typeof COMMANDS !== 'object' || !COMMANDS.has('doctor')) throw new Error('Set export');
  if (typeof LOOKUP !== 'object' || LOOKUP.get('doctor') !== true) throw new Error('Map export');
  if (TOTAL !== 42 || EMPTY !== undefined) throw new Error('primitive export');
  if (!checkArgs(['doctor'])) throw new Error('function alias');
  if (staticCommands !== COMMANDS || staticTotal !== TOTAL) throw new Error('static/dynamic identity');
  console.log('PASS: local variable exports');
}
main();
