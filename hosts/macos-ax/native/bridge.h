#ifndef LESELANG_AX_BRIDGE_H
#define LESELANG_AX_BRIDGE_H
#include <stddef.h>
#include <stdint.h>

enum {
    LL_AX_OK = 0,
    LL_AX_PERMISSION = 1,
    LL_AX_UNAVAILABLE = 2,
    LL_AX_INVALID = 3,
    LL_AX_STALE = 4,
    LL_AX_NOT_EDITABLE = 5,
    LL_AX_BOUNDS = 6,
    LL_AX_WRONG_THREAD = 7,
    LL_AX_API = 8,
    LL_AX_UNCERTAIN = 9
};
typedef struct {
    uint64_t identity;
    size_t document_length;
    size_t value_length;
    int32_t editable;
} LLAXSnapshot;

int32_t ll_ax_trusted(void);
int32_t ll_ax_connect(const uint8_t *path, size_t length, void **out, int32_t *detail);
int32_t ll_ax_destroy(void *context);
int32_t ll_ax_permission(void *context, int32_t *detail);
int32_t ll_ax_snapshot(void *context, uint8_t *bytes, size_t capacity,
                       LLAXSnapshot *out, int32_t *detail);
int32_t ll_ax_set_text(void *context, uint64_t identity,
                      const uint8_t *document, size_t document_length,
                      const uint8_t *expected_value, size_t expected_length,
                      int32_t expected_editable,
                      const uint8_t *text, size_t text_length, int32_t *detail);
#endif
