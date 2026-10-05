import adapter from '@sveltejs/adapter-auto'

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    prerender: { origin: 'https://configure-pages.github.io' },
    paths: { base: '/docs' },
    adapter: adapter()
  }
}

export default config
