exports.id = module.id;
exports.filename = module.filename;
exports.path = module.path;
exports.loadedDuringEvaluation = module.loaded;
exports.paths = module.paths;
exports.requireIdentity = module.require === require;
exports.parentId = module.parent && module.parent.id;
