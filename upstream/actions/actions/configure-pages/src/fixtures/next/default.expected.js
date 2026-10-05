/** @type {import('next').NextConfig} */
const nextConfig = {
  images: { unoptimized: true },
  basePath: '/docs',
  output: 'export',
  reactStrictMode: true,
  swcMinify: true
}

module.exports = nextConfig
