class Original { constructor() { this.original = 1; } value() { return 1; } }
class Replacement { constructor() { this.replacement = 20; } value() { return 20; } }
exports.Parent = Original;
exports.replace = function () { exports.Parent = Replacement; };
