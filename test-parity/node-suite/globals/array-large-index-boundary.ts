function show(label: string, value: any) {
  try {
    console.log(label + ':', String(value));
  } catch (err: any) {
    console.log(label + ': throw', err && err.name, err && err.message);
  }
}

const a: any = [0, 1, 2];
a[4294967295] = 'x';
show('a length', a.length);
show('a max-u32', a[4294967295]);
show('a string max-u32', a['4294967295']);
show('a has max-u32', 4294967295 in a);
show('a own max-u32', Object.prototype.hasOwnProperty.call(a, '4294967295'));
show('a keys', Object.keys(a).join(','));

const b: any = [0, 1, 2];
b[4294967294] = 'y';
show('b length', b.length);
show('b max-index', b[4294967294]);
show('b string max-index', b['4294967294']);
show('b has max-index', 4294967294 in b);
show('b own max-index', Object.prototype.hasOwnProperty.call(b, '4294967294'));
show('b keys tail', Object.keys(b).slice(-3).join(','));
