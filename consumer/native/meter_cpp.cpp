#include "meter_app.h"

#include <limits>

namespace {
struct Meter {
    uint64_t balance;
    bool return_string_after_write;
    unsigned add_calls = 0;
    unsigned read_calls = 0;
    unsigned committed_writes = 0;
};

Meter app;
}

void meter_reset(uint64_t initial_balance, bool bad_write_result) {
    app = Meter{initial_balance, bad_write_result};
}

MeterValue meter_add(uint64_t amount) {
    ++app.add_calls;
    if (amount > std::numeric_limits<uint64_t>::max() - app.balance)
        return {METER_REJECTED, 0, "balance_overflow"};

    app.balance += amount;
    ++app.committed_writes;
    if (app.return_string_after_write)
        return {METER_STRING, 0, "wrong type"};
    return {METER_INTEGER, app.balance, nullptr};
}

MeterValue meter_read() {
    ++app.read_calls;
    return {METER_INTEGER, app.balance, nullptr};
}

MeterSnapshot meter_snapshot() {
    return {app.balance, app.add_calls, app.read_calls, app.committed_writes};
}
