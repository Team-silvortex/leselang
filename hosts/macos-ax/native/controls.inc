// Included after calculator.inc: private, bounded CF/AX readers are shared;
// neither older public ABI nor their deliberately narrow profiles change.
#include "controls.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

// The generic application extension defines this shared bounded URL projection.
static int scopeDocumentValue(CFTypeRef value, NSString **out);

@interface LLControlEntry : NSObject
@property(nonatomic, strong) LLCF *element;
@property(nonatomic) uint64_t identity;
@property(nonatomic, copy) NSString *key;
@property(nonatomic) BOOL alive;
@end
@implementation LLControlEntry
@end

@interface LLControlsContext : LLAXContext
@property(nonatomic) uint32_t kind;
@property(nonatomic, copy) NSString *nonce;
@property(nonatomic, copy) NSArray<LLControlEntry *> *registry;
@property(nonatomic, copy) NSArray<NSDictionary *> *rows;
@property(nonatomic, copy) NSArray<NSDictionary *> *dependencies;
@property(nonatomic, copy) NSArray<LLCF *> *tree;
@property(nonatomic, copy) NSArray<NSNumber *> *parents;
@property(nonatomic) uint64_t revision;
@property(nonatomic) BOOL retired;
@end
@implementation LLControlsContext
@end

static BOOL controlsAbsent(int result, int32_t detail) {
    return result == LL_AX_API &&
        (detail == kAXErrorAttributeUnsupported || detail == kAXErrorNoValue);
}
static BOOL controlsTextRole(NSString *role) {
    return [role isEqualToString:(__bridge NSString *)kAXTextAreaRole] ||
        [role isEqualToString:(__bridge NSString *)kAXTextFieldRole];
}
static BOOL controlsAdmittedRole(NSString *role) {
    return controlsTextRole(role) || [role isEqualToString:(__bridge NSString *)kAXButtonRole] ||
        [role isEqualToString:(__bridge NSString *)kAXStaticTextRole];
}
static BOOL controlsDescribedRole(NSString *role) {
    return [role isEqualToString:(__bridge NSString *)kAXButtonRole] ||
        [role isEqualToString:(__bridge NSString *)kAXStaticTextRole];
}
static BOOL controlsEffectiveEnabled(NSString *role, BOOL present, BOOL enabled, BOOL editable) {
    return present ? enabled : controlsTextRole(role) && editable;
}
static int controlsPermission(LLControlsContext *context, int32_t *detail) {
    if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
    if (!context) return LL_AX_INVALID;
    if (context.retired) return LL_AX_STALE;
    return permission(context, detail);
}
static int controlsSelectWindow(LLControlsContext *context, LLCF *candidate,
                                  NSString *role, NSString *key, LLCF **selected) {
    NSString *wanted = context.kind == LL_CONTROLS_DOCUMENT ? context.document : @"main";
    if (![role isEqualToString:(__bridge NSString *)kAXWindowRole] || ![key isEqualToString:wanted]) return LL_AX_OK;
    if (*selected) return LL_AX_INVALID;
    if (context.window && !sameElement(context.window, candidate)) return LL_AX_STALE;
    *selected = candidate;
    return LL_AX_OK;
}
static int controlsPinnedWindow(LLCF *pinned, NSArray<LLCF *> *windows, LLCF **out) {
    if (!pinned || !out) return LL_AX_INVALID;
    LLCF *selected = nil;
    for (NSUInteger index = 0; index < windows.count; ++index) {
        for (NSUInteger prior = 0; prior < index; ++prior) {
            if (sameElement(windows[prior], windows[index])) return LL_AX_INVALID;
        }
        if (sameElement(pinned, windows[index])) selected = windows[index];
    }
    if (!selected) return LL_AX_STALE;
    *out = selected;
    return LL_AX_OK;
}
static int controlsWindow(LLControlsContext *context, LLCF **out, int32_t *detail) {
    NSMutableArray<LLCF *> *windows = nil;
    int result = children(context.application, kAXWindowsAttribute, context.pid, &windows, detail);
    if (result) return result;
    // Generic selectors resolve once. Mutable metadata never selects a
    // replacement; the exact retained window must remain in the process.
    if (context.kind == LL_CONTROLS_PINNED_WINDOW) return controlsPinnedWindow(context.window, windows, out);
    LLCF *selected = nil;
    for (LLCF *candidate in windows) {
        NSString *role = nil;
        result = stringAttribute(candidate, kAXRoleAttribute, &role, detail);
        if (result) return result;
        if (![role isEqualToString:(__bridge NSString *)kAXWindowRole]) continue;
        NSString *key = nil;
        if (context.kind == LL_CONTROLS_DOCUMENT) {
            result = canonicalDocument(candidate, &key, detail);
            if (controlsAbsent(result, *detail)) continue;
        } else {
            result = optionalString(candidate, kAXIdentifierAttribute, &key, detail);
        }
        if (result) return result;
        result = controlsSelectWindow(context, candidate, role, key, &selected);
        if (result) return result;
    }
    if (!selected) return context.window ? LL_AX_STALE : LL_AX_UNAVAILABLE;
    if (context.window && !sameElement(context.window, selected)) return LL_AX_STALE;
    *out = selected;
    return LL_AX_OK;
}

// String values are exported without coercion. Scalar numeric/boolean values
// are kept as typed epoch dependencies only; unknown CF types fail closed.
// Missing AXValue is distinct from the present empty string.
static int controlsTypedValue(CFTypeRef value, NSString *role, NSString **stringOut,
                               id *dependencyOut, int32_t *detail) {
    if (!value) return LL_AX_INVALID;
    CFTypeID type = CFGetTypeID(value);
    if (type == CFStringGetTypeID()) {
        NSString *text = (__bridge NSString *)value;
        int result = utf8(text, NULL);
        if (result) return result;
        *stringOut = [text copy];
        *dependencyOut = @[@"string", *stringOut];
        return LL_AX_OK;
    }
    // A text role with a non-string value cannot masquerade as readable text.
    if (controlsTextRole(role) || [role isEqualToString:(__bridge NSString *)kAXStaticTextRole]) {
        *detail = -101; return LL_AX_INVALID;
    }
    *stringOut = nil;
    if (type == CFBooleanGetTypeID()) {
        *dependencyOut = @[@"boolean", @(CFBooleanGetValue((CFBooleanRef)value))];
        return LL_AX_OK;
    }
    if (type == CFNumberGetTypeID()) {
        double number = 0;
        if (!CFNumberGetValue((CFNumberRef)value, kCFNumberDoubleType, &number) || !isfinite(number)) {
            *detail = -102; return LL_AX_INVALID;
        }
        *dependencyOut = @[@"number", [(__bridge NSNumber *)value copy]];
        return LL_AX_OK;
    }
    *detail = -103;
    return LL_AX_INVALID;
}
static int controlsAttributeNames(CFTypeRef raw, NSArray<NSString *> **out, size_t *byteCount) {
    if (!raw || CFGetTypeID(raw) != CFArrayGetTypeID()) return LL_AX_INVALID;
    CFIndex count = CFArrayGetCount((CFArrayRef)raw);
    if (count < 0 || count > 128) return LL_AX_BOUNDS;
    NSMutableArray<NSString *> *names = [NSMutableArray new];
    for (CFIndex index = 0; index < count; ++index) {
        CFTypeRef item = CFArrayGetValueAtIndex((CFArrayRef)raw, index);
        if (!item || CFGetTypeID(item) != CFStringGetTypeID()) return LL_AX_INVALID;
        NSString *name = (__bridge NSString *)item;
        int result = utf8(name, NULL);
        if (result) return result;
        if (!name.length || [names containsObject:name]) return LL_AX_INVALID;
        size_t length = [name lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (*byteCount > LL_CONTROLS_BYTES_LIMIT || length > LL_CONTROLS_BYTES_LIMIT - *byteCount) return LL_AX_BOUNDS;
        *byteCount += length;
        [names addObject:[name copy]];
    }
    *out = [names sortedArrayUsingSelector:@selector(compare:)];
    return LL_AX_OK;
}
static int controlsValue(LLCF *candidate, NSString *role, NSString **stringOut,
                          id *dependencyOut, int32_t *detail) {
    LLCF *value = nil;
    int result = attribute(candidate, kAXValueAttribute, &value, detail);
    if (controlsAbsent(result, *detail)) {
        *stringOut = nil;
        *dependencyOut = [NSNull null];
        return LL_AX_OK;
    }
    if (result) return result;
    return controlsTypedValue(value->value, role, stringOut, dependencyOut, detail);
}
static int controlsDiagnostic(int result, int32_t detail, const char *stage, NSString *role) {
    const char *enabled = getenv("LESELANG_AX_DIAGNOSTICS");
    if (result && enabled && strcmp(enabled, "1") == 0) {
        // Opt-in transport diagnosis: no document contents, AXValue or handles.
        fprintf(stderr, "controls AX read failed: %s, role=%s, status=%d, detail=%d\n",
            stage, role ? role.UTF8String : "unknown", result, detail);
    }
    return result;
}

static int controlsMetadata(LLCF *candidate, NSDictionary **out, size_t *byteCount,
                             int32_t *detail) {
    NSString *role = nil, *identifier = nil, *title = nil, *description = nil;
    LLCF *storage = [LLCF new];
    CFArrayRef rawNames = NULL;
    int result = api(AXUIElementCopyAttributeNames((AXUIElementRef)candidate->value, &rawNames), detail);
    storage->value = rawNames;
    if (result) return controlsDiagnostic(result, *detail, "AX attribute names", role);
    NSArray<NSString *> *attributes = nil;
    result = controlsAttributeNames(rawNames, &attributes, byteCount);
    if (result) return controlsDiagnostic(result, *detail, "AX attribute name types/bounds", role);
    result = stringAttribute(candidate, kAXRoleAttribute, &role, detail);
    if (result) return controlsDiagnostic(result, *detail, "AXRole", role);
    if (!role.length) return LL_AX_INVALID;
    // AXAttributeNames is descriptive, not exhaustive in every application.
    // Query the declared optional fields directly; only Unsupported/NoValue
    // establish absence. A generic AX failure still invalidates the snapshot.
    result = optionalString(candidate, kAXIdentifierAttribute, &identifier, detail);
    if (result) return controlsDiagnostic(result, *detail, "AXIdentifier", role);
    result = optionalString(candidate, kAXTitleAttribute, &title, detail);
    if (result) return controlsDiagnostic(result, *detail, "AXTitle", role);
    // Fixed role policy: buttons and readonly static text expose accessible
    // descriptions. Text editors/containers use title only; their description
    // is outside this profile (some apps advertise it but return AX failure).
    description = @"";
    if (controlsDescribedRole(role)) {
        result = optionalString(candidate, kAXDescriptionAttribute, &description, detail);
        if (result) return controlsDiagnostic(result, *detail, "AXDescription", role);
    }
    NSString *value = nil;
    id valueDependency = nil;
    result = controlsValue(candidate, role, &value, &valueDependency, detail);
    if (result) return controlsDiagnostic(result, *detail, "AXValue", role);
    LLCF *enabledValue = nil;
    BOOL enabled = NO;
    BOOL enabledPresent = NO;
    result = attribute(candidate, kAXEnabledAttribute, &enabledValue, detail);
    if (!controlsAbsent(result, *detail)) {
        if (result) return controlsDiagnostic(result, *detail, "AXEnabled", role);
        if (CFGetTypeID(enabledValue->value) != CFBooleanGetTypeID()) { *detail = -104; return LL_AX_INVALID; }
        enabledPresent = YES;
        enabled = CFBooleanGetValue((CFBooleanRef)enabledValue->value);
    }
    BOOL pressable = NO;
    if ([role isEqualToString:(__bridge NSString *)kAXButtonRole]) {
        LLCF *actions = [LLCF new];
        CFArrayRef names = NULL;
        result = api(AXUIElementCopyActionNames((AXUIElementRef)candidate->value, &names), detail);
        actions->value = names;
        if (!controlsAbsent(result, *detail)) {
            if (result) return controlsDiagnostic(result, *detail, "AXActionNames", role);
            result = pressActionNames(names, &pressable);
            if (result) return controlsDiagnostic(result, *detail, "AXActionNames types", role);
        }
    }
    Boolean editable = false;
    if (controlsTextRole(role)) {
        result = api(AXUIElementIsAttributeSettable((AXUIElementRef)candidate->value, kAXValueAttribute, &editable), detail);
        if (controlsAbsent(result, *detail)) editable = false;
        else if (result) return controlsDiagnostic(result, *detail, "AXValue settable", role);
    }
    // TextEdit omits AXEnabled on a writable AXTextArea. Only text controls
    // may infer enabled from a successful settable query; explicit false wins.
    enabled = controlsEffectiveEnabled(role, enabledPresent, enabled, editable);
    // Count all observed strings, including unexported container dependencies.
    // The bound precedes retaining a complete tree, not just the final copy.
    for (NSString *text in @[role, identifier, title, description, value ?: @""]) {
        size_t length = [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (length > LL_CONTROLS_BYTES_LIMIT - *byteCount) return LL_AX_BOUNDS;
        *byteCount += length;
    }
    *out = @{ @"role": role, @"identifier": identifier, @"label": title.length ? title : description,
              @"title": title, @"description": description,
              @"value": value ?: (id)[NSNull null], @"typedValue": valueDependency, @"attributes": attributes,
              @"enabled": @(enabled), @"enabledPresent": @(enabledPresent),
              @"editable": @(editable), @"pressable": @(pressable) };
    return LL_AX_OK;
}

static LLControlEntry *controlsLiveEntry(NSArray<LLControlEntry *> *registry, LLCF *item) {
    for (LLControlEntry *entry in registry) {
        if (entry.alive && sameElement(entry.element, item)) return entry;
    }
    return nil;
}
static int controlsEpoch(uint64_t previous, BOOL changed, uint64_t *out) {
    if (!previous) { *out = 1; return LL_AX_OK; }
    if (changed && previous == UINT64_MAX) return LL_AX_BOUNDS;
    *out = previous + (changed ? 1 : 0);
    return LL_AX_OK;
}
static BOOL controlsSameTree(NSArray<LLCF *> *left, NSArray<NSNumber *> *leftParents,
                             NSArray<LLCF *> *right, NSArray<NSNumber *> *rightParents) {
    if (!left || left.count != right.count || ![leftParents isEqualToArray:rightParents]) return NO;
    for (NSUInteger index = 0; index < left.count; ++index) {
        if (!sameElement(left[index], right[index])) return NO;
    }
    return YES;
}

// Stage a complete state before committing any membership or tombstones.
// Retired entries retain their CF object and consume lifetime capacity; an old
// language target can therefore never bind to an object replacing its label.
static int controlsCommitBounded(LLControlsContext *context, NSArray<LLCF *> *items,
                           NSArray<NSDictionary *> *metadata, NSArray<NSDictionary *> *dependencies,
                           NSArray<LLCF *> *tree, NSArray<NSNumber *> *parents, size_t observedBytes, size_t treeLimit) {
    if (items.count != metadata.count) return LL_AX_INVALID;
    if (tree.count != parents.count || !tree.count) return LL_AX_INVALID;
    if (items.count > LL_CONTROLS_LIMIT || context.registry.count > LL_CONTROLS_REGISTRY_LIMIT ||
        tree.count > treeLimit || observedBytes > LL_CONTROLS_BYTES_LIMIT) return LL_AX_BOUNDS;
    NSMutableArray<LLControlEntry *> *registry = [context.registry mutableCopy] ?: [NSMutableArray new];
    NSMutableArray<LLControlEntry *> *live = [NSMutableArray new];
    NSMutableArray<NSDictionary *> *rows = [NSMutableArray new];
    size_t keyBytes = 0;
    for (NSUInteger index = 0; index < items.count; ++index) {
        for (NSUInteger previous = 0; previous < index; ++previous) {
            if (sameElement(items[previous], items[index])) return LL_AX_INVALID;
        }
        LLControlEntry *entry = controlsLiveEntry(context.registry, items[index]);
        if (!entry) {
            if (registry.count >= LL_CONTROLS_REGISTRY_LIMIT || nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
            entry = [LLControlEntry new];
            entry.element = items[index];
            entry.identity = nextIdentity++;
            entry.key = [NSString stringWithFormat:@"%@/%llu", context.nonce, (unsigned long long)entry.identity];
            [registry addObject:entry];
        }
        if ([live containsObject:entry]) return LL_AX_INVALID;
        [live addObject:entry];
        size_t length = [entry.key lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (!length || length > LL_TEXT_LIMIT - keyBytes || length > LL_CONTROLS_BYTES_LIMIT - observedBytes) return LL_AX_BOUNDS;
        keyBytes += length;
        observedBytes += length;
        NSMutableDictionary *row = [metadata[index] mutableCopy];
        row[@"identity"] = @(entry.identity);
        row[@"key"] = entry.key;
        [rows addObject:row];
    }
    [rows sortUsingComparator:^NSComparisonResult(NSDictionary *left, NSDictionary *right) {
        return [left[@"identity"] compare:right[@"identity"]];
    }];
    uint64_t epoch = 0;
    int result = controlsEpoch(context.revision,
        ![context.rows isEqualToArray:rows] || ![context.dependencies isEqualToArray:dependencies] ||
        !controlsSameTree(context.tree, context.parents, tree, parents), &epoch);
    if (result) return result;
    for (LLControlEntry *entry in registry) entry.alive = [live containsObject:entry];
    context.registry = registry;
    context.rows = rows;
    context.dependencies = dependencies;
    context.tree = tree;
    context.parents = parents;
    context.revision = epoch;
    return LL_AX_OK;
}

static int controlsCommit(LLControlsContext *context, NSArray<LLCF *> *items,
                           NSArray<NSDictionary *> *metadata, NSArray<NSDictionary *> *dependencies,
                           NSArray<LLCF *> *tree, NSArray<NSNumber *> *parents, size_t observedBytes) {
    return controlsCommitBounded(context, items, metadata, dependencies, tree, parents, observedBytes, LL_NODE_LIMIT);
}

static int controlsRefreshInner(LLControlsContext *context, int32_t *detail) {
    int result = controlsPermission(context, detail);
    if (result) return result;
    LLCF *window = nil;
    result = controlsWindow(context, &window, detail);
    if (result) return result;
    NSMutableArray<LLCF *> *queue = [NSMutableArray arrayWithObject:window];
    NSMutableArray<NSNumber *> *depths = [NSMutableArray arrayWithObject:@0];
    NSMutableArray<NSNumber *> *parents = [NSMutableArray arrayWithObject:@0];
    NSMutableArray<LLCF *> *admitted = [NSMutableArray new];
    NSMutableArray<NSDictionary *> *metadata = [NSMutableArray new];
    NSMutableArray<NSDictionary *> *dependencies = [NSMutableArray new];
    size_t observedBytes = 0;
    for (NSUInteger index = 0; index < queue.count; ++index) {
        LLCF *candidate = queue[index];
        NSUInteger depth = depths[index].unsignedIntegerValue;
        NSDictionary *state = nil;
        result = controlsMetadata(candidate, &state, &observedBytes, detail);
        if (result) return result;
        // Pinned membership preserves identity across title changes, but never
        // admits a root that has ceased to be a window in the selected scope.
        if (index == 0 && context.kind == LL_CONTROLS_PINNED_WINDOW &&
            ![state[@"role"] isEqualToString:(__bridge NSString *)kAXWindowRole]) return LL_AX_STALE;
        if (index == 0 && context.kind == LL_CONTROLS_PINNED_WINDOW) {
            LLCF *value = nil;
            NSString *document = nil;
            result = attribute(candidate, kAXDocumentAttribute, &value, detail);
            if (!controlsAbsent(result, *detail)) {
                if (result) return controlsDiagnostic(result, *detail, "AXDocument", state[@"role"]);
                result = scopeDocumentValue(value->value, &document);
                if (result) return result;
            }
            size_t bytes = [document lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
            if (bytes > LL_CONTROLS_BYTES_LIMIT - observedBytes) return LL_AX_BOUNDS;
            observedBytes += bytes;
            NSMutableDictionary *windowState = [state mutableCopy];
            windowState[@"document"] = document ?: (id)[NSNull null];
            state = windowState;
        }
        if (controlsAdmittedRole(state[@"role"])) {
            if (admitted.count >= LL_CONTROLS_LIMIT) return LL_AX_BOUNDS;
            [admitted addObject:candidate];
            [metadata addObject:state];
        } else {
            // Container role/id/title/description/value state can carry semantic
            // dependencies (for example Calculator mode and display history).
            [dependencies addObject:state];
        }
        NSMutableArray<LLCF *> *descendants = nil;
        result = children(candidate, kAXChildrenAttribute, context.pid, &descendants, detail);
        if (result) return controlsDiagnostic(result, *detail, "AXChildren", state[@"role"]);
        if ((depth >= LL_DEPTH_LIMIT && descendants.count) || queue.count + descendants.count > LL_NODE_LIMIT) return LL_AX_BOUNDS;
        for (LLCF *child in descendants) {
            for (LLCF *seen in queue) if (sameElement(seen, child)) return LL_AX_INVALID;
            [queue addObject:child];
            [depths addObject:@(depth + 1)];
            [parents addObject:@(index)];
        }
    }
    result = controlsPermission(context, detail);
    if (result) return result;
    // This last scope lookup catches window replacement during observation.
    LLCF *confirmed = nil;
    result = controlsWindow(context, &confirmed, detail);
    if (result) return result;
    if (!sameElement(window, confirmed)) return LL_AX_STALE;
    result = controlsCommit(context, admitted, metadata, dependencies, queue, parents, observedBytes);
    if (result) return result;
    context.window = window;
    return LL_AX_OK;
}
static int controlsRefresh(LLControlsContext *context, int32_t *detail) {
    int result = controlsRefreshInner(context, detail);
    // A gap in a complete observation may conceal removal/replacement. Once a
    // connected scope cannot be read, refuse reconnecting its old identities.
    if (result && context && result != LL_AX_WRONG_THREAD) context.retired = YES;
    return result;
}

static int controlsPublish(LLControlsContext *context, LLControlRow *rows, size_t rowCapacity,
                            uint8_t *bytes, size_t byteCapacity, LLControlsSnapshot *out) {
    if (!context || !rows || !bytes || !out || !context.identity || !context.revision) return LL_AX_INVALID;
    if (context.rows.count > LL_CONTROLS_LIMIT || context.rows.count > rowCapacity) return LL_AX_BOUNDS;
    LLControlRow stagedRows[LL_CONTROLS_LIMIT] = {0};
    NSMutableData *stagedBytes = [NSMutableData new];
    for (NSUInteger index = 0; index < context.rows.count; ++index) {
        NSDictionary *row = context.rows[index];
        LLControlRow *raw = &stagedRows[index];
        raw->identity = [row[@"identity"] unsignedLongLongValue];
        BOOL present = row[@"value"] != [NSNull null];
        NSArray<NSString *> *strings = @[row[@"key"], row[@"role"], row[@"identifier"], row[@"label"], present ? row[@"value"] : @""];
        size_t lengths[5] = {0};
        for (NSUInteger part = 0; part < strings.count; ++part) {
            NSData *data = nil;
            int result = utf8(strings[part], &data);
            if (result) return result;
            if (data.length > LL_CONTROLS_BYTES_LIMIT - stagedBytes.length) return LL_AX_BOUNDS;
            lengths[part] = data.length;
            [stagedBytes appendData:data];
        }
        raw->key_length = lengths[0]; raw->role_length = lengths[1];
        raw->identifier_length = lengths[2]; raw->label_length = lengths[3]; raw->value_length = lengths[4];
        raw->value_present = present ? 1 : 0;
        raw->enabled = [row[@"enabled"] boolValue] ? 1 : 0;
        raw->editable = [row[@"editable"] boolValue] ? 1 : 0;
        raw->pressable = [row[@"pressable"] boolValue] ? 1 : 0;
    }
    if (stagedBytes.length > byteCapacity) return LL_AX_BOUNDS;
    size_t count = context.rows.count, length = stagedBytes.length;
    const void *source = stagedBytes.bytes;
    LLControlsSnapshot snapshot = {context.identity, context.revision, count, length};
    // No operations that allocate or call out can occur after publication starts.
    memcpy(rows, stagedRows, count * sizeof(LLControlRow));
    if (length) memcpy(bytes, source, length);
    *out = snapshot;
    return LL_AX_OK;
}

static int controlsExpected(LLControlsContext *context, uint64_t scope, uint64_t revision,
                             uint64_t target, BOOL press, LLControlEntry **out) {
    if (!scope || scope != context.identity || !revision || revision != context.revision || !target) return LL_AX_STALE;
    LLControlEntry *selected = nil;
    for (LLControlEntry *entry in context.registry) if (entry.alive && entry.identity == target) selected = entry;
    if (!selected) return LL_AX_STALE;
    NSDictionary *row = nil;
    for (NSDictionary *candidate in context.rows) if ([candidate[@"identity"] unsignedLongLongValue] == target) row = candidate;
    if (!row) return LL_AX_STALE;
    if (![row[@"enabled"] boolValue]) return LL_AX_NOT_EDITABLE;
    if (press) {
        if (![row[@"role"] isEqualToString:(__bridge NSString *)kAXButtonRole] || ![row[@"pressable"] boolValue]) return LL_AX_NOT_EDITABLE;
    } else if (!controlsTextRole(row[@"role"]) || ![row[@"editable"] boolValue]) return LL_AX_NOT_EDITABLE;
    *out = selected;
    return LL_AX_OK;
}

int32_t ll_controls_connect(uint32_t kind, const uint8_t *path, size_t length,
                            void **out, int32_t *detail) {
    @autoreleasepool { @try {
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        if (!out || !detail) return LL_AX_INVALID;
        *out = NULL; *detail = 0;
        if (kind != LL_CONTROLS_DOCUMENT && kind != LL_CONTROLS_CALCULATOR) return LL_AX_INVALID;
        if (!AXIsProcessTrusted()) return LL_AX_PERMISSION;
        LLControlsContext *context = [LLControlsContext new];
        context.kind = kind;
        context.nonce = [NSUUID UUID].UUIDString;
        if (kind == LL_CONTROLS_DOCUMENT) {
            NSString *file = nil;
            int result = decode(path, length, &file);
            if (result) return result;
            if (!file.isAbsolutePath || containsNul(file)) return LL_AX_INVALID;
            context.document = [NSURL fileURLWithPath:file].URLByResolvingSymlinksInPath.URLByStandardizingPath.absoluteString;
            result = utf8(context.document, NULL);
            if (result) return result;
        } else if (length) return LL_AX_INVALID;
        int result = kind == LL_CONTROLS_DOCUMENT ? discoverProcess(context, detail) :
            discoverApp(context, @"/System/Applications/Calculator.app", @"com.apple.calculator", detail);
        if (result) return result;
        context.application = [LLCF new];
        context.application->value = AXUIElementCreateApplication(context.pid);
        if (nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
        context.identity = nextIdentity++;
        result = controlsRefresh(context, detail);
        if (result) return result;
        *out = (__bridge_retained void *)context;
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_controls_permission(void *raw, int32_t *detail) {
    @autoreleasepool { @try {
        if (!detail) return LL_AX_INVALID;
        *detail = 0;
        LLControlsContext *context = (__bridge LLControlsContext *)raw;
        int result = controlsPermission(context, detail);
        if (result && context && result != LL_AX_WRONG_THREAD) context.retired = YES;
        return result;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLControlsContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
int32_t ll_controls_snapshot(void *raw, LLControlRow *rows, size_t rowCapacity,
                             uint8_t *bytes, size_t byteCapacity,
                             LLControlsSnapshot *out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !rows || !bytes || !out || !detail) return LL_AX_INVALID;
        *detail = 0;
        LLControlsContext *context = (__bridge LLControlsContext *)raw;
        int result = controlsRefresh(context, detail);
        if (result) return result;
        return controlsPublish(context, rows, rowCapacity, bytes, byteCapacity, out);
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLControlsContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
static int controlsMutate(void *raw, uint64_t scope, uint64_t revision, uint64_t target,
                           BOOL press, const uint8_t *text, size_t length, int32_t *detail) {
    BOOL entered = NO;
    @autoreleasepool { @try {
        if (!raw || !detail) return LL_AX_INVALID;
        *detail = 0;
        NSString *replacement = nil;
        if (!press) {
            int result = decode(text, length, &replacement);
            if (result) return result;
        }
        LLControlsContext *context = (__bridge LLControlsContext *)raw;
        int result = controlsRefresh(context, detail);
        if (result) return result;
        LLControlEntry *selected = nil;
        result = controlsExpected(context, scope, revision, target, press, &selected);
        if (result) return result;
        // This is the final scope/capability comparison before native mutation.
        // AX has no atomic read/compare/write or unseen ABA detection primitive.
        entered = YES;
        AXError error = press ? AXUIElementPerformAction((AXUIElementRef)selected.element->value, kAXPressAction) :
            AXUIElementSetAttributeValue((AXUIElementRef)selected.element->value, kAXValueAttribute, (__bridge CFStringRef)replacement);
        if (error != kAXErrorSuccess) {
            context.retired = YES;
            *detail = (int32_t)error;
            return LL_AX_UNCERTAIN;
        }
        return LL_AX_OK;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLControlsContext *)raw).retired = YES;
        return entered ? LL_AX_UNCERTAIN : LL_AX_API;
    } }
}
int32_t ll_controls_press(void *raw, uint64_t scope, uint64_t revision,
                          uint64_t target, int32_t *detail) {
    return controlsMutate(raw, scope, revision, target, YES, NULL, 0, detail);
}
int32_t ll_controls_set_text(void *raw, uint64_t scope, uint64_t revision, uint64_t target,
                             const uint8_t *text, size_t length, int32_t *detail) {
    return controlsMutate(raw, scope, revision, target, NO, text, length, detail);
}
