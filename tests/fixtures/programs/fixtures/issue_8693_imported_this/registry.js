export class Group {
  entities = [];

  pushEntity(entity) {
    this.entities.push(entity);
  }

  removeEntity(entity) {
    const index = this.entities.indexOf(entity);
    if (index !== -1) this.entities.splice(index, 1);
  }
}

export class Registry {
  group = new Group();

  add(entity) {
    this.group.pushEntity(entity);
  }

  remove(entity) {
    this.group.removeEntity(entity);
  }
}
