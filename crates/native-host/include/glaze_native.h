#ifndef GLAZE_NATIVE_H
#define GLAZE_NATIVE_H
#include <stddef.h>
#include <stdint.h>

#ifdef _WIN32
#define MP_EXPORT __declspec(dllexport)
#else
#define MP_EXPORT
#endif
#ifdef __cplusplus
extern "C" {
#endif

/* ABI 1, native pointer width, C calling convention. Exactly three exports.
 * init and call run serially on the host's dispatch thread. Return 0 on success;
 * nonzero is a module error. Never unwind/throw across this boundary.
 * The host table and context remain valid until process exit: copy or retain it.
 * emit may be called concurrently from any thread, including during init/call.
 * Event name is NUL-terminated UTF-8; bytes are borrowed only until emit returns.
 * emit copies the data, is thread-safe, and may block on pipe backpressure.
 * All names must be UTF-8. Payloads are opaque bytes (maximum 8 MiB per frame).
 * A null byte pointer is valid only when its length is zero.
 * No shutdown/unload ABI: modules stay loaded until their host process exits.
 */
typedef struct mp_host {
    uint32_t abi_version;
    void *context;
    void (*emit)(void *context, const char *event_name,
                 const uint8_t *bytes, size_t len);
} mp_host;

MP_EXPORT int mp_native_init(const mp_host *host);
/* method/input are borrowed for this call. Initialize *out=NULL, *out_len=0.
 * A module owns output allocation. On success the host copies it, then calls
 * mp_native_free(out, out_len) on the same dispatch thread. On failure, outputs
 * must remain NULL/0. free is also called for an empty successful response.
 * Calls/init have a deadline; expiry terminates the entire hosting process.
 */
MP_EXPORT int mp_native_call(const char *method, const uint8_t *in, size_t in_len,
                            uint8_t **out, size_t *out_len);
MP_EXPORT void mp_native_free(uint8_t *out, size_t out_len);

#ifdef __cplusplus
}
#endif
#endif
