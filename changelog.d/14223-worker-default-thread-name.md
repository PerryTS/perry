### Fixed

- Workers with omitted or falsy name options report `WorkerThread` through both the parent Worker and worker namespace, while explicit names and main-thread metadata remain unchanged.
