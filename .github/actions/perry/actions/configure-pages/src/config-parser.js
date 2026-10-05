const fs = require('fs')
const espree = require('espree')
const core = require('@actions/core')

/*
Parse a JavaScript based configuration file and inject arbitrary key/value in it.
This is used to make sure most static site generators can automatically handle
Pages's path based routing (and work).

Supported configuration initializations:

(1) Direct default export:

  export default {
    // configuration object here
  }

(2) Direct module export:

  module.exports = {
    // configuration object here
  }

(3) Indirect default export:

  const config = {
    // configuration object here
  }
  export default config

(4) Indirect module export:

  const config = {
    // configuration object here
  }
  module.exports = config

(5) Direct default export with wrapping call:

  export default defineConfig({
    // configuration object here
  })

(6) Direct module export with wrapping call:

  module.exports = defineConfig({
    // configuration object here
  })

(7) Indirect default export with wrapping call at the definition:

  const config = defineConfig({
    // configuration object here
  })
  export default config

(8) Indirect default export with wrapping call at the export:

  const config = {
    // configuration object here
  }
  export default defineConfig(config)

(9) Indirect module export with wrapping call at the definition:

  const config = defineConfig({
    // configuration object here
  })
  module.exports = config

(10) Indirect module export with wrapping call at the export:

  const config = {
    // configuration object here
  }
  module.exports = defineConfig(config)
*/

class ConfigParser {
  // Ctor
  // - configurationFile: path to the configuration file
  // - blankConfigurationFile: a blank configuration file to use if non was previously found
  constructor({ configurationFile, blankConfigurationFile, allowWrappingCall = false, properties }) {
    // Save field
    this.configurationFile = configurationFile
    this.allowWrappingCall = allowWrappingCall === true
    this.properties = properties

    // If the configuration file does not exist, initialize it with the blank configuration file
    if (!fs.existsSync(this.configurationFile)) {
      core.info('Using default blank configuration')
      const blankConfiguration = fs.readFileSync(blankConfigurationFile, 'utf8')
      fs.writeFileSync(this.configurationFile, blankConfiguration, {
        encoding: 'utf8'
      })
    }

    // Read the configuration file
    this.configuration = fs.readFileSync(this.configurationFile, 'utf8')
  }

  findTopLevelVariableDeclarator(ast, identifierName) {
    let targetDeclarator
    ast.body.find(
      node =>
        node.type === 'VariableDeclaration' &&
        node.declarations &&
        node.declarations.length > 0 &&
        node.declarations.find(declarator => {
          if (
            declarator.type === 'VariableDeclarator' &&
            declarator.id &&
            declarator.id.type === 'Identifier' &&
            declarator.id.name === identifierName
          ) {
            targetDeclarator = declarator
            return true
          }
        })
    )
    return targetDeclarator
  }

  // Find the configuration object in an AST.
  // Look for, in order:
  //  - a direct default export
  //  - a direct default export with a wrapping call
  //  - an indirect default export
  //  - an indirect default export with a wrapping call at the definition
  //  - an indirect default export with a wrapping call at the export
  //  - a direct module export
  //  - a direct module export with a wrapping call
  //  - an indirect module export
  //  - an indirect module export with a wrapping call at the definition
  //  - an indirect module export with a wrapping call at the export
  //
  // Return the configuration object or null.
  findConfigurationObject(ast, allowWrappingCall = false) {
    // Try to find a default export
    var defaultExport = ast.body.find(node => node.type === 'ExportDefaultDeclaration')

    // Direct default export
    if (defaultExport && defaultExport.declaration.type === 'ObjectExpression') {
      core.info('Found configuration object in direct default export declaration')
      return defaultExport.declaration
    }

    // Direct default export with a wrapping call
    else if (
      allowWrappingCall &&
      defaultExport &&
      defaultExport.declaration.type === 'CallExpression' &&
      defaultExport.declaration.arguments.length > 0 &&
      defaultExport.declaration.arguments[0] &&
      defaultExport.declaration.arguments[0].type === 'ObjectExpression'
    ) {
      core.info('Found configuration object in direct default export declaration with a wrapping call')
      return defaultExport.declaration.arguments[0]
    }

    // Indirect default export
    else if (defaultExport && defaultExport.declaration.type === 'Identifier') {
      const identifierName = defaultExport.declaration.name
      const identifierDeclarator = this.findTopLevelVariableDeclarator(ast, identifierName)
      const identifierInitialization = identifierDeclarator && identifierDeclarator.init
      if (identifierInitialization && identifierInitialization.type === 'ObjectExpression') {
        core.info('Found configuration object in indirect default export declaration')
        return identifierInitialization
      }
      // Indirect default export with a wrapping call at the definition
      else if (
        allowWrappingCall &&
        identifierInitialization &&
        identifierInitialization.type === 'CallExpression' &&
        identifierInitialization.arguments.length > 0 &&
        identifierInitialization.arguments[0] &&
        identifierInitialization.arguments[0].type === 'ObjectExpression'
      ) {
        core.info(
          'Found configuration object in indirect default export declaration with a wrapping call at the definition'
        )
        return identifierInitialization.arguments[0]
      }
    }

    // Indirect default export with a wrapping call at the export
    else if (
      allowWrappingCall &&
      defaultExport &&
      defaultExport.declaration.type === 'CallExpression' &&
      defaultExport.declaration.arguments.length > 0 &&
      defaultExport.declaration.arguments[0] &&
      defaultExport.declaration.arguments[0].type === 'Identifier'
    ) {
      const identifierName = defaultExport.declaration.arguments[0].name
      const identifierDeclarator = this.findTopLevelVariableDeclarator(ast, identifierName)
      const identifierInitialization = identifierDeclarator && identifierDeclarator.init
      if (identifierInitialization && identifierInitialization.type === 'ObjectExpression') {
        core.info(
          'Found configuration object in indirect default export declaration with a wrapping call at the export'
        )
        return identifierInitialization
      }
    }

    // Try to find a module export
    var moduleExport = ast.body.find(
      node =>
        node.type === 'ExpressionStatement' &&
        node.expression.type === 'AssignmentExpression' &&
        node.expression.operator === '=' &&
        node.expression.left.type === 'MemberExpression' &&
        node.expression.left.object.type === 'Identifier' &&
        node.expression.left.object.name === 'module' &&
        node.expression.left.property.type === 'Identifier' &&
        node.expression.left.property.name === 'exports'
    )

    // Direct module export
    if (moduleExport && moduleExport.expression.right.type === 'ObjectExpression') {
      core.info('Found configuration object in direct module export')
      return moduleExport.expression.right
    }

    // Direct default export with a wrapping call
    else if (
      allowWrappingCall &&
      moduleExport &&
      moduleExport.expression.right.type === 'CallExpression' &&
      moduleExport.expression.right.arguments.length > 0 &&
      moduleExport.expression.right.arguments[0] &&
      moduleExport.expression.right.arguments[0].type === 'ObjectExpression'
    ) {
      core.info('Found configuration object in direct module export with a wrapping call')
      return moduleExport.expression.right.arguments[0]
    }

    // Indirect module export
    else if (moduleExport && moduleExport.expression.right.type === 'Identifier') {
      const identifierName = moduleExport && moduleExport.expression.right.name
      const identifierDeclarator = this.findTopLevelVariableDeclarator(ast, identifierName)
      const identifierInitialization = identifierDeclarator && identifierDeclarator.init
      if (identifierInitialization && identifierInitialization.type === 'ObjectExpression') {
        core.info('Found configuration object in indirect module export')
        return identifierInitialization
      }
      // Indirect module export with a wrapping call at the definition
      else if (
        allowWrappingCall &&
        identifierInitialization &&
        identifierInitialization.type === 'CallExpression' &&
        identifierInitialization.arguments.length > 0 &&
        identifierInitialization.arguments[0] &&
        identifierInitialization.arguments[0].type === 'ObjectExpression'
      ) {
        core.info('Found configuration object in indirect module export with a wrapping call at the definition')
        return identifierInitialization.arguments[0]
      }
    }

    // Indirect module export with a wrapping call at the export
    else if (
      allowWrappingCall &&
      moduleExport &&
      moduleExport.expression.right.type === 'CallExpression' &&
      moduleExport.expression.right.arguments.length > 0 &&
      moduleExport.expression.right.arguments[0] &&
      moduleExport.expression.right.arguments[0].type === 'Identifier'
    ) {
      const identifierName = moduleExport.expression.right.arguments[0].name
      const identifierDeclarator = this.findTopLevelVariableDeclarator(ast, identifierName)
      const identifierInitialization = identifierDeclarator && identifierDeclarator.init
      if (identifierInitialization && identifierInitialization.type === 'ObjectExpression') {
        core.info('Found configuration object in indirect module export declaration with a wrapping call at the export')
        return identifierInitialization
      }
    }

    // No configuration object found
    return null
  }

  // Find a property with a given name on a given object.
  //
  // Return the matching property or null.
  findProperty(object, name) {
    // Try to find a property matching a given name
    const property =
      object.type === 'ObjectExpression' &&
      object.properties.find(node => node.key.type === 'Identifier' && node.key.name === name)

    // Return the property's value (if found) or null
    if (property) {
      return property.value
    }
    return null
  }

  // Generate a (nested) property declaration.
  // - properties: list of properties to generate
  // - startIndex: the index at which to start in the declaration
  // - propertyValue: the value of the property
  //
  // Return a nested property declaration as a string.
  getPropertyDeclaration(properties, startIndex, propertyValue) {
    if (startIndex === properties.length - 1) {
      return `${properties[startIndex]}: ${JSON.stringify(propertyValue)}`
    } else {
      return (
        `${properties[startIndex]}: {` + this.getPropertyDeclaration(properties, startIndex + 1, propertyValue) + '}'
      )
    }
  }

  // Inject all properties into the configuration
  injectAll() {
    for (var [propertyName, propertyValue] of Object.entries(this.properties)) {
      this.inject(propertyName, propertyValue)
    }
  }

  // Inject an arbitrary property into the configuration
  // - propertyName: the name of the property (may use . to target nested objects)
  // - propertyValue: the value of the property
  inject(propertyName, propertyValue) {
    // Logging
    core.info(`Injecting property=${propertyName} and value=${propertyValue} in:`)
    core.info(this.configuration)

    // Parse the AST out of the configuration file
    const espreeOptions = {
      ecmaVersion: 'latest',
      sourceType: 'module',
      range: true
    }
    const ast = espree.parse(this.configuration, espreeOptions)

    // Find the configuration object
    var configurationObject = this.findConfigurationObject(ast, this.allowWrappingCall)
    if (!configurationObject) {
      throw 'Could not find a configuration object in the configuration file'
    }

    // A property may be nested in the configuration file. Split the property name with '.'
    // then walk the configuration object one property at a time.
    var depth = 0
    const properties = propertyName.split('.')
    var lastNode = configurationObject
    // eslint-disable-next-line no-constant-condition
    while (true) {
      // Find the node for the current property
      var propertyNode = this.findProperty(lastNode, properties[depth])

      // Update last node
      if (propertyNode != null) {
        lastNode = propertyNode
        depth++
      }

      // Exit when exiting the current configuration object
      if (propertyNode == null || depth >= properties.length) {
        break
      }
    }

    // If the configuration file is defining the property we are after, update it.
    if (depth == properties.length) {
      // The last node identified is an object expression, so do the assignment
      if (lastNode.type === 'ObjectExpression') {
        this.configuration =
          this.configuration.slice(0, lastNode.range[0]) +
          JSON.stringify(propertyValue) +
          this.configuration.slice(lastNode.range[1])
      }

      // A misc object was found in the configuration file (e.g. an array, a string, a boolean,
      // a number, etc.), just replace the whole range by our declaration
      else {
        this.configuration =
          this.configuration.slice(0, lastNode.range[0]) +
          JSON.stringify(propertyValue) +
          this.configuration.slice(lastNode.range[1])
      }
    }

    // Create nested properties in the configuration file
    else {
      // Build the declaration to inject
      const declaration = this.getPropertyDeclaration(properties, depth, propertyValue)

      // The last node identified is an object expression, so do the assignment
      if (lastNode.type === 'ObjectExpression') {
        // The object is blank (no properties) so replace the whole range by a new object containing the declaration
        if (lastNode.properties.length === 0) {
          this.configuration =
            this.configuration.slice(0, lastNode.range[0]) +
            '{' +
            declaration +
            '}' +
            this.configuration.slice(lastNode.range[1])
        }

        // The object contains other properties, prepend our new one at the beginning
        else {
          this.configuration =
            this.configuration.slice(0, lastNode.properties[0].range[0]) +
            declaration +
            ',' +
            this.configuration.slice(lastNode.properties[0].range[0])
        }
      }

      // A misc object was found in the configuration file (e.g. an array, a string, a boolean,
      // a number, etc.), just replace the whole range by our declaration
      else {
        this.configuration =
          this.configuration.slice(0, lastNode.range[0]) +
          '{' +
          declaration +
          '}' +
          this.configuration.slice(lastNode.range[1])
      }
    }

    // Logging
    core.info('Injection successful, new configuration:')
    core.info(this.configuration)

    // Finally write the new configuration in the file
    fs.writeFileSync(this.configurationFile, this.configuration, {
      encoding: 'utf8'
    })
  }
}

module.exports = { ConfigParser }
