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

// Aligned with runtime.ts: seq is strictly monotonic (1, 2, ...) to drop out-of-order frames;
// terminal frame with closed == true is guaranteed to fire at most / exactly once.
typedef void (*AgentHubStreamCallback)(
    uint64_t seq,
    const char *event_json_utf8,
    size_t len,
    bool closed,
    void *ctx
);

typedef enum AgentHubRawImageKind {
    AGENTHUB_RAW_IMAGE_NONE = 0,
    AGENTHUB_RAW_IMAGE_PNG  = 1,
    AGENTHUB_RAW_IMAGE_JPEG = 2,
    AGENTHUB_RAW_IMAGE_TIFF = 3,
} AgentHubRawImageKind;

typedef struct AgentHubPasteboardLimits {
    size_t files, image_bytes, text_utf16_units, path_utf16_units;
} AgentHubPasteboardLimits;
AgentHubPasteboardLimits agenthub_ffi_pasteboard_limits(void);

typedef struct AgentHubRawFileUrl {
    const char *utf8_ptr;
    size_t utf8_len;
    size_t utf16_len;
} AgentHubRawFileUrl;

// Swift captures a logic-free raw snapshot; 100% validation and lossless PNG transcoding stay in Rust.
typedef struct AgentHubRawPasteboard {
    int64_t change_count_before;
    int64_t change_count_after;
    size_t total_item_count;
    const AgentHubRawFileUrl *file_urls_ptr;
    size_t file_urls_len;
    AgentHubRawImageKind image_kind;
    size_t image_declared_bytes;
    const uint8_t *image_bytes_ptr;
    size_t image_bytes_len;
    bool has_string;
    size_t string_utf16_len;
    const char *string_utf8_ptr;
    size_t string_utf8_len;
} AgentHubRawPasteboard;

// Every Host callback MUST return immediately without blocking on MainActor.
typedef struct AgentHubHostCallbacks {
    void *host_ctx;
    void (*retain_host_ctx)(void *host_ctx);
    void (*release_host_ctx)(void *host_ctx);
    void (*open_external_url)(void *host_ctx, const char *url_utf8, size_t len);
    void (*deliver_ci_notifications)(void *host_ctx, const char *events_json_utf8, size_t len);
    void (*request_app_restart)(void *host_ctx);
    void (*begin_choose_local_path)(void *host_ctx, uint64_t op_id, bool directory, const char *prompt_utf8, size_t len);
    void (*cancel_choose_local_path)(void *host_ctx, uint64_t op_id);
    void (*begin_pasteboard_change_count)(void *host_ctx, uint64_t op_id);
    void (*begin_write_pasteboard)(void *host_ctx, uint64_t op_id, const char *utf8_ptr, size_t len);
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

void agenthub_ffi_set_main_window_state(const AgentHubRuntime *runtime, bool visible, bool focused);

void agenthub_ffi_complete_choose_local_path(const AgentHubRuntime *runtime, uint64_t op_id, const char *path_or_null);
void agenthub_ffi_complete_pasteboard_change_count(const AgentHubRuntime *runtime, uint64_t op_id, int64_t count);
void agenthub_ffi_complete_write_pasteboard(const AgentHubRuntime *runtime, uint64_t op_id, bool ok);

char *agenthub_ffi_parse_gfm_markdown(const char *utf8_ptr, size_t len);

void agenthub_ffi_invoke(
    const AgentHubRuntime *runtime, const char *command_utf8, const char *args_json_utf8,
    uint64_t request_id, AgentHubRpcCallback callback, void *callback_ctx
);
void agenthub_ffi_desktop_attach(
    const AgentHubRuntime *runtime, const char *request_json_utf8, uint64_t request_id,
    AgentHubRpcCallback initial_cb, void *initial_ctx, AgentHubStreamCallback stream_cb, void *stream_ctx
);
void agenthub_ffi_desktop_attachments(
    const AgentHubRuntime *runtime, const char *request_json_utf8, uint64_t request_id,
    AgentHubRpcCallback result_cb, void *result_ctx, AgentHubStreamCallback event_cb, void *event_ctx
);
void agenthub_ffi_desktop_paste(
    const AgentHubRuntime *runtime, const char *request_json_utf8,
    const AgentHubRawPasteboard *raw_pasteboard, uint64_t request_id,
    AgentHubRpcCallback result_cb, void *result_ctx
);
void agenthub_ffi_desktop_drop_files(
    const AgentHubRuntime *runtime, const char *paths_json_utf8, double logical_x, double logical_y
);

#ifdef __cplusplus
}
#endif
