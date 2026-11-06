import { requireFromModule, computed, destructured, getLoader, arrow } from './loader.js';

function check(loader) {
  if (typeof loader !== 'function') throw new Error('loader not callable');
  if (typeof loader('util').inherits !== 'function') throw new Error('util missing');
  if (typeof loader('stream').Stream !== 'function') throw new Error('stream missing');
}

if (requireFromModule !== computed || computed !== destructured ||
    destructured !== getLoader() || getLoader() !== arrow()) {
  throw new Error('module loader identity not stable');
}
check(requireFromModule);
import('./loader.js').then(module => {
  if (module.requireFromModule !== requireFromModule) throw new Error('export identity changed');
  check(module.requireFromModule);
  console.log('PASS: first-class import.meta.require');
});
