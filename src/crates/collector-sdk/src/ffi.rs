//! C-ABI interface for non-Rust collectors.
//!
//! Two usage patterns:
//!
//! 1. **Low-level**: `telescope_sdk_init` → `telescope_sdk_submit` / `telescope_sdk_heartbeat`
//!    in your own loop → `telescope_sdk_shutdown`.
//!
//! 2. **High-level (callback-based)**: `telescope_sdk_run` with a callback function —
//!    the SDK manages the collect loop.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use tracing::{error, info};

use telescope_collector_types::ipc::collector_protocol::SubmitResponse;
use telescope_collector_types::ipc::{IpcChannel, IpcRequest, IpcStream};

/// Next handle ID.
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);

/// Active SDK handles (`handle_id` → runtime + stream).
static HANDLES: std::sync::LazyLock<Mutex<HashMap<i64, SdkHandle>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

struct SdkHandle {
    runtime: tokio::runtime::Runtime,
    stream: IpcStream,
    collector_id: String,
}

// ── Error codes ──

const ERR_INVALID_JSON: i32 = -1;
const ERR_CONNECT_FAILED: i32 = -2;
const ERR_REGISTER_FAILED: i32 = -3;
const ERR_SUBMIT_FAILED: i32 = -4;
const ERR_HEARTBEAT_FAILED: i32 = -5;
const ERR_INVALID_HANDLE: i32 = -7;
const ERR_NULL_POINTER: i32 = -8;

// ── Lifecycle ──

/// Initialize the SDK: connect to the Telescope service and register.
///
/// `manifest_json`: JSON string with fields `name`, `version`, `description`,
/// and `provenance` (object with `collector_type`, `confidence`, `capture_method`).
///
/// Returns a positive handle on success, or a negative error code on failure.
///
/// # Safety
///
/// `manifest_json` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telescope_sdk_init(manifest_json: *const c_char) -> i64 {
    if manifest_json.is_null() {
        return i64::from(ERR_NULL_POINTER);
    }

    let Ok(json_str) = (unsafe { CStr::from_ptr(manifest_json) }).to_str() else {
        return i64::from(ERR_INVALID_JSON);
    };

    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return i64::from(ERR_INVALID_JSON);
    };

    let Ok(rt) = tokio::runtime::Runtime::new() else {
        return i64::from(ERR_CONNECT_FAILED);
    };

    let result = rt.block_on(async {
        let channel = resolve_channel();
        let mut stream = IpcStream::connect(&channel)
            .await
            .map_err(|_| ERR_CONNECT_FAILED)?;

        let name = manifest["name"].as_str().unwrap_or("unknown");
        let request = IpcRequest::new(
            "collector.register",
            serde_json::json!({
                "name": name,
                "version": manifest["version"].as_str().unwrap_or("0.0.0"),
                "description": manifest["description"].as_str().unwrap_or(""),
                "provenance": manifest.get("provenance").cloned().unwrap_or(serde_json::json!({
                    "collector_type": "manual",
                    "confidence": 0.5,
                    "capture_method": "volunteered"
                })),
                "pid": std::process::id(),
            }),
        );

        let response = stream
            .call(&request)
            .await
            .map_err(|_| ERR_REGISTER_FAILED)?;
        if response.is_error() {
            return Err(ERR_REGISTER_FAILED);
        }

        Ok((stream, name.to_string()))
    });

    match result {
        Ok((stream, collector_id)) => {
            let handle_id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
            let handle = SdkHandle {
                runtime: rt,
                stream,
                collector_id,
            };
            HANDLES
                .lock()
                .expect("lock poisoned")
                .insert(handle_id, handle);
            info!(handle = handle_id, "SDK initialized");
            handle_id
        }
        Err(code) => i64::from(code),
    }
}

/// Submit events to the Telescope service.
///
/// `events_json`: JSON array of `EventKind` objects.
/// `response_json`: OUT parameter — pointer to a `char*` that will be set to a
/// JSON string with `{"accepted": N, "delay_hint_ms": M}`. Caller must free
/// with `telescope_sdk_free`.
///
/// Returns 0 on success, negative error code on failure.
///
/// # Safety
///
/// - `events_json` must be a valid null-terminated C string.
/// - `response_json` must point to a valid `*mut c_char` location.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telescope_sdk_submit(
    handle: i64,
    events_json: *const c_char,
    response_json: *mut *mut c_char,
) -> i32 {
    if events_json.is_null() || response_json.is_null() {
        return ERR_NULL_POINTER;
    }

    let Ok(json_str) = (unsafe { CStr::from_ptr(events_json) }).to_str() else {
        return ERR_INVALID_JSON;
    };

    let Ok(events) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) else {
        return ERR_INVALID_JSON;
    };

    let mut handles = HANDLES.lock().expect("lock poisoned");
    let Some(h) = handles.get_mut(&handle) else {
        return ERR_INVALID_HANDLE;
    };

    let result: Result<SubmitResponse, i32> = h.runtime.block_on(async {
        let request = IpcRequest::new("collector.submit", serde_json::json!({ "events": events }));

        let response = h
            .stream
            .call(&request)
            .await
            .map_err(|_| ERR_SUBMIT_FAILED)?;
        if response.is_error() {
            return Err(ERR_SUBMIT_FAILED);
        }

        serde_json::from_value(response.result.ok_or(ERR_SUBMIT_FAILED)?)
            .map_err(|_| ERR_SUBMIT_FAILED)
    });

    match result {
        Ok(resp) => {
            let resp_str = serde_json::to_string(&resp).unwrap_or_default();
            match CString::new(resp_str) {
                Ok(c) => {
                    unsafe { *response_json = c.into_raw() };
                    0
                }
                Err(_) => ERR_SUBMIT_FAILED,
            }
        }
        Err(code) => code,
    }
}

/// Send a heartbeat to the Telescope service.
///
/// Returns 0 on success, negative error code on failure.
#[unsafe(no_mangle)]
pub extern "C" fn telescope_sdk_heartbeat(handle: i64) -> i32 {
    let mut handles = HANDLES.lock().expect("lock poisoned");
    let Some(h) = handles.get_mut(&handle) else {
        return ERR_INVALID_HANDLE;
    };

    let result = h.runtime.block_on(async {
        let request = IpcRequest::simple("collector.heartbeat");
        let response = h
            .stream
            .call(&request)
            .await
            .map_err(|_| ERR_HEARTBEAT_FAILED)?;
        if response.is_error() {
            return Err(ERR_HEARTBEAT_FAILED);
        }
        Ok(())
    });

    match result {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// Shutdown: deregister from the service, disconnect, and free resources.
///
/// Returns 0 on success, negative error code on failure.
#[unsafe(no_mangle)]
pub extern "C" fn telescope_sdk_shutdown(handle: i64) -> i32 {
    let mut handles = HANDLES.lock().expect("lock poisoned");
    let Some(mut h) = handles.remove(&handle) else {
        return ERR_INVALID_HANDLE;
    };
    drop(handles);

    let result = h.runtime.block_on(async {
        let request = IpcRequest::simple("collector.deregister");
        let _ = h.stream.call(&request).await;
        Ok::<_, i32>(())
    });

    info!(handle, collector = %h.collector_id, "SDK shutdown");

    match result {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// Free a string allocated by the SDK (e.g., `response_json` from submit).
///
/// # Safety
///
/// `ptr` must have been allocated by the SDK via `CString::into_raw()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telescope_sdk_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

// ── Callback-based collect loop ──

/// Callback type for the high-level collect loop.
///
/// The SDK calls this function every `interval_secs` seconds.
/// It should return a JSON array of `EventKind` objects as a null-terminated
/// C string, or null to signal shutdown. The returned string is owned by the
/// caller (SDK will not free it).
pub type CollectFn = unsafe extern "C" fn(context: *mut std::ffi::c_void) -> *const c_char;

/// Run a collector with a callback-based collect loop.
///
/// Connects to the service, registers, and calls `collect_fn` every `interval_secs`.
/// Blocks until `collect_fn` returns null or the process receives SIGTERM/Ctrl-C.
///
/// Returns 0 on clean shutdown, negative error code on failure.
///
/// # Safety
///
/// - `manifest_json` must be a valid null-terminated C string.
/// - `collect_fn` must be a valid function pointer.
/// - `context` is passed through to `collect_fn` unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn telescope_sdk_run(
    manifest_json: *const c_char,
    collect_fn: CollectFn,
    context: *mut std::ffi::c_void,
    interval_secs: u32,
) -> i32 {
    let handle = unsafe { telescope_sdk_init(manifest_json) };
    if handle < 0 {
        #[allow(clippy::cast_possible_truncation)]
        return handle as i32;
    }

    let interval = std::time::Duration::from_secs(u64::from(interval_secs));
    let mut delay_hint = std::time::Duration::ZERO;

    loop {
        std::thread::sleep(interval + delay_hint);

        let raw_events = unsafe { collect_fn(context) };
        if raw_events.is_null() {
            break;
        }

        let Ok(raw_str) = (unsafe { CStr::from_ptr(raw_events) }).to_str() else {
            error!("collect_fn returned invalid UTF-8");
            continue;
        };

        if raw_str == "[]" {
            continue;
        }

        let Ok(events_cstring) = CString::new(raw_str) else {
            continue;
        };

        let mut response_ptr: *mut c_char = std::ptr::null_mut();
        let rc =
            unsafe { telescope_sdk_submit(handle, events_cstring.as_ptr(), &raw mut response_ptr) };
        if rc != 0 {
            error!(rc, "submit failed in callback loop");
            break;
        }

        // Parse delay_hint from response.
        if !response_ptr.is_null() {
            if let Ok(s) = unsafe { CStr::from_ptr(response_ptr) }.to_str()
                && let Ok(resp) = serde_json::from_str::<SubmitResponse>(s)
            {
                delay_hint = std::time::Duration::from_millis(resp.delay_hint_ms);
            }
            unsafe { telescope_sdk_free(response_ptr) };
        }
    }

    telescope_sdk_shutdown(handle)
}

/// Resolve the collector IPC channel (respects `TELESCOPE_COLLECTOR_PIPE` env).
fn resolve_channel() -> IpcChannel {
    if let Ok(path) = std::env::var("TELESCOPE_COLLECTOR_PIPE") {
        IpcChannel {
            name: "collector".to_string(),
            path: std::path::PathBuf::from(path),
        }
    } else {
        IpcChannel::collector()
    }
}
