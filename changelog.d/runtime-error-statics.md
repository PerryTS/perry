Fix `Error.isError` and `Error.prepareStackTrace` becoming `undefined` when a
program references stream or fetch globals. Error statics and their inherited
identities now stay consistent when providers initialize the realm before
program literals register.
