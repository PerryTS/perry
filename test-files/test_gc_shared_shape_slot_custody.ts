// Old receivers share shape keys and a shape-owned prototype. Their ordinary
// fields still need remembering when rewritten to young values.
declare function gc(): void;
const proto = { marker: 'shared-prototype' };
const graph: any[][] = [];
for (let p = 0; p < 20; p++) {
  const page: any[] = [];
  for (let j = 0; j < 1000; j++) {
    const obj = Object.create(proto);
    obj.id = p * 1000 + j;
    obj.kind = obj.id % 16;
    obj.name = 'type';
    page.push(obj);
  }
  graph.push(page);
}
let sum = 0;
for (let round = 0; round < 6; round++) {
  if (typeof gc === 'function') gc();
  for (let p = 0; p < graph.length; p++) {
    const page = graph[p];
    for (let j = 0; j < page.length; j++) {
      const obj = page[j];
      obj.name = 'round-' + round + '-node-' + obj.id;
      obj['extra' + round] = { label: 'young-' + obj.id };
      const keys = Object.keys(obj);
      let expectedKeys = 'id,kind,name';
      for (let r = 0; r <= round; r++) expectedKeys += ',extra' + r;
      if (keys.join(',') !== expectedKeys) throw new Error('lost shape keys');
      if (Object.getPrototypeOf(obj) !== proto || obj.marker !== 'shared-prototype') {
        throw new Error('lost shared prototype');
      }
      sum += obj.id + obj.kind + obj.name.length;
      if (obj['extra' + round].label !== 'young-' + obj.id) throw new Error('lost young field');
    }
  }
}
if (typeof gc === 'function') gc();
console.log(graph.length, graph[19][999].name, sum);
