#ifndef LESELANG_CALCULATOR_BRIDGE_H
#define LESELANG_CALCULATOR_BRIDGE_H
#include <stddef.h>
#include <stdint.h>
enum { LL_CALC_BUTTONS = 4 };
typedef struct {
    uint64_t identity;
    size_t display_length;
    size_t last_expression_length;
    int32_t last_expression_present;
    uint64_t button_identities[LL_CALC_BUTTONS];
    int32_t button_enabled[LL_CALC_BUTTONS];
} LLCalcSnapshot;

int32_t ll_calc_connect(void **out, int32_t *detail);
int32_t ll_calc_snapshot(void *context, uint8_t *bytes, size_t capacity,
                         LLCalcSnapshot *out, int32_t *detail);
int32_t ll_calc_press(void *context, const LLCalcSnapshot *expected,
                      const uint8_t *display, size_t display_length,
                      const uint8_t *last_expression, size_t last_expression_length,
                      uint32_t button, int32_t *detail);
// Context destruction and permission checks use ll_ax_destroy/ll_ax_permission.
#endif
