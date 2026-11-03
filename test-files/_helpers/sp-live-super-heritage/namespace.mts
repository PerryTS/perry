class Base { constructor(value) { this.value = value; } }
export class First extends Base { constructor(value) { super(value); this.kind = 'first'; } }
export class Second extends Base { constructor(value) { super(value); this.kind = 'second'; } }
