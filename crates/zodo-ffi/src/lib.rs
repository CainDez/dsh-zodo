//! Minimal C ABI for embedding the Zodo kernel in a native client.
//!
//! This is a synchronous command/query bridge, not the full HTTP runtime. It owns no network
//! listener and no UI; the caller supplies a data directory and must keep the handle alive for the
//! duration of calls. All entry points catch panics and return a JSON error object for malformed
//! input; no panic crosses the C boundary.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, QueryEnvelope};
use zodo_kernel::model::{CommandContext, PrincipalKind};
use zodo_kernel::ports::{Store, SystemClock, UuidIds};
use zodo_runtime::SqliteStore;

pub struct Handle {
    store: Mutex<SqliteStore>,
    workspace_id: String,
    instance_id: String,
    counter: AtomicU64,
}

thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn set_last_error(message: impl Into<String>) {
    LAST_ERROR.with(|value| *value.borrow_mut() = Some(message.into()));
}

fn error_json(message: impl Into<String>) -> String {
    json!({
        "schema": "zodo.error/1",
        "error": {"code": "FFI_ERROR", "message": message.into(), "retryable": false}
    })
    .to_string()
}

fn ffi_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new(error_json("response contained an interior NUL"))
            .unwrap()
            .into_raw(),
    }
}

fn read_c(pointer: *const c_char) -> Result<String, String> {
    if pointer.is_null() {
        return Err("null string pointer".to_string());
    }
    // SAFETY: caller contract: pointer is a valid NUL-terminated string for this call.
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|error| format!("argument is not UTF-8: {error}"))?;
    Ok(value.to_string())
}

fn context(handle: &Handle, request_id: String) -> CommandContext {
    CommandContext {
        principal_id: "ffi_embedder".to_string(),
        principal_kind: PrincipalKind::InteractiveUi,
        channel: "ffi".to_string(),
        capabilities: [
            "read", "write", "capture", "propose", "approve", "execute", "admin",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        workspace_id: handle.workspace_id.clone(),
        request_id,
        received_at: chrono::Utc::now(),
        runtime_instance: handle.instance_id.clone(),
    }
}

fn open_inner(data_dir: *const c_char) -> Result<Handle, String> {
    let directory = read_c(data_dir)?;
    let path = Path::new(&directory);
    let mut store = SqliteStore::open(path.join("zodo.sqlite3"))
        .map_err(|error| format!("open SQLite store: {error}"))?;
    let world = store
        .load_world("ws_local")
        .map_err(|error| format!("load workspace: {error}"))?
        .ok_or_else(|| "workspace ws_local does not exist in this data directory".to_string())?;
    Ok(Handle {
        store: Mutex::new(store),
        workspace_id: world.workspace.id,
        instance_id: format!("ffi_{}", uuid_like()),
        counter: AtomicU64::new(0),
    })
}

fn command_inner(
    handle: *mut Handle,
    operation: *const c_char,
    payload_json: *const c_char,
    idempotency_key: *const c_char,
) -> Result<String, String> {
    if handle.is_null() {
        return Err("null Zodo handle".to_string());
    }
    let operation = read_c(operation)?;
    let payload: Value = serde_json::from_str(&read_c(payload_json)?)
        .map_err(|error| format!("payload is not JSON: {error}"))?;
    let idempotency_key = if idempotency_key.is_null() {
        format!("ffi-{}", uuid_like())
    } else {
        read_c(idempotency_key)?
    };
    // SAFETY: non-null checked above and owned by the caller for this call.
    let handle = unsafe { &*handle };
    let counter = handle.counter.fetch_add(1, Ordering::SeqCst);
    let request_id = format!("ffi-{counter}");
    let envelope = CommandEnvelope {
        schema: "zodo.command/1".to_string(),
        request_id: request_id.clone(),
        idempotency_key,
        workspace_id: handle.workspace_id.clone(),
        operation,
        operation_version: 1,
        base_revision: None,
        policy_revision: None,
        payload,
    };
    let mut store = handle
        .store
        .lock()
        .map_err(|_| "Zodo handle is poisoned".to_string())?;
    let instance_id = handle.instance_id.clone();
    let mut app = Application {
        store: &mut *store,
        clock: &SystemClock,
        ids: &UuidIds::default(),
        instance_id,
    };
    let ctx = context(handle, request_id);
    match app.execute(&ctx, envelope) {
        Ok(result) => serde_json::to_string(&result).map_err(|error| error.to_string()),
        Err(error) => {
            set_last_error(error.message.clone());
            Ok(error_json(error.message))
        }
    }
}

fn query_inner(
    handle: *mut Handle,
    query: *const c_char,
    payload_json: *const c_char,
) -> Result<String, String> {
    if handle.is_null() {
        return Err("null Zodo handle".to_string());
    }
    let query = read_c(query)?;
    let payload: Value = serde_json::from_str(&read_c(payload_json)?)
        .map_err(|error| format!("payload is not JSON: {error}"))?;
    // SAFETY: non-null checked above and owned by the caller for this call.
    let handle = unsafe { &*handle };
    let counter = handle.counter.fetch_add(1, Ordering::SeqCst);
    let request_id = format!("ffi-q-{counter}");
    let envelope = QueryEnvelope {
        schema: "zodo.query/1".to_string(),
        request_id: request_id.clone(),
        workspace_id: handle.workspace_id.clone(),
        query,
        payload,
    };
    let mut store = handle
        .store
        .lock()
        .map_err(|_| "Zodo handle is poisoned".to_string())?;
    let instance_id = handle.instance_id.clone();
    let mut app = Application {
        store: &mut *store,
        clock: &SystemClock,
        ids: &UuidIds::default(),
        instance_id,
    };
    let ctx = context(handle, request_id);
    match app.query(&ctx, envelope) {
        Ok(result) => serde_json::to_string(&result).map_err(|error| error.to_string()),
        Err(error) => Ok(error_json(error.message)),
    }
}

fn catch_json<T>(
    body: impl FnOnce() -> Result<T, String>,
    render: impl FnOnce(T) -> String,
) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(Ok(value)) => render(value),
        Ok(Err(message)) => error_json(message),
        Err(_) => error_json("panic inside Zodo FFI; no state was committed"),
    }
}

/// Open an existing Zodo data directory. Returns null on failure; see `zodo_ffi_last_error`.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_open(data_dir: *const c_char) -> *mut Handle {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open_inner(data_dir))) {
        Ok(Ok(handle)) => Box::into_raw(Box::new(handle)),
        Ok(Err(message)) => {
            set_last_error(message);
            std::ptr::null_mut()
        }
        Err(_) => {
            set_last_error("panic while opening Zodo store");
            std::ptr::null_mut()
        }
    }
}

/// Execute one command. Returns an allocated JSON string; caller frees with `zodo_ffi_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_command(
    handle: *mut Handle,
    operation: *const c_char,
    payload_json: *const c_char,
    idempotency_key: *const c_char,
) -> *mut c_char {
    let value = catch_json(
        || command_inner(handle, operation, payload_json, idempotency_key),
        |value| value,
    );
    ffi_string(value)
}

/// Execute one query. Returns an allocated JSON string; caller frees with `zodo_ffi_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_query(
    handle: *mut Handle,
    query: *const c_char,
    payload_json: *const c_char,
) -> *mut c_char {
    let value = catch_json(|| query_inner(handle, query, payload_json), |value| value);
    ffi_string(value)
}

/// Free a string returned by this library.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_string_free(pointer: *mut c_char) {
    if pointer.is_null() {
        return;
    }
    // SAFETY: pointer must come from `CString::into_raw` in this library.
    unsafe {
        let _ = CString::from_raw(pointer);
    }
}

/// Close a handle and release its SQLite connection.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_free(handle: *mut Handle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: handle must come from `zodo_ffi_open` and not be used after this call.
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

/// Start a durable Core run and return its JSON record. Caller frees the string.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_run_start(
    handle: *mut Handle,
    intent: *const c_char,
    input_json: *const c_char,
) -> *mut c_char {
    let value = catch_json(
        || {
            if handle.is_null() {
                return Err("null Zodo handle".to_string());
            }
            let intent = read_c(intent)?;
            let input: Value = serde_json::from_str(&read_c(input_json)?)
                .map_err(|error| format!("run input is not JSON: {error}"))?;
            // SAFETY: non-null checked above and owned by the caller for this call.
            let handle = unsafe { &*handle };
            let store = handle
                .store
                .lock()
                .map_err(|_| "Zodo handle is poisoned".to_string())?;
            let run = store
                .create_run(&handle.workspace_id, &intent, &input)
                .map_err(|error| error.message)?;
            serde_json::to_string(&run).map_err(|error| error.to_string())
        },
        |value| value,
    );
    ffi_string(value)
}

/// Poll one durable run. Caller frees the string.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_run_poll(handle: *mut Handle, run_id: *const c_char) -> *mut c_char {
    let value = catch_json(
        || {
            if handle.is_null() {
                return Err("null Zodo handle".to_string());
            }
            let run_id = read_c(run_id)?;
            // SAFETY: non-null checked above and owned by the caller for this call.
            let handle = unsafe { &*handle };
            let store = handle
                .store
                .lock()
                .map_err(|_| "Zodo handle is poisoned".to_string())?;
            let run = store
                .get_run(&run_id)
                .map_err(|error| error.message)?
                .ok_or_else(|| format!("run {run_id} not found"))?;
            serde_json::to_string(&run).map_err(|error| error.to_string())
        },
        |value| value,
    );
    ffi_string(value)
}

/// Request cancellation of a queued/running/waiting run. Caller frees the string.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_run_cancel(handle: *mut Handle, run_id: *const c_char) -> *mut c_char {
    let value = catch_json(
        || {
            if handle.is_null() {
                return Err("null Zodo handle".to_string());
            }
            let run_id = read_c(run_id)?;
            // SAFETY: non-null checked above and owned by the caller for this call.
            let handle = unsafe { &*handle };
            let store = handle
                .store
                .lock()
                .map_err(|_| "Zodo handle is poisoned".to_string())?;
            let run = store
                .request_run_cancel(&run_id)
                .map_err(|error| error.message)?
                .ok_or_else(|| format!("run {run_id} not found"))?;
            serde_json::to_string(&run).map_err(|error| error.to_string())
        },
        |value| value,
    );
    ffi_string(value)
}

/// Return the last error message for this thread as an allocated string, or null.
#[unsafe(no_mangle)]
pub extern "C" fn zodo_ffi_last_error() -> *mut c_char {
    LAST_ERROR.with(|value| {
        value
            .borrow_mut()
            .take()
            .map(ffi_string)
            .unwrap_or(std::ptr::null_mut())
    })
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use zodo_kernel::model::{Workspace, World};

    fn seed() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let mut store = SqliteStore::open(path.join("zodo.sqlite3")).unwrap();
        let now = chrono::Utc::now();
        store
            .create_workspace(&World::new(Workspace::new(
                "ws_local".to_string(),
                "FFI Test".to_string(),
                now,
            )))
            .unwrap();
        (dir, path)
    }

    fn take(pointer: *mut c_char) -> String {
        assert!(!pointer.is_null());
        // SAFETY: pointer is returned by this library and valid until freed.
        let text = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_string();
        zodo_ffi_string_free(pointer);
        text
    }

    #[test]
    fn open_command_query_free_round_trip() {
        let (_dir, path) = seed();
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let handle = zodo_ffi_open(c_path.as_ptr());
        assert!(!handle.is_null());
        let c_op = CString::new("task.create").unwrap();
        let c_payload =
            CString::new(r#"{"task":{"title":"FFI 任务","estimate_minutes":15}}"#).unwrap();
        let c_key = CString::new("ffi-test-key").unwrap();
        let created = take(zodo_ffi_command(
            handle,
            c_op.as_ptr(),
            c_payload.as_ptr(),
            c_key.as_ptr(),
        ));
        assert!(created.contains("committed"), "{created}");
        let c_query = CString::new("tasks.list").unwrap();
        let c_query_payload = CString::new("{}").unwrap();
        let listed = take(zodo_ffi_query(
            handle,
            c_query.as_ptr(),
            c_query_payload.as_ptr(),
        ));
        assert!(listed.contains("FFI 任务"), "{listed}");
        zodo_ffi_free(handle);
    }

    #[test]
    fn null_handle_and_bad_json_do_not_panic() {
        let c_op = CString::new("task.create").unwrap();
        let c_payload = CString::new("{}").unwrap();
        let response = take(zodo_ffi_command(
            std::ptr::null_mut(),
            c_op.as_ptr(),
            c_payload.as_ptr(),
            std::ptr::null(),
        ));
        assert!(response.contains("FFI_ERROR"), "{response}");

        let (_dir, path) = seed();
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let handle = zodo_ffi_open(c_path.as_ptr());
        assert!(!handle.is_null());
        let bad = CString::new("{not json").unwrap();
        let response = take(zodo_ffi_command(
            handle,
            c_op.as_ptr(),
            bad.as_ptr(),
            std::ptr::null(),
        ));
        assert!(response.contains("FFI_ERROR"), "{response}");
        zodo_ffi_free(handle);
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use std::ffi::CString;
    use zodo_kernel::model::{Workspace, World};

    fn seed() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let mut store = SqliteStore::open(path.join("zodo.sqlite3")).unwrap();
        store
            .create_workspace(&World::new(Workspace::new(
                "ws_local".to_string(),
                "FFI Run Test".to_string(),
                chrono::Utc::now(),
            )))
            .unwrap();
        (dir, path)
    }

    fn take(pointer: *mut c_char) -> String {
        assert!(!pointer.is_null());
        let text = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_string();
        zodo_ffi_string_free(pointer);
        text
    }

    #[test]
    fn run_start_poll_cancel_lifecycle() {
        let (_dir, path) = seed();
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let handle = zodo_ffi_open(c_path.as_ptr());
        assert!(!handle.is_null());
        let intent = CString::new("goal.decompose").unwrap();
        let input = CString::new(r#"{"goal_id":"g1"}"#).unwrap();
        let started = take(zodo_ffi_run_start(handle, intent.as_ptr(), input.as_ptr()));
        let run: Value = serde_json::from_str(&started).unwrap();
        assert_eq!(run["state"], "queued");
        let run_id = run["id"].as_str().unwrap().to_string();
        let c_run = CString::new(run_id.clone()).unwrap();
        let polled = take(zodo_ffi_run_poll(handle, c_run.as_ptr()));
        let polled: Value = serde_json::from_str(&polled).unwrap();
        assert_eq!(polled["state"], "queued");
        let cancelled = take(zodo_ffi_run_cancel(handle, c_run.as_ptr()));
        let cancelled: Value = serde_json::from_str(&cancelled).unwrap();
        assert_eq!(cancelled["state"], "cancelled");
        assert!(cancelled["ended_at"].is_string());
        let after = take(zodo_ffi_run_poll(handle, c_run.as_ptr()));
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["state"], "cancelled");
        zodo_ffi_free(handle);
    }
}
