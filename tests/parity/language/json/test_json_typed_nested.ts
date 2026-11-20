// Step 1 test: nested object fields parse correctly through the
// slow-path-on-nested-values contract of parse_object_shaped.

interface Point { x: number; y: number; }
interface Record {
  id: number;
  pos: Point;
  tags: string[];
}

const blob = '[{"id":1,"pos":{"x":10,"y":20},"tags":["a","b"]},{"id":2,"pos":{"x":30,"y":40},"tags":["c"]}]';

const items = JSON.parse<Record[]>(blob);
console.log("len:" + items.length);
for (let i = 0; i < items.length; i++) {
  console.log("id=" + items[i].id + " x=" + items[i].pos.x + " y=" + items[i].pos.y + " tags=" + items[i].tags.join(","));
}
