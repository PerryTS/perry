module.exports = function removeTrailingSlash(str) {
  return str.endsWith('/') ? str.slice(0, -1) : str
}
