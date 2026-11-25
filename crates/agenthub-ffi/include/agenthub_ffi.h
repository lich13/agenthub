#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct AgentHubRuntime AgentHubRuntime;

typedef struct AgentHubCommandAccess {
    bool remote;
    bool read_only;
} AgentHubCommandAccess;

// Returns the registered access for this command. hook_status selects the read-only hooks query.
AgentHubCommandAccess agenthub_ffi_command_access(const char *command_utf8, bool hook_status);

// Callback buffer (utf8_ptr, len) is valid only for the duration of the callback frame;
// never dereference utf8_ptr when len == 0.
typedef void (*AgentHubRpcCallback)(
    uint64_t request_id,
    bool ok,
    const char *utf8_ptr,
    size_t len,
    void *ctx
);

// Every Host callback MUST return immediately without blocking on MainActor.
typedef struct AgentHubHostCallbacks {
    void *host_ctx;
    void (*retain_host_ctx)(void *host_ctx);
    void (*release_host_ctx)(void *host_ctx);
    void (*deliver_ci_notifications)(void *host_ctx, const char *events_json_utf8, size_t len);
    void (*request_app_restart)(void *host_ctx);
} AgentHubHostCallbacks;

int32_t agenthub_ffi_init(
    const char *resource_dir_utf8,
    const char *isolated_root_utf8,
    AgentHubHostCallbacks callbacks,
    AgentHubRuntime **out_runtime,
    char **out_error_utf8
);
void agenthub_ffi_shutdown(AgentHubRuntime *runtime);
void agenthub_ffi_string_free(char *ptr);

char *agenthub_ffi_parse_gfm_markdown(const char *utf8_ptr, size_t len);

void agenthub_ffi_invoke(
    const AgentHubRuntime *runtime, const char *command_utf8, const char *args_json_utf8,
    uint64_t request_id, AgentHubRpcCallback callback, void *callback_ctx
);

#ifdef __cplusplus
}
#endif
