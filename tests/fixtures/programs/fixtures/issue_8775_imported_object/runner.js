export default {
  // Intentionally does not read `this`: this is the concise-method HIR shape
  // used by ddmills' `suite.perform(ctx)` wrapper.
  perform(ctx, id) {
    const entity = ctx.createEntity(id);
    ctx.addComponent(entity);
    ctx.addComponent(entity);
    return ctx.destroyEntity(entity);
  },
};
