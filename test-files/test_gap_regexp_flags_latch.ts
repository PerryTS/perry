// RegExp flag reads. The runtime serves `.flags` and the flag-driven String
// methods (replace, replaceAll, match, matchAll, split, search) from the
// regex's own flag bits while the nine flag accessors on RegExp.prototype are
// still the built-in getters. Every observable change must still be seen: an
// unrelated write to RegExp.prototype, a redefined or deleted flag getter, a
// shadowing own property, a subclass, a prototype swap, and lastIndex.
// Every line is compared with node.
const out = [];
const check = (label, value) => out.push(label + ': ' + value);
const attempt = (f) => {
  try {
    return f();
  } catch (e) {
    return 'threw ' + e.constructor.name;
  }
};
// Property names come from data, never from literals the compiler could resolve.
const k = (s) => s.split('').join('');
const flagNames = ['hasIndices', 'global', 'ignoreCase', 'multiline', 'dotAll', 'unicode', 'unicodeSets', 'sticky'];
const flagsRow = (re) => re[k('flags')] + '|' + flagNames.map((n) => (re[n] ? 1 : 0)).join('');
const ops = (re) =>
  [
    attempt(() => 'aXbxc'.replace(re, '-')),
    attempt(() => 'aXbxc'.split(re).join(',')),
    attempt(() => String('aXbxc'.match(re))),
    attempt(() => [...'aXbxc'.matchAll(new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g'))].length),
    attempt(() => 'aXbxc'.search(re)),
    attempt(() => 'aXbxc'.replaceAll(new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g'), '_')),
    attempt(() => 'aXbxc'.replaceAll(re, '_')),
    attempt(() => String(re.test('aXbxc')) + re.lastIndex),
    attempt(() => String(re.exec('aXbxc')) + re.lastIndex),
  ].join(' ; ');
const sources = () => [/x/, /x/g, /x/i, /x/gi, /x/y, /x/gy, /x/m, /x/s, /x/u, /x/d, /x/v, /(?<L>[a-c])/g, /[a-c]/gimsuy];

for (let round = 0; round < 3; round++) {
  check('builtin rows ' + round, sources().map((re) => flagsRow(re) + ' => ' + ops(re)).join(' || '));
}

// An unrelated write to RegExp.prototype keeps every flag read exact.
const side = k('sideChannel');
RegExp.prototype[side] = 1;
check('unrelated write', sources().map((re) => flagsRow(re) + ' => ' + ops(re) + ' ' + re[side]).join(' || '));
delete RegExp.prototype[side];
check('unrelated delete', sources().map((re) => flagsRow(re) + ' => ' + ops(re) + ' ' + re[side]).join(' || '));
RegExp.prototype.exec2 = RegExp.prototype.exec;
check('method alias write', ops(/x/gi) + ' ' + flagsRow(/x/gi));
delete RegExp.prototype.exec2;

// lastIndex through the flag-driven paths
{
  const g = /x/g;
  g.lastIndex = 3;
  const afterReplace = 'aXbxc'.replace(g, '-') + ' ' + g.lastIndex;
  g.lastIndex = 3;
  const afterTest = String(g.test('aXbxc')) + ' ' + g.lastIndex;
  const y = /x/y;
  y.lastIndex = 3;
  const sticky = String('aXbxc'.match(y)) + ' ' + y.lastIndex;
  const plain = /x/;
  plain.lastIndex = 3;
  check('lastIndex', [afterReplace, afterTest, sticky, 'aXbxc'.replace(plain, '-') + ' ' + plain.lastIndex].join(' | '));
}

// A flag getter redefined on RegExp.prototype is called by the String methods
// and by .flags, with the receiver, and its answer decides the behavior.
const restore = (name, desc) => Object.defineProperty(RegExp.prototype, name, desc);
const globalDesc = Object.getOwnPropertyDescriptor(RegExp.prototype, 'global');
const flagsDesc = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags');
{
  const calls = [];
  Object.defineProperty(RegExp.prototype, 'global', {
    configurable: true,
    get() {
      calls.push('global');
      return false;
    },
  });
  const re = /x/g;
  check('global getter redefined: flags', re.flags + ' ' + calls.join() + ' ' + flagsRow(re));
  calls.length = 0;
  check('global getter redefined: replace', attempt(() => 'aXxbxc'.replace(/x/gi, '-')) + ' ' + calls.join());
  check('global getter redefined: split', attempt(() => 'aXxbxc'.split(/x/gi).join(',')) + ' ' + calls.length);
  check('global getter redefined: match', attempt(() => String('aXxbxc'.match(/x/gi))) + ' ' + calls.length);
  restore('global', globalDesc);
  check('global getter restored', flagsRow(/x/g) + ' ' + ops(/x/g) + ' ' + ops(/x/));
}
{
  const calls = [];
  let answer = null;
  Object.defineProperty(RegExp.prototype, 'flags', {
    configurable: true,
    get() {
      calls.push(this.source);
      return answer === null ? flagsDesc.get.call(this) : answer;
    },
  });
  const re = /x/g;
  check('flags getter redefined: delegating read', re.flags + ' ' + calls.join());
  calls.length = 0;
  check('flags getter redefined: split', attempt(() => 'aXxbxc'.split(/x/gi).join(',')) + ' ' + calls.join());
  calls.length = 0;
  answer = 'i';
  check('flags getter answers i: read', /x/gi.flags + ' ' + flagsRow(/x/g));
  check('flags getter answers i: split', attempt(() => 'aXxbxc'.split(/x/gi).join(',')));
  restore('flags', flagsDesc);
  check('flags getter restored', flagsRow(/x/gi) + ' ' + ops(/x/gi) + ' ' + ops(/x/));
}
{
  const desc = Object.getOwnPropertyDescriptor(RegExp.prototype, 'sticky');
  Object.defineProperty(RegExp.prototype, 'sticky', { configurable: true, get: () => true });
  check('sticky getter redefined', flagsRow(/x/) + ' ' + ops(/x/g));
  restore('sticky', desc);
  check('sticky getter restored', flagsRow(/x/y) + ' ' + ops(/x/g));
}

// Own properties on one regex shadow the prototype for that regex only.
{
  const re = /x/g;
  Object.defineProperty(re, 'flags', { value: 'i' });
  const other = /x/g;
  check('own flags', re.flags + ' ' + attempt(() => 'aXbxc'.replace(re, '-')) + ' ' + other.flags + ' ' + attempt(() => 'aXbxc'.replace(other, '-')));
  const own = /x/gi;
  Object.defineProperty(own, 'global', { value: false });
  check('own global', flagsRow(own) + ' ' + attempt(() => 'aXbxc'.replace(own, '-')) + ' ' + attempt(() => 'aXbxc'.replaceAll(own, '-')));
  const plain = /x/g;
  plain.extra = 1;
  check('own unrelated property', flagsRow(plain) + ' ' + ops(plain));
  const named = /x/gi;
  named[k('lastIndex')] = 0;
  check('own lastIndex write', ops(named));
}

// Subclasses and prototype swaps.
{
  class Sub extends RegExp {}
  class FlagSub extends RegExp {
    get flags() {
      return 'i';
    }
  }
  class GlobalSub extends RegExp {
    get global() {
      return false;
    }
  }
  check('subclass', flagsRow(new Sub('x', 'gi')) + ' ' + ops(new Sub('x', 'g')) + ' ' + ops(new Sub('x', 'i')));
  check('subclass flags getter', flagsRow(new FlagSub('x', 'gi')));
  check('subclass global getter', flagsRow(new GlobalSub('x', 'gi')) + ' ' + attempt(() => 'aXbxc'.replace(new GlobalSub('x', 'gi'), '-')));
  const moved = /x/gi;
  const donorProto = { __proto__: RegExp.prototype, get flags() { return 'y'; } };
  Object.setPrototypeOf(moved, donorProto);
  check('instance prototype swapped', moved.flags);
  Object.setPrototypeOf(moved, RegExp.prototype);
  check('instance prototype restored', flagsRow(moved) + ' ' + ops(moved));
  const nullProto = /x/g;
  Object.setPrototypeOf(nullProto, null);
  check('instance null prototype', String(nullProto.flags) + ' ' + attempt(() => 'aXbxc'.replace(nullProto, '-')));
  check('Symbol.replace callable', attempt(() => RegExp.prototype[Symbol.replace].call(/x/gi, 'aXbxc', '-')) + ' ' + attempt(() => RegExp.prototype[Symbol.split].call(/x/i, 'aXbxc').join(',')));
  const fake = { flags: 'g', global: true, lastIndex: 0, exec: () => null, [Symbol.replace]: undefined };
  check('plain object flags', attempt(() => typeof RegExp.prototype[Symbol.matchAll].call(fake, 'abc').next));
}

// Flags reads on odd receivers keep their spec behavior.
check('flags getter receivers', [undefined, null, 1, 'x', {}, [], RegExp.prototype, { global: true, sticky: true, hasIndices: 1 }].map((r) => attempt(() => String(flagsDesc.get.call(r)))).join(' | '));
check('construction flags', [new RegExp('x', 'dgimsuy'), new RegExp(/x/g, 'i'), new RegExp(/x/gi), RegExp('x', 'v')].map(flagsRow).join(' | '));

// A deleted flag getter (last: later reads would otherwise run on a prototype that lost it).
delete RegExp.prototype.sticky;
check('sticky getter deleted', flagsRow(/x/gy) + ' ' + ops(/x/g) + ' ' + flagsRow(/x/y) + ' ' + ops(/x/y));
console.log(out.join('\n'));
