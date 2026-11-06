import * as Shared from './types.js';

export function checkNamespace() {
  const namespace = Shared;
  if (namespace.touch() !== 'loaded' || Shared.OtherClass.state('namespace').namespace !== 'wrong-class') {
    throw new Error('class metadata shadowed namespace import');
  }
}
