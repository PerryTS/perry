class Store {
  constructor() {
    this.entities = [];
  }

  create(id) {
    const entity = { id, value: 0 };
    this.entities.push(entity);
    return entity;
  }

  add(entity) {
    entity.value += 1;
  }

  destroy(entity) {
    const index = this.entities.indexOf(entity);
    if (index !== -1) this.entities.splice(index, 1);
    return entity.value;
  }
}

export default {
  store: null,
  setup() {
    this.store = new Store();
  },
  createEntity(id) {
    return this.store.create(id);
  },
  addComponent(entity) {
    this.store.add(entity);
  },
  destroyEntity(entity) {
    return this.store.destroy(entity);
  },
};
