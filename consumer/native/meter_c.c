#include "meter_app.h"

static MeterSnapshot meter;
static bool return_string_after_write;

void meter_reset(uint64_t initial_balance, bool bad_write_result) {
    meter = (MeterSnapshot){ .balance = initial_balance };
    return_string_after_write = bad_write_result;
}

MeterValue meter_add(uint64_t amount) {
    ++meter.add_calls;
    if (amount > UINT64_MAX - meter.balance)
        return (MeterValue){ .kind = METER_REJECTED, .text = "balance_overflow" };

    meter.balance += amount;
    ++meter.committed_writes;
    if (return_string_after_write)
        return (MeterValue){ .kind = METER_STRING, .text = "wrong type" };
    return (MeterValue){ .kind = METER_INTEGER, .integer = meter.balance };
}

MeterValue meter_read(void) {
    ++meter.read_calls;
    return (MeterValue){ .kind = METER_INTEGER, .integer = meter.balance };
}

MeterSnapshot meter_snapshot(void) {
    return meter;
}
