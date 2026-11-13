#ifndef LESELANG_APPKIT_BRIDGE_H
#define LESELANG_APPKIT_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Private process-local bridge ABI. Neither slots nor identities are LeseLang
 * object handles; the Rust host wraps them in the native ObjectRef protocol. */
typedef struct LlGui LlGui;

enum {
    LL_GUI_OK = 0,
    LL_GUI_ERROR = 1,
    LL_GUI_WRONG_THREAD = 2,
    LL_GUI_CLOSED = 3,
    LL_GUI_BAD_INPUT = 4,
    LL_GUI_INVALID_UTF8 = 5,
    LL_GUI_TEXT_BOUNDS = 6,
    LL_GUI_NOT_FOUND = 7,
    LL_GUI_STALE_IDENTITY = 8,
    LL_GUI_AMBIGUOUS = 9,
    LL_GUI_TREE_BOUNDS = 10,
    LL_GUI_WIDGET_COUNT = 3,
    LL_GUI_MAX_TEXT_BYTES = 4096
};

typedef struct {
    /* A checked, nonwrapping instance token attached to the actual NSControl.
     * It is never a recyclable memory address. */
    uintptr_t identity;
    uint64_t clicks;
    /* Advances before every relevant mutation made through this adapter,
     * including same-value setters and successful target/action delivery. */
    uint64_t mutation;
    size_t identifier_length;
    size_t title_length;
    size_t value_length;
    int32_t role; /* 1: button, 2: text_field */
    int32_t enabled;
    int32_t editable;
} LlGuiSnapshot;

/* All calls, including destruction, must run on the process main thread. A
 * non-null handle must be the live handle returned by create, and may be
 * destroyed exactly once. Pointer buffers must be valid for their stated size.
 * Objective-C exceptions are caught at each boundary and return LL_GUI_ERROR;
 * native mutation before such failure is not rolled back. */
int32_t ll_gui_create(LlGui **out);
int32_t ll_gui_destroy(LlGui *gui);
int32_t ll_gui_show(LlGui *gui);
int32_t ll_gui_close(LlGui *gui);
int32_t ll_gui_is_open(LlGui *gui, int32_t *open);

/* Handles at most one pending AppKit event, without waiting for a new event or
 * resuming any language computation. Framework callbacks are trusted native
 * work; this is not a preemptive wall-clock limit on event handling. */
int32_t ll_gui_poll(LlGui *gui, int32_t *dispatched);

/* Bounded real-view-tree discovery. The returned metadata is not authority.
 * List output is NUL-terminated UTF-8 identifiers; count and aggregate are bounded. */
int32_t ll_gui_list(LlGui *gui, uint8_t *buffer, size_t capacity,
                   size_t *length, size_t *count);
/* Resolves a unique registered control identifier, refusing ambiguity. */
int32_t ll_gui_find(LlGui *gui, const uint8_t *name, size_t length,
                    int32_t *slot, uintptr_t *identity);
/* Packs identifier, title and value bytes consecutively, with lengths in the
 * snapshot and no delimiters. Each string is <=4096 UTF-8 bytes. */
int32_t ll_gui_snapshot(LlGui *gui, int32_t slot, uint8_t *title,
                        size_t capacity, LlGuiSnapshot *snapshot);
/* Invokes NSButton performClick: and its actual target/action. */
int32_t ll_gui_click(LlGui *gui, int32_t slot, uintptr_t expected_identity);
/* Sets real field text and emits its native edit notification. */
int32_t ll_gui_set_text(LlGui *gui, int32_t slot, uintptr_t expected_identity,
                        const uint8_t *value, size_t length);

/* Explicit host hooks used to verify replacement and external-change fences. */
int32_t ll_gui_replace(LlGui *gui, int32_t slot);
int32_t ll_gui_set_title(LlGui *gui, int32_t slot,
                         const uint8_t *title, size_t length);
int32_t ll_gui_set_enabled(LlGui *gui, int32_t slot, int32_t enabled);
int32_t ll_gui_set_editable(LlGui *gui, int32_t slot, int32_t editable);
int32_t ll_gui_status(LlGui *gui, const uint8_t *text, size_t length);

#ifdef __cplusplus
}
#endif
#endif
