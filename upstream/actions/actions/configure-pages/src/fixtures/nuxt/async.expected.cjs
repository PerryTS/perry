const getAllDynamicRoute = async function () {
  const routes = await (async () => {
    return ['/posts/hello-world', '/posts/hello-again']
  })()
  return routes
}

module.exports = {
  target: 'static',
  router: { base: '/docs/' },
  mode: 'universal',
  generate: {
    async routes() {
      return getAllDynamicRoute()
    }
  }
}
