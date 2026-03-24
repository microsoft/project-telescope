/**
 * Telescope Collector SDK — C API
 *
 * This header declares the C-ABI functions exported by the
 * telescope-collector-sdk shared library (cdylib).
 *
 * Two usage patterns:
 *
 * 1. Low-level:
 *      handle = telescope_sdk_init(manifest_json);
 *      telescope_sdk_submit(handle, events_json, &response_json);
 *      telescope_sdk_heartbeat(handle);
 *      telescope_sdk_shutdown(handle);
 *
 * 2. Callback-based:
 *      telescope_sdk_run(manifest_json, collect_fn, context, interval_secs);
 *
 * Link against:
 *   Windows:  telescope_collector_sdk.dll
 *   Linux:    libtelescope_collector_sdk.so
 *   macOS:    libtelescope_collector_sdk.dylib
 */

#ifndef TELESCOPE_COLLECTOR_SDK_H
#define TELESCOPE_COLLECTOR_SDK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Error Codes ── */

#define TELESCOPE_ERR_INVALID_JSON     (-1)
#define TELESCOPE_ERR_CONNECT_FAILED   (-2)
#define TELESCOPE_ERR_REGISTER_FAILED  (-3)
#define TELESCOPE_ERR_SUBMIT_FAILED    (-4)
#define TELESCOPE_ERR_HEARTBEAT_FAILED (-5)
#define TELESCOPE_ERR_SHUTDOWN_FAILED  (-6)
#define TELESCOPE_ERR_INVALID_HANDLE   (-7)
#define TELESCOPE_ERR_NULL_POINTER     (-8)

/* ── Lifecycle ── */

/**
 * Initialize the SDK: connect to the Telescope service and register.
 *
 * @param manifest_json  JSON string: {"name": "...", "version": "...",
 *                       "description": "...", "provenance": {"collector_type": "...",
 *                       "capture_method": "..."}}
 * @return  Positive handle on success, negative error code on failure.
 */
int64_t telescope_sdk_init(const char* manifest_json);

/**
 * Submit events to the Telescope service.
 *
 * @param handle         Handle returned by telescope_sdk_init.
 * @param events_json    JSON array of EventKind objects.
 * @param response_json  OUT: pointer to response JSON string.
 *                       Contains {"accepted": N, "delay_hint_ms": M}.
 *                       Caller must free with telescope_sdk_free().
 * @return  0 on success, negative error code on failure.
 */
int32_t telescope_sdk_submit(int64_t handle, const char* events_json,
                             char** response_json);

/**
 * Send a heartbeat to the Telescope service.
 *
 * @param handle  Handle returned by telescope_sdk_init.
 * @return  0 on success, negative error code on failure.
 */
int32_t telescope_sdk_heartbeat(int64_t handle);

/**
 * Shutdown: deregister, disconnect, and free resources.
 *
 * @param handle  Handle returned by telescope_sdk_init.
 * @return  0 on success, negative error code on failure.
 */
int32_t telescope_sdk_shutdown(int64_t handle);

/**
 * Free a string allocated by the SDK.
 *
 * @param ptr  String pointer from telescope_sdk_submit response_json.
 *             Safe to pass NULL.
 */
void telescope_sdk_free(char* ptr);

/* ── Callback-based Collect Loop ── */

/**
 * Callback type for the high-level collect loop.
 *
 * Called every interval_secs. Should return a JSON array of EventKind objects
 * as a null-terminated string, or NULL to signal shutdown.
 *
 * @param context  Opaque pointer passed through from telescope_sdk_run.
 * @return  JSON array string (caller-owned), or NULL to stop.
 */
typedef const char* (*telescope_collect_fn)(void* context);

/**
 * Run a collector with a callback-based collect loop.
 *
 * Connects, registers, and calls collect_fn every interval_secs.
 * Blocks until collect_fn returns NULL or SIGTERM/Ctrl-C.
 *
 * @param manifest_json  Collector manifest JSON (same as telescope_sdk_init).
 * @param collect_fn     Callback function.
 * @param context        Opaque pointer passed to collect_fn.
 * @param interval_secs  Seconds between collect_fn calls.
 * @return  0 on clean shutdown, negative error code on failure.
 */
int32_t telescope_sdk_run(const char* manifest_json,
                          telescope_collect_fn collect_fn,
                          void* context,
                          uint32_t interval_secs);

#ifdef __cplusplus
}
#endif

#endif /* TELESCOPE_COLLECTOR_SDK_H */
