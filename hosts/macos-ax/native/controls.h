#ifndef LESELANG_AX_CONTROLS_H
#define LESELANG_AX_CONTROLS_H
#include "bridge.h"

enum { LL_CONTROLS_LIMIT = 64, LL_CONTROLS_REGISTRY_LIMIT = 128,
       LL_CONTROLS_BYTES_LIMIT = 65536 };
enum { LL_CONTROLS_DOCUMENT = 1, LL_CONTROLS_CALCULATOR = 2,
       LL_CONTROLS_PINNED_WINDOW = 3 };

// Strings are packed in row order: key, role, identifier, label, then value.
// No handles, pointers, Objective-C objects or serialized capabilities cross
// this boundary. The rows and bytes are published only after full validation.
typedef struct {
    uint64_t identity;
    size_t key_length;
    size_t role_length;
    size_t identifier_length;
    size_t label_length;
    size_t value_length;
    int32_t value_present;
    int32_t enabled;
    int32_t editable;
    int32_t pressable;
} LLControlRow;
typedef struct {
    uint64_t identity;
    uint64_t revision;
    size_t count;
    size_t bytes_length;
} LLControlsSnapshot;

int32_t ll_controls_connect(uint32_t kind, const uint8_t *path, size_t length,
                            void **out, int32_t *detail);
int32_t ll_controls_permission(void *context, int32_t *detail);
int32_t ll_controls_snapshot(void *context, LLControlRow *rows, size_t row_capacity,
                             uint8_t *bytes, size_t bytes_capacity,
                             LLControlsSnapshot *out, int32_t *detail);
int32_t ll_controls_press(void *context, uint64_t scope, uint64_t revision,
                          uint64_t target, int32_t *detail);
int32_t ll_controls_set_text(void *context, uint64_t scope, uint64_t revision,
                             uint64_t target, const uint8_t *text, size_t length,
                             int32_t *detail);
#endif
