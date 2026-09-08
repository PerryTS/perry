const load = import.meta.require;
const computed = import.meta['require'];
const { require: destructured } = import.meta;
function getLoader() { return import.meta.require; }
const arrow = () => import.meta.require;
export { load as requireFromModule, computed, destructured, getLoader, arrow };
