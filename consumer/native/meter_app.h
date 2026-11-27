#ifndef LESE_CONSUMER_METER_APP_H
#define LESE_CONSUMER_METER_APP_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Each host executable links its own application behind these test callbacks. */
typedef enum { METER_INTEGER, METER_STRING, METER_REJECTED } MeterKind;

typedef struct {
    MeterKind kind;
    uint64_t integer;
    const char *text; /* A static string result or rejection code. */
} MeterValue;

typedef struct {
    uint64_t balance;
    unsigned add_calls;
    unsigned read_calls;
    unsigned committed_writes;
} MeterSnapshot;

void meter_reset(uint64_t initial_balance, bool bad_write_result);
MeterValue meter_add(uint64_t amount);
MeterValue meter_read(void);
MeterSnapshot meter_snapshot(void);

#ifdef __cplusplus
}
#endif

#endif
