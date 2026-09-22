### Fixed

- Direct calls through static getters inherited from a factory-produced class now invoke the getter's returned function, including on further subclasses.
