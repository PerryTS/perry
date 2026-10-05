const getAllDynamicRoute = async function () {
  const routes = await (async () => {
    return ['/posts/hello-world', '/posts/hello-again']
  })()
  return routes
}

module.exports = {
  mode: 'universal',
  generate: {
    async routes() {
      return getAllDynamicRoute()
    }
  }
}
