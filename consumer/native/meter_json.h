/* Bounded JSON helpers local to the native meter conformance peers. */
#ifndef LESE_CONSUMER_METER_JSON_H
#define LESE_CONSUMER_METER_JSON_H
#include <cjson/cJSON.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

void require(bool condition, const char *message);
cJSON *parse_json(const char *bytes, size_t length);
cJSON *read_fixture(const char *directory, const char *file);
cJSON *read_frame(size_t *length);
void send_frame(cJSON *object, size_t max_bytes);
cJSON *field(const cJSON *object, const char *key);
const char *string_field(const cJSON *object, const char *key);
unsigned number_field(const cJSON *object, const char *key, unsigned maximum);
uint64_t decimal(const char *text);
void put(cJSON *object, const char *key, cJSON *item);
#endif
