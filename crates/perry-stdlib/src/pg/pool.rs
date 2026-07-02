//! PostgreSQL connection pool implementation

use perry_runtime::{js_promise_new, JSValue, Promise};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;

use super::result::rows_to_pg_result;
use super::types::parse_pg_config;
use crate::common::{register_handle, Handle};

/// Wrapper around PgPool that we can store in the handle registry.
///
/// Lives in two states like PgConnectionHandle: pre-pool (`pending_url`
/// holds the connection URL, `pool` is None) and pool-built (`pool` is
/// Some). `new Pool(config)` creates the pre-pool form synchronously
/// without touching the Tokio runtime — sqlx's `connect_lazy` ALSO
/// touches Tokio internals and panics outside a runtime context, so we
/// can't even use it; the actual sqlx pool is built on first query.
/// The older combined `js_pg_create_pool` factory still returns a fully
/// built pool inside its async block.
pub struct PgPoolHandle {
    pub pool: Option<PgPool>,
    pub pending_url: Option<String>,
}

impl PgPoolHandle {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            pending_url: None,
        }
    }

    pub fn pending(url: String) -> Self {
        Self {
            pool: None,
            pending_url: Some(url),
        }
    }

    /// Lazy-build the sqlx pool on first use. Only callable from within a
    /// Tokio runtime context (every spawn_for_promise body). Safe to call
    /// repeatedly — only the first call actually builds the pool.
    pub async fn ensure_pool(&mut self) -> Result<&PgPool, String> {
        if self.pool.is_none() {
            let url = self
                .pending_url
                .take()
                .ok_or_else(|| "Pool config missing".to_string())?;
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await
                .map_err(|e| format!("Failed to create pool: {}", e))?;
            self.pool = Some(pool);
        }
        Ok(self.pool.as_ref().unwrap())
    }
}

/// `new Pool(config)` — synchronous constructor matching npm pg's API.
///
/// Returns a Handle directly (no Promise wrapper). The actual sqlx pool
/// can't be built here because sqlx 0.8's `PgPoolOptions::connect_lazy`
/// touches Tokio runtime internals and panics outside a runtime context,
/// and the synchronous `new` path doesn't have one. Instead we store
/// just the connection URL; `pool.query()` lazy-builds the pool on
/// first use (its spawn_for_promise body runs inside a Tokio runtime).
///
/// # Safety
/// The config parameter must be a valid JSValue representing a config object.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_new(config_f: f64) -> Handle {
    let config = JSValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    register_handle(PgPoolHandle::pending(pg_config.to_url()))
}

/// new Pool(config) -> Promise<Pool>
///
/// Creates a new PostgreSQL connection pool with the given configuration.
///
/// # Safety
/// The config parameter must be a valid JSValue representing a config object.
#[no_mangle]
pub unsafe extern "C" fn js_pg_create_pool(config_f: f64) -> *mut Promise {
    // Take f64 at the FFI boundary to avoid SysV AMD64 ABI mismatch
    // (see js_mysql2_create_pool for details).
    let config = JSValue::from_bits(config_f.to_bits());
    let promise = js_promise_new();

    // Parse the config
    let pg_config = parse_pg_config(config);

    // Extract max connections if provided (default to 10)
    let max_conns = 10u32;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let url = pg_config.to_url();

        match PgPoolOptions::new()
            .max_connections(max_conns)
            .connect(&url)
            .await
        {
            Ok(pool) => {
                let handle = register_handle(PgPoolHandle::new(pool));
                Ok(handle as u64)
            }
            Err(e) => Err(format!("Failed to create pool: {}", e)),
        }
    });

    promise
}

/// pool.query(sql) -> Promise<Result>
///
/// Executes a query on the pool.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_query(pool_handle: Handle, sql_ptr: *const u8) -> *mut Promise {
    let promise = js_promise_new();

    // Extract the SQL string
    let sql = if sql_ptr.is_null() {
        String::new()
    } else {
        let header = sql_ptr as *const perry_runtime::StringHeader;
        let len = (*header).byte_len as usize;
        let data_ptr = sql_ptr.add(std::mem::size_of::<perry_runtime::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(bytes).to_string()
    };

    // Determine command type from SQL
    let command = sql
        .split_whitespace()
        .next()
        .unwrap_or("SELECT")
        .to_uppercase();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle_mut;

        if let Some(wrapper) = get_handle_mut::<PgPoolHandle>(pool_handle) {
            // Lazy-build the sqlx pool on first query if `new Pool(config)`
            // produced a pre-pool handle. Already-built pools (from the
            // older `js_pg_create_pool` factory) skip the build cheaply.
            let pool = wrapper.ensure_pool().await?;
            match sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .fetch_all(pool)
                .await
            {
                Ok(rows) => {
                    let columns: Vec<_> = if !rows.is_empty() {
                        rows[0].columns().to_vec()
                    } else {
                        Vec::new()
                    };

                    let result = rows_to_pg_result(rows, &columns, &command);
                    Ok(result.bits())
                }
                Err(e) => Err(format!("Query failed: {}", e)),
            }
        } else {
            Err("Invalid pool handle".to_string())
        }
    });

    promise
}

/// pool.end() -> Promise<void>
///
/// Closes all connections in the pool.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::take_handle;

        if let Some(mut wrapper) = take_handle::<PgPoolHandle>(pool_handle) {
            if let Some(pool) = wrapper.pool.take() {
                pool.close().await;
                Ok(JSValue::undefined().bits())
            } else {
                // Pre-pool handle (`new Pool` ctor never had a query) — close
                // is a no-op since no connections were ever opened.
                Ok(JSValue::undefined().bits())
            }
        } else {
            Err("Invalid pool handle".to_string())
        }
    });

    promise
}
