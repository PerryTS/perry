export function exercise(): string {
  const path = process.getBuiltinModule('node:path');
  const util = process.getBuiltinModule('node:util');
  const events = process.getBuiltinModule('node:events');
  const rt = (s: string) => s.split('').join('');
  const out: string[] = [];
  function check(name: string, f: () => any) {
    try { out.push(name + ':' + JSON.stringify(f())); }
    catch (e: any) { out.push(name + ':error:' + e.code); }
  }
  check('join-literal-2', () => path.join('/a', 'b'));
  check('join-runtime-2', () => path.join(rt('/a'), rt('b')));
  check('join-mixed-2', () => path.join(rt('/a'), 'b'));
  check('join-runtime-3', () => path.join(rt('/a'), rt('b'), rt('c')));
  check('join-runtime-4', () => path.join(rt('/a'), rt('b'), rt('c'), rt('d')));
  check('join-literal-4', () => path.join('/a', 'b', 'c', 'd'));
  check('join-literal-3', () => path.join('/a', 'b', 'c'));
  check('join-mixed-4', () => path.join('/a', rt('b'), 'c', rt('d')));
  check('join-manifest', () => path.join(rt('/tmp/x'), 'package.json'));
  check('normalize-literal', () => path.normalize('/a/b/../c'));
  check('normalize-runtime', () => path.normalize(rt('/a/b/../c')));
  check('resolve-runtime-2', () => path.resolve(rt('/a'), rt('b')));
  check('resolve-runtime-3', () => path.resolve(rt('/a'), rt('b'), rt('../c')));
  check('resolve-literal-4', () => path.resolve('/a', 'b', 'c', '../d'));
  check('resolve-literal', () => path.resolve('/a', 'b', '../c'));
  check('resolve-mixed', () => path.resolve(rt('/a'), 'b', rt('c'), '../d'));
  check('basename-literal', () => path.basename('/a/file.txt', '.txt'));
  check('basename-mixed', () => path.basename(rt('/a/file.txt'), '.txt'));
  check('basename-runtime', () => path.basename(rt('/a/file.txt'), rt('.txt')));
  check('dirname-literal', () => path.dirname('/a/file.txt'));
  check('dirname-runtime', () => path.dirname(rt('/a/file.txt')));
  check('extname-literal', () => path.extname('/a/file.txt'));
  check('extname-runtime', () => path.extname(rt('/a/file.txt')));
  check('relative-literal', () => path.relative('/a/b', '/a/c'));
  check('relative-mixed', () => path.relative(rt('/a/b'), '/a/c'));
  check('relative-runtime', () => path.relative(rt('/a/b'), rt('/a/c')));
  check('parse-literal', () => path.parse('/a/file.txt'));
  check('parse-runtime', () => path.parse(rt('/a/file.txt')));
  check('json-literal', () => JSON.stringify('literal string'));
  check('json-object', () => JSON.stringify({'literal key': 'literal value', tiny: 'x'}));
  check('json-runtime', () => JSON.stringify({[rt('literal key')]: rt('literal value')}));
  check('url-literal', () => new URL('../c?q=x', 'https://example.com/a/b').href);
  check('url-runtime', () => new URL(rt('../c?q=x'), rt('https://example.com/a/b')).href);
  check('url-mixed', () => new URL(rt('../c?q=x'), 'https://example.com/a/b').href);
  check('mime-literal', () => new util.MIMEType('text/plain;charset=utf-8').toString());
  check('mime-runtime', () => new util.MIMEType(rt('text/plain;charset=utf-8')).toString());
  check('mime-mixed', () => { const m = new util.MIMEType('text/plain'); m.params.set('charset', rt('utf-8')); return m.toString(); });
  check('event', () => {
    const emitter = new events.EventEmitter();
    let result = '';
    emitter.on('literal-event', (s: string) => { result += s; });
    emitter.emit(rt('literal-event'), 'first');
    emitter.emit('literal-event', rt('second'));
    return result;
  });
  check('compare', () => ['literal string' === rt('literal string'), rt('literal string') === 'literal string', 'x' === rt('x'), rt('x') === 'x', 'literal string' < rt('literal string'), 'literal string' < rt('zebra'), rt('literal string') < 'zebra', 'literal string'.localeCompare(rt('literal string'))]);
  check('map-set', () => {
    const map = new Map<string, number>(); const set = new Set<string>();
    map.set('literal key', 1); map.set(rt('literal key'), 2);
    set.add('literal key'); set.add(rt('literal key'));
    return [map.size, map.get('literal key'), map.get(rt('literal key')), set.size, set.has(rt('literal key'))];
  });
  check('object-key', () => {
    const object: any = {'literal key': 'value'};
    object[rt('literal key')] = 'updated';
    return [Object.keys(object), object['literal key'], object[rt('literal key')]];
  });
  check('negative-number', () => path.normalize(42 as any));
  return out.join('\n');
}
