const CYCLIC = true;
const EPOCHS = 96;
const BATCH = 128;
const DEPTH = 5;
const WINDOW = 8;
// Reference nodes: child-to-parent cycles are observed during every traversal.
const TEXT = 'A collaborative document revision contains paragraphs, comments, edits, and history. '.repeat(3);
class DocNode {
  id: number;
  text: string;
  children: DocNode[];
  parent: DocNode | null;
  constructor(seed: number, parent: DocNode | null) {
    this.id = seed % 100003;
    this.text = TEXT + seed;
    this.children = [];
    this.parent = CYCLIC ? parent : null;
  }
}
function makeTree(depth: number, seed: number, parent: DocNode | null): DocNode {
  const node = new DocNode(seed, parent);
  if (depth > 0) {
    for (let i = 0; i < 4; i++) {
      node.children.push(makeTree(depth - 1, seed * 4 + i + 1, node));
    }
  }
  return node;
}
function scan(node: DocNode): number {
  let value = node.id + node.text.length + node.text.charCodeAt(node.text.length - 1);
  for (let i = 0; i < node.children.length; i++) {
    const child = node.children[i];
    if (CYCLIC && child.parent !== node) throw new Error('parent identity mismatch');
    if (!CYCLIC && child.parent !== null) throw new Error('unexpected parent');
    value += scan(child);
  }
  return value;
}
function capture(root: DocNode): () => number {
  return () => scan(root);
}

// A bounded revision cache keeps old graphs and closures alive across churn.
const roots: DocNode[] = [];
let latestRoot: DocNode | null = null;
let readLatest: () => number = () => 0;
let checksum = 0;
let treesBuilt = 0;
let nodesBuilt = 0;
function nodeCount(depth: number): number {
  let count = 1;
  for (let i = 0; i < depth; i++) count = count * 4 + 1;
  return count;
}
function build(depth: number, seed: number, parent: DocNode | null): DocNode {
  treesBuilt++;
  nodesBuilt += nodeCount(depth);
  return makeTree(depth, seed, parent);
}
function epoch(index: number, retain: boolean): void {
  for (let i = 0; i < BATCH; i++) {
    const root = build(DEPTH, (index * BATCH + i) % 4000, null);
    checksum += scan(root);
    if (retain && i === 0) {
      const slot = index % WINDOW;
      if (roots.length < WINDOW) roots.push(root);
      else roots[slot] = root;
      latestRoot = root;
      readLatest = capture(root);
    }
  }
  for (let i = 0; i < roots.length; i++) {
    // An older cached object now points at newly allocated children.
    roots[i].children[0] = build(DEPTH - 1, index * 13 + i, roots[i]);
    checksum += scan(roots[i]);
  }
}

function verifyCapture(): void {
  if (latestRoot !== null && scan(latestRoot) !== readLatest()) {
    throw new Error('closure/cache identity mismatch');
  }
  checksum += readLatest();
}

// Fill, churn, release, then reuse. No forced GC or runtime-specific API.
function checkpoint(phase: string, index: number): void {
  verifyCapture();
  console.log('phase=' + phase + ' epoch=' + index + ' checksum=' + checksum
    + ' roots=' + roots.length + ' trees=' + treesBuilt + ' nodes=' + nodesBuilt);
}
for (let i = 0; i < WINDOW; i++) {
  epoch(i, true);
  checkpoint('fill', i);
}
for (let i = WINDOW; i < WINDOW + EPOCHS; i++) {
  epoch(i, true);
  checkpoint('churn', i);
}
roots.length = 0;
latestRoot = null;
readLatest = () => 0;
checkpoint('released', WINDOW + EPOCHS);
for (let i = WINDOW + EPOCHS; i < WINDOW + EPOCHS * 2; i++) {
  epoch(i, true);
  checkpoint('reuse', i);
}
roots.length = 0;
latestRoot = null;
readLatest = () => 0;
checkpoint('released-again', WINDOW + EPOCHS * 2);
for (let i = WINDOW + EPOCHS * 2; i < WINDOW + EPOCHS * 2 + WINDOW; i++) {
  epoch(i, false);
  checkpoint('drain', i);
}
console.log('done checksum=' + checksum + ' nodes=' + nodesBuilt);
