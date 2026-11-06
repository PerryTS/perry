// Every super() form into a node:zlib codec keeps the subclass instance:
// implicit constructor, explicit super(opts), explicit super(...args), and an
// indirect subclass through a constructor-free user class.
import { Deflate, Gunzip, Gzip, gzipSync, inflateSync } from 'node:zlib';

class Implicit extends Deflate {
  seen = 0;
  _transform(chunk: any, encoding: any, callback: any) {
    this.seen++;
    super._transform(chunk, encoding, callback);
  }
}
class Explicit extends Deflate {
  tag: string;
  constructor(opts: any) {
    super(opts);
    this.tag = 'explicit';
  }
  hi() { return this.tag; }
}
class Spread extends Gunzip {
  constructor(...args: any[]) {
    super(...args);
  }
  hi() { return 'spread'; }
}
class Mid extends Gzip {}
class Leaf extends Mid {
  hi() { return 'leaf'; }
}

const implicit = new Implicit({ level: 9 });
console.log('implicit', Object.getPrototypeOf(implicit) === Implicit.prototype, implicit instanceof Deflate);
const out: any[] = [];
implicit.on('data', (c: any) => out.push(c));
implicit.on('end', () => console.log('implicit end', implicit.seen, inflateSync(Buffer.concat(out)).toString()));
implicit.write('a/');
implicit.end('b');

const explicit = new Explicit({});
console.log('explicit', Object.getPrototypeOf(explicit) === Explicit.prototype, explicit.hi());

const spread = new Spread();
const plain: any[] = [];
spread.on('data', (c: any) => plain.push(c));
spread.on('end', () => console.log('spread end', spread.hi(), Buffer.concat(plain).toString()));
spread.end(gzipSync('zipped'));

const leaf = new Leaf();
console.log('leaf', leaf instanceof Leaf, leaf instanceof Mid, leaf instanceof Gzip, leaf.hi());
