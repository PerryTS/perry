# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.7](https://github.com/PerryTS/turnloop/compare/turnloop-http-v0.1.0-alpha.6...turnloop-http-v0.1.0-alpha.7) - 2026-09-22

### Other

- HTTP/2 SETTINGS as a live negotiation; websocket Received step contract (#87, #86) ([#100](https://github.com/PerryTS/turnloop/pull/100))
- HTTP/1 step and encoder gaps, and Content-Encoding lists and step reasons in compression ([#104](https://github.com/PerryTS/turnloop/pull/104))
- HTTP client pool deadlines and stale ids, multipart encoder, host-built rustls configs and provider choice ([#101](https://github.com/PerryTS/turnloop/pull/101))

## [0.1.0-alpha.6](https://github.com/PerryTS/turnloop/compare/turnloop-http-v0.1.0-alpha.5...turnloop-http-v0.1.0-alpha.6) - 2026-09-17

### Other

- A terminated HTTP/2 stream no longer takes the connection with it ([#85](https://github.com/PerryTS/turnloop/pull/85))

## [0.1.0-alpha.3](https://github.com/PerryTS/turnloop/compare/turnloop-http-v0.1.0-alpha.2...turnloop-http-v0.1.0-alpha.3) - 2026-09-15

### Other
- Servers close gracefully: HTTP/1 and HTTP/2 linger after the final response instead of resetting a peer that still has unread bytes (#21)
- `server::Options { linger_timeout }` (default 5 s), `Server::bind_with`, `Shutdown::with_options` and `Shutdown::stop_by(deadline)`
- release v0.1.0-alpha.3
