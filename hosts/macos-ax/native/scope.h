#ifndef LESELANG_AX_SCOPE_H
#define LESELANG_AX_SCOPE_H
#include "controls.h"

enum { LL_WINDOWS_LIMIT = 64, LL_WINDOWS_REGISTRY_LIMIT = 128,
       LL_WINDOWS_BYTES_LIMIT = 65536 };
// Packed strings follow row order: key, title, identifier, document. Keys are
// scoped lookup metadata, never transferable handles or native capabilities.
typedef struct {
    uint64_t identity;
    size_t key_length;
    size_t title_length;
    size_t identifier_length;
    size_t document_length;
    int32_t document_present;
} LLWindowRow;
typedef struct { size_t count; size_t bytes_length; } LLWindowInventory;

int32_t ll_application_connect(const uint8_t *bundle, size_t bundle_length,
                                int32_t selected_pid, void **out,
                                int32_t *actual_pid, int32_t *detail);
int32_t ll_application_windows(void *context, LLWindowRow *rows, size_t row_capacity,
                                uint8_t *bytes, size_t byte_capacity,
                                LLWindowInventory *out, int32_t *detail);
int32_t ll_application_controls(void *context,
    const uint8_t *key, size_t key_length,
    const uint8_t *title, size_t title_length,
    const uint8_t *identifier, size_t identifier_length,
    // Optional absolute local document path (regular file or directory package).
    const uint8_t *document, size_t document_length,
    void **out, int32_t *detail);
#endif
