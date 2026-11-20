### Fixed

- Guard the prebuilt stdlib HTTP isolation contract with a regression test that
  follows transitive Cargo feature edges, so the 0.5.1220 configuration (which
  enabled `external-http-client-pump` through `perry-stdlib/full`) cannot recur
  through an indirect edge (#8587).
