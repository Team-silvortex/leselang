/* A peer for the shared scalar fixtures. Its stdin and stdout connect directly
 * to one worker. The enclosing runner supervises both process exit statuses. */
#include "meter_app.h"
#include "meter_json.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    cJSON *success, *failures;
    const cJSON *grants, *expected;
    const char *id, *source;
    char session[33], last_challenge[65];
    unsigned requests, accepted;
    bool success_case;
} Run;

static bool same(const char *a, const char *b) { return strcmp(a, b) == 0; }

static void shape(const cJSON *object, int fields) {
    require(cJSON_IsObject(object) && cJSON_GetArraySize(object) == fields, "unexpected object shape");
}

static bool granted(const cJSON *grants, const char *capability) {
    const cJSON *item;
    cJSON_ArrayForEach(item, grants)
        if (cJSON_IsString(item) && same(item->valuestring, capability)) return true;
    return false;
}

static void open_run(Run *run, const char *directory, const char *id, const char *session) {
    require(strlen(session) == 32 && strspn(session, "0123456789abcdef") == 32, "invalid session ID");
    memcpy(run->session, session, sizeof(run->session));
    run->id = id;
    run->success = read_fixture(directory, "two-effects.json");
    run->failures = read_fixture(directory, "failure-vectors.json");
    require(same(string_field(run->failures, "entry"), "main"), "unexpected fixture entry");
    run->success_case = same(id, "two-effects");
    const cJSON *sample = run->success;
    bool invalid_result = false;
    if (!run->success_case) {
        require(same(id, "missing-grant") || same(id, "post-write-invalid-result"), "unknown case ID");
        sample = NULL;
        const cJSON *item;
        cJSON_ArrayForEach(item, field(run->failures, "cases")) {
            if (same(string_field(item, "id"), id)) {
                require(!sample, "duplicate fixture case");
                sample = item;
            }
        }
        require(sample != NULL, "missing failure fixture case");
        const char *mode = string_field(sample, "write_reply");
        invalid_result = same(mode, "string");
        require(invalid_result || same(mode, "balance"), "unknown reply mode");
    }
    run->source = string_field(run->success_case ? run->success : run->failures, "source");
    run->grants = field(sample, "grants");
    require(cJSON_IsArray(run->grants), "invalid grants");
    run->expected = field(sample, "expected");
    meter_reset(decimal(string_field(run->failures, "initial_balance")), invalid_result);
}

static cJSON *envelope(const char *kind) {
    cJSON *object = cJSON_CreateObject();
    put(object, "agent_worker_version", cJSON_CreateNumber(4));
    put(object, "kind", cJSON_CreateString(kind));
    return object;
}

static void add_scope(cJSON *object, const Run *run) {
    put(object, "window", cJSON_CreateString("meter"));
    put(object, "epoch", cJSON_CreateString("native"));
    put(object, "session", cJSON_CreateString(run->session));
}

static void start(const Run *run) {
    cJSON *outer = envelope("start");
    cJSON *scope = cJSON_CreateObject();
    add_scope(scope, run);
    put(outer, "scope", scope);
    put(outer, "profile", cJSON_CreateString("lese.operations"));
    put(outer, "source", cJSON_CreateString(run->source));
    put(outer, "arguments", run->success_case ? cJSON_Duplicate(field(run->success, "arguments"), 1)
                                              : cJSON_CreateObject());
    put(outer, "operations", cJSON_Duplicate(field(run->success, "operations"), 1));
    put(outer, "grants", cJSON_Duplicate(run->grants, 1));
    send_frame(outer, 128 * 1024);
}

static cJSON *decimal_string(uint64_t value) {
    char text[21];
    snprintf(text, sizeof(text), "%" PRIu64, value);
    return cJSON_CreateString(text);
}

static void dispatch(Run *run, const cJSON *frame, size_t length) {
    require(length + 1 <= 64 * 1024, "oversized request envelope");
    shape(frame, 4);
    const char *challenge = string_field(frame, "challenge");
    require(strlen(challenge) == 64 && strspn(challenge, "0123456789abcdef") == 64 &&
            !same(challenge, run->last_challenge), "bad or reused challenge");
    const cJSON *request = field(frame, "request");
    char *encoded = cJSON_PrintUnformatted(request);
    require(encoded && strlen(encoded) <= 32 * 1024, "oversized inner request");
    cJSON_free(encoded);
    shape(request, 8);
    require(number_field(request, "transport_version", 1) == 1 &&
            number_field(request, "request_id", 2) == run->requests + 1 &&
            number_field(request, "budget_ms", 5000) >= 1, "invalid request correlation or budget");
    require(same(string_field(request, "window"), "meter") &&
            same(string_field(request, "epoch"), "native") &&
            same(string_field(request, "session"), run->session), "foreign request scope");

    const char *operation = string_field(request, "operation");
    const cJSON *args = field(request, "arguments");
    MeterValue value;
    if (run->requests == 0) {
        require(same(operation, "meter.add") && granted(run->grants, "meter.write"), "unexpected write");
        shape(args, 1);
        value = meter_add(decimal(string_field(args, "amount")));
    } else {
        require(run->requests == 1 && run->accepted == 1 &&
                same(operation, "meter.read") && granted(run->grants, "meter.read"), "unexpected read");
        shape(args, 0);
        value = meter_read();
    }
    ++run->requests;
    memcpy(run->last_challenge, challenge, sizeof(run->last_challenge));
    cJSON *reply = cJSON_CreateObject();
    add_scope(reply, run);
    put(reply, "transport_version", cJSON_CreateNumber(1));
    put(reply, "request_id", cJSON_CreateNumber(run->requests));
    if (value.kind == METER_INTEGER) {
        put(reply, "status", cJSON_CreateString("ok"));
        put(reply, "value", decimal_string(value.integer));
        put(reply, "error", cJSON_CreateNull());
        ++run->accepted;
    } else {
        bool rejected = value.kind == METER_REJECTED;
        put(reply, "status", cJSON_CreateString(rejected ? "rejected" : "uncertain"));
        put(reply, "value", cJSON_CreateNull());
        put(reply, "error", cJSON_CreateString(rejected ? value.text : "operation_failed"));
    }
    cJSON *outer = envelope("reply");
    put(outer, "challenge", cJSON_CreateString(challenge));
    put(outer, "reply", reply);
    send_frame(outer, 64 * 1024);
}

static void verify_report(const Run *run, const cJSON *frame, cJSON *observed, MeterSnapshot app) {
    const char *text = string_field(frame, "report_json");
    require(strlen(text) <= 128 * 1024, "oversized report");
    cJSON *report = parse_json(text, strlen(text));
    shape(report, 6);
    require(number_field(report, "schema_version", 1) == 1 &&
            same(string_field(report, "kind"), "operation_task_run") &&
            same(string_field(report, "profile"), "lese.operations"), "unexpected report identity");
    const cJSON *result = field(report, "result");
    shape(result, 2);
    require(same(string_field(result, "type"), "integer") &&
            decimal(string_field(result, "value")) == app.balance, "report differs from application balance");
    unsigned effects = number_field(report, "effects", 2);
    unsigned fuel = number_field(report, "fuel_remaining", 10000);
    require(run->accepted == effects && effects == number_field(run->expected, "effects", 2) &&
            fuel == number_field(run->expected, "fuel_remaining", 10000) &&
            cJSON_Compare(result, field(run->expected, "result"), 1), "report differs from fixture");
    require(app.add_calls == 1 && app.read_calls == 1 && app.committed_writes == 1, "incorrect meter counts");
    put(observed, "effects", cJSON_CreateNumber(effects));
    put(observed, "fuel_remaining", cJSON_CreateNumber(fuel));
    put(observed, "result", cJSON_Duplicate(result, 1));
    cJSON_Delete(report);
}

static void finish(const Run *run, const cJSON *frame) {
    shape(frame, 3);
    const char *kind = string_field(frame, "kind");
    MeterSnapshot app = meter_snapshot();
    cJSON *observed = cJSON_CreateObject();
    put(observed, "outcome", cJSON_CreateString(run->success_case ? "reported" : "failed"));
    put(observed, "balance", decimal_string(app.balance));
    put(observed, "add_calls", cJSON_CreateNumber(app.add_calls));
    put(observed, "read_calls", cJSON_CreateNumber(app.read_calls));
    put(observed, "committed_writes", cJSON_CreateNumber(app.committed_writes));
    if (run->success_case) {
        require(same(kind, "report"), "expected a successful worker report");
        verify_report(run, frame, observed, app);
    } else {
        require(same(kind, "error"), "expected a failed worker");
        const char *diagnostic = string_field(frame, "error");
        require(*diagnostic && strlen(diagnostic) <= 1024, "invalid terminal diagnostic");
        require(cJSON_Compare(observed, run->expected, 1), "failure effects differ from fixture");
    }
    require(getc(stdin) == EOF && !ferror(stdin), "worker wrote data after its terminal frame");

    cJSON *row = cJSON_CreateObject();
    put(row, "id", cJSON_CreateString(run->id));
    put(row, "observed", observed);
    char *json = cJSON_PrintUnformatted(row);
    require(json && fprintf(stderr, "LESE_METER %s\n", json) > 0, "could not emit observation");
    cJSON_free(json);
    cJSON_Delete(row);
}

int main(int argc, char **argv) {
    require(argc == 4, "usage: native-meter FIXTURE_DIR CASE_ID SESSION");
    Run run = {0};
    open_run(&run, argv[1], argv[2], argv[3]);
    start(&run);
    for (;;) {
        size_t length;
        cJSON *frame = read_frame(&length);
        require(number_field(frame, "agent_worker_version", 4) == 4, "unsupported worker version");
        bool request = same(string_field(frame, "kind"), "request");
        if (request) dispatch(&run, frame, length);
        else finish(&run, frame);
        cJSON_Delete(frame);
        if (!request) break;
    }
    cJSON_Delete(run.success);
    cJSON_Delete(run.failures);
    return EXIT_SUCCESS;
}
