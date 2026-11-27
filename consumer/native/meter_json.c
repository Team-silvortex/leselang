#include "meter_json.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void require(bool condition, const char *message) {
    if (!condition) {
        fprintf(stderr, "native meter: %s\n", message);
        exit(EXIT_FAILURE);
    }
}

static bool digit(char c) { return c >= '0' && c <= '9'; }

/* cJSON represents numbers as doubles and strings as NUL-terminated UTF-8.
 * Check the wire spelling before those representations discard information. */
static void check_lexemes(const char *bytes, size_t length) {
    bool quoted = false;
    unsigned depth = 0;
    for (size_t i = 0; i < length; ++i) {
        unsigned char c = (unsigned char)bytes[i];
        if (c == '"') {
            quoted = !quoted;
        } else if (quoted && c == '\\') {
            require(++i < length, "incomplete JSON escape");
            if (bytes[i] == 'u') {
                require(length - i > 4 && strspn(bytes + i + 1, "0123456789abcdefABCDEF") >= 4 &&
                        memcmp(bytes + i + 1, "0000", 4) != 0, "invalid Unicode escape");
                i += 4;
            }
        } else if (c >= 0x80) {
            unsigned count = c >= 0xf0 ? 3 : (c >= 0xe0 ? 2 : 1);
            require(quoted && c >= 0xc2 && c <= 0xf4 && length - i > count, "invalid UTF-8");
            unsigned char next = (unsigned char)bytes[i+1];
            require(!(c == 0xe0 && next < 0xa0) && !(c == 0xed && next >= 0xa0) &&
                    !(c == 0xf0 && next < 0x90) && !(c == 0xf4 && next >= 0x90), "invalid UTF-8");
            while (count-- > 0)
                require(((unsigned char)bytes[++i] & 0xc0) == 0x80, "invalid UTF-8 continuation");
        } else if (c < 0x20) {
            require(!quoted && (c == '\t' || c == '\r' || c == '\n'), "control byte in JSON");
        } else if (!quoted && digit((char)c)) {
            require(c != '0' || i + 1 == length || !digit(bytes[i+1]), "noncanonical JSON number");
            while (i + 1 < length && digit(bytes[i+1])) ++i;
            require(i + 1 == length || strchr(",}] \t\r\n", bytes[i+1]) != NULL,
                    "invalid JSON metadata number");
        } else if (!quoted) {
            require(c != '-' && c != '+' && c != '.', "invalid JSON metadata number");
            if (c == '{' || c == '[') require(++depth <= 32, "excessive JSON depth");
            if (c == '}' || c == ']') {
                require(depth > 0, "unbalanced JSON");
                --depth;
            }
        }
    }
}

static void check_keys(const cJSON *value) {
    const cJSON *child;
    if (cJSON_IsObject(value))
        require(cJSON_GetArraySize(value) <= 32, "too many fields in meter object");
    cJSON_ArrayForEach(child, value) {
        if (cJSON_IsObject(value)) {
            for (const cJSON *previous = value->child; previous != child; previous = previous->next)
                require(strcmp(previous->string, child->string) != 0, "duplicate JSON field");
        }
        check_keys(child);
    }
}

cJSON *parse_json(const char *bytes, size_t length) {
    check_lexemes(bytes, length);
    cJSON *value = cJSON_ParseWithLengthOpts(bytes, length + 1, NULL, 1);
    require(value && cJSON_IsObject(value), "expected a JSON object");
    check_keys(value);
    return value;
}

cJSON *read_fixture(const char *directory, const char *file) {
    size_t size = strlen(directory) + strlen(file) + 2;
    char *path = malloc(size);
    require(path != NULL, "allocation failed");
    snprintf(path, size, "%s/%s", directory, file);
    FILE *input = fopen(path, "rb");
    free(path);
    require(input != NULL, "cannot open fixture");
    size_t limit = 128 * 1024;
    char *data = malloc(limit + 1);
    require(data != NULL, "allocation failed");
    size_t length = fread(data, 1, limit + 1, input);
    require(!ferror(input) && length <= limit, "fixture read error or oversized fixture");
    fclose(input);
    data[length] = 0;
    cJSON *root = parse_json(data, length);
    free(data);
    return root;
}

cJSON *read_frame(size_t *length) {
    const size_t limit = 512 * 1024;
    char *bytes = malloc(limit + 1);
    require(bytes != NULL, "allocation failed");
    *length = 0;
    for (;;) {
        int c = getc(stdin);
        require(c != EOF, "worker closed without a complete frame");
        require(*length + 1 <= limit, "oversized worker frame");
        if (c == '\n') break;
        bytes[(*length)++] = (char)c;
    }
    bytes[*length] = 0;
    cJSON *frame = parse_json(bytes, *length);
    free(bytes);
    return frame;
}

void send_frame(cJSON *object, size_t max_bytes) {
    char *bytes = cJSON_PrintUnformatted(object);
    require(bytes && strlen(bytes) + 1 <= max_bytes, "oversized outbound frame");
    require(puts(bytes) >= 0 && fflush(stdout) == 0, "worker pipe closed");
    cJSON_free(bytes);
    cJSON_Delete(object);
}

cJSON *field(const cJSON *object, const char *key) {
    cJSON *item = cJSON_GetObjectItemCaseSensitive(object, key);
    require(item != NULL, key);
    return item;
}

const char *string_field(const cJSON *object, const char *key) {
    const char *text = cJSON_GetStringValue(field(object, key));
    require(text != NULL, key);
    return text;
}

unsigned number_field(const cJSON *object, const char *key, unsigned maximum) {
    const cJSON *item = field(object, key);
    require(cJSON_IsNumber(item) && item->valuedouble >= 0 && item->valuedouble <= maximum, key);
    return (unsigned)item->valuedouble;
}

uint64_t decimal(const char *text) {
    require(*text && !(text[0] == '0' && text[1]), "noncanonical decimal integer");
    uint64_t result = 0;
    for (; *text; ++text) {
        require(digit(*text), "invalid decimal integer");
        unsigned next = (unsigned)(*text - '0');
        require(result <= (UINT64_MAX - next) / 10, "integer outside u64 range");
        result = result * 10 + next;
    }
    return result;
}

void put(cJSON *object, const char *key, cJSON *item) {
    require(object && item && cJSON_AddItemToObject(object, key, item), "could not build frame");
}
