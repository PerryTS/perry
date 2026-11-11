import { Settings as Registry, context } from './state.js';
export function store() { return Registry.of(context().host); }
