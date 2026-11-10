export { Group, Registry } from './registry.js';

// Deliberately has multiple `this` field sites so an importing adapter's
// class-ID dispatch tower can profitably route to the producer's clone.
export class TowerRegistry {
  left = [];
  right = [];

  cycle(value) {
    this.left.push(value);
    this.right.push(value);
    this.left.pop();
    this.right.pop();
  }
}
