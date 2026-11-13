#ifndef LESELANG_AX_WIDGETS_H
#define LESELANG_AX_WIDGETS_H
#include "controls.h"
#include "scope.h"

enum { LL_WIDGET_OPEN_MENU = 1, LL_WIDGET_CHOOSE_MENU = 2, LL_WIDGET_CANCEL_MENU = 4,
       LL_WIDGET_SET_CHECKED = 8, LL_WIDGET_SELECT_ITEM = 16, LL_WIDGET_SCROLL = 32 };
enum { LL_WIDGET_PRESS = 1, LL_WIDGET_SET_TEXT = 2, LL_WIDGET_OPEN = 3,
       LL_WIDGET_CHOOSE = 4, LL_WIDGET_CANCEL = 5, LL_WIDGET_CHECK = 6,
       LL_WIDGET_SELECT = 7, LL_WIDGET_POSITION = 8 };
// String order per row: key, role, identifier, label, value, orientation.
typedef struct {
    LLControlRow control;
    uint64_t parent;
    int32_t checked; // -1 absent, 0 unchecked, 1 checked, 2 mixed
    int32_t selected; // -1 absent, 0 false, 1 true
    int32_t position; // -1 absent, otherwise 0..1000
    uint32_t capabilities;
    size_t orientation_length;
} LLWidgetRow;
enum { LL_AUXILIARY_LIMIT = 4, LL_AUXILIARY_BYTES_LIMIT = 16384,
       LL_WIDGET_NODE_LIMIT = 512, LL_WIDGET_DEPTH_LIMIT = 24 };
// Explicit immutable process identity; paths must already be canonical.
typedef struct {
    int32_t pid;
    uint64_t start_seconds;
    uint32_t start_micros;
    const uint8_t *executable;
    size_t executable_length;
} LLAuxiliarySpec;
typedef struct {
    int32_t pid;
    uint64_t start_seconds;
    uint32_t start_micros;
    size_t executable_length;
    size_t edge_count;
} LLAuxiliaryRow;
typedef struct { size_t count; size_t bytes_length; } LLAuxiliaryInventory;
// Synchronous, non-reentrant, read-only callback; owner must outlive context.
typedef int32_t (*LLAuxiliaryGrantCheck)(const void *context);
int32_t ll_application_auxiliary_processes(void *application,
    const uint8_t *key, size_t key_length, const uint8_t *title, size_t title_length,
    const uint8_t *identifier, size_t identifier_length,
    const uint8_t *document, size_t document_length,
    LLAuxiliaryRow *rows, size_t row_capacity, uint8_t *bytes, size_t byte_capacity,
    LLAuxiliaryInventory *out, int32_t *detail);
int32_t ll_application_widgets(void *application, const uint8_t *key, size_t key_length,
    const uint8_t *title, size_t title_length, const uint8_t *identifier, size_t identifier_length,
    const uint8_t *document, size_t document_length, void **out, int32_t *detail);
int32_t ll_application_widgets_with_auxiliary(void *application,
    const uint8_t *key, size_t key_length, const uint8_t *title, size_t title_length,
    const uint8_t *identifier, size_t identifier_length,
    const uint8_t *document, size_t document_length,
    const LLAuxiliarySpec *specifications, size_t count,
    LLAuxiliaryGrantCheck check, const void *grant_context,
    void **out, int32_t *detail);
int32_t ll_widgets_permission(void *context, int32_t *detail);
int32_t ll_widgets_revoke(void *context, int32_t *detail);
int32_t ll_widgets_snapshot(void *context, LLWidgetRow *rows, size_t row_capacity,
    uint8_t *bytes, size_t byte_capacity, LLControlsSnapshot *out, int32_t *detail);
int32_t ll_widgets_perform(void *context, uint64_t scope, uint64_t revision,
    uint64_t target, uint32_t operation, uint64_t item, int32_t number,
    const uint8_t *text, size_t text_length, int32_t *detail);
#endif
