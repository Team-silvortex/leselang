// Explicit application discovery and one-time window selection. Included after
// controls.inc; all ownership, process checks and control execution are shared.
#include "scope.h"

@interface LLWindowEntry : NSObject
@property(nonatomic, strong) LLCF *element;
@property(nonatomic) uint64_t identity;
@property(nonatomic, copy) NSString *key;
@property(nonatomic) BOOL alive;
@end
@implementation LLWindowEntry
@end
@interface LLApplicationContext : LLAXContext
@property(nonatomic, copy) NSString *nonce;
@property(nonatomic, copy) NSArray<LLWindowEntry *> *windowRegistry;
@property(nonatomic, copy) NSArray<NSDictionary *> *rows;
@property(nonatomic) BOOL retired;
@end
@implementation LLApplicationContext
@end

static BOOL scopeProcessCandidate(pid_t pid, pid_t selectedPID, uid_t uid,
                                   NSString *actual, NSString *expected) {
    return pid > 0 && (!selectedPID || selectedPID == pid) && uid == geteuid() &&
        actual && expected && [actual isEqualToString:expected];
}
static int scopeApplicationExecutable(NSString *path, NSString **out) {
    if (!path.isAbsolutePath || containsNul(path) ||
        ![path.pathExtension.lowercaseString isEqualToString:@"app"]) return LL_AX_INVALID;
    NSString *canonical = path.stringByResolvingSymlinksInPath.stringByStandardizingPath;
    BOOL directory = NO;
    if (![[NSFileManager defaultManager] fileExistsAtPath:canonical isDirectory:&directory] || !directory) return LL_AX_UNAVAILABLE;
    NSBundle *bundle = [NSBundle bundleWithPath:canonical];
    NSString *identifier = bundle.bundleIdentifier;
    if (!bundle || !identifier.length || utf8(identifier, NULL)) return LL_AX_INVALID;
    NSString *executable = bundle.executableURL.path.stringByResolvingSymlinksInPath.stringByStandardizingPath;
    if (!executable.length || utf8(executable, NULL) ||
        ![executable hasPrefix:[canonical stringByAppendingString:@"/"]] ||
        ![[NSFileManager defaultManager] isExecutableFileAtPath:executable]) return LL_AX_INVALID;
    *out = executable;
    return LL_AX_OK;
}
static int scopeDiscoverApplication(LLApplicationContext *context, NSString *path,
                                     pid_t selectedPID, int32_t *detail) {
    NSString *expected = nil;
    int result = scopeApplicationExecutable(path, &expected);
    if (result) return result;
    enum { processLimit = 4096 };
    pid_t pids[processLimit + 1] = {0};
    int size = proc_listpids(PROC_UID_ONLY, (uint32_t)geteuid(), pids, sizeof(pids));
    if (size < 0 || size % sizeof(pid_t)) { *detail = errno; return LL_AX_API; }
    if (size >= (int)sizeof(pids)) return LL_AX_BOUNDS;
    NSUInteger matches = 0;
    for (size_t index = 0; index < (size_t)size / sizeof(pid_t); ++index) {
        pid_t pid = pids[index];
        if (pid <= 0 || (selectedPID && selectedPID != pid)) continue;
        char buffer[PROC_PIDPATHINFO_MAXSIZE] = {0};
        int length = proc_pidpath(pid, buffer, sizeof(buffer));
        if (length <= 0 || length >= (int)sizeof(buffer)) continue;
        NSString *candidate = [[NSString alloc] initWithBytes:buffer length:strlen(buffer) encoding:NSUTF8StringEncoding];
        if (!candidate || ![candidate.stringByResolvingSymlinksInPath.stringByStandardizingPath isEqualToString:expected]) continue;
        struct proc_bsdinfo info = {0};
        NSString *actual = nil;
        result = processIdentity(pid, &info, &actual);
        if (result) return result;
        if (!scopeProcessCandidate(pid, selectedPID, info.pbi_uid, actual, expected)) return LL_AX_STALE;
        if (++matches > 1) return LL_AX_INVALID;
        context.pid = pid;
        context.executable = actual;
        context.startSeconds = info.pbi_start_tvsec;
        context.startMicros = info.pbi_start_tvusec;
    }
    return matches == 1 ? LL_AX_OK : LL_AX_UNAVAILABLE;
}
static int scopePermission(LLApplicationContext *context, int32_t *detail) {
    if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
    if (!context) return LL_AX_INVALID;
    if (context.retired) return LL_AX_STALE;
    return permission(context, detail);
}
static int scopeDocumentValue(CFTypeRef value, NSString **out) {
    NSString *text = nil;
    if (!value) return LL_AX_INVALID;
    if (CFGetTypeID(value) == CFStringGetTypeID()) text = (__bridge NSString *)value;
    else if (CFGetTypeID(value) == CFURLGetTypeID()) text = [(__bridge NSURL *)value absoluteString];
    else return LL_AX_INVALID;
    int result = utf8(text, NULL);
    if (result) return result;
    if (!text.length) { *out = nil; return LL_AX_OK; }
    if (containsNul(text) || [text rangeOfString:@"%00" options:NSCaseInsensitiveSearch].location != NSNotFound) return LL_AX_INVALID;
    NSURL *url = [NSURL URLWithString:text];
    if (!url || !url.scheme.length) return LL_AX_INVALID;
    if (url.isFileURL) return canonicalFileURL(text, out);
    // Non-file document URLs are metadata only. Preserve the exact spelling;
    // they are neither fetched nor admitted by the local-file selector.
    *out = [text copy];
    return LL_AX_OK;
}
static int scopeWindowRole(NSString *role, BOOL *admitted) {
    if (!role.length || containsNul(role)) return LL_AX_INVALID;
    *admitted = [role isEqualToString:(__bridge NSString *)kAXWindowRole];
    return LL_AX_OK;
}
static int scopeWindowMetadata(LLCF *window, NSDictionary **out, size_t *bytes, int32_t *detail) {
    NSString *role = nil, *title = nil, *identifier = nil, *document = nil;
    int result = stringAttribute(window, kAXRoleAttribute, &role, detail);
    if (result) return controlsDiagnostic(result, *detail, "window AXRole", role);
    if (![role isEqualToString:(__bridge NSString *)kAXWindowRole]) return controlsDiagnostic(LL_AX_INVALID, 0, "window role admission", role);
    result = optionalString(window, kAXTitleAttribute, &title, detail);
    if (result) return controlsDiagnostic(result, *detail, "window AXTitle", role);
    result = optionalString(window, kAXIdentifierAttribute, &identifier, detail);
    if (result) return controlsDiagnostic(result, *detail, "window AXIdentifier", role);
    if (containsNul(title) || containsNul(identifier)) return LL_AX_INVALID;
    LLCF *value = nil;
    result = attribute(window, kAXDocumentAttribute, &value, detail);
    if (!controlsAbsent(result, *detail)) {
        if (result) return controlsDiagnostic(result, *detail, "window AXDocument", role);
        result = scopeDocumentValue(value->value, &document);
        if (result) return controlsDiagnostic(result, 0, "window AXDocument value", role);
    }
    for (NSString *text in @[title, identifier, document ?: @""]) {
        size_t length = [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (*bytes > LL_WINDOWS_BYTES_LIMIT || length > LL_WINDOWS_BYTES_LIMIT - *bytes) return LL_AX_BOUNDS;
        *bytes += length;
    }
    *out = @{ @"title": title, @"identifier": identifier, @"document": document ?: (id)[NSNull null] };
    return LL_AX_OK;
}
static LLWindowEntry *scopeLiveWindow(NSArray<LLWindowEntry *> *registry, LLCF *element) {
    for (LLWindowEntry *entry in registry) if (entry.alive && sameElement(entry.element, element)) return entry;
    return nil;
}
static int scopeCommitWindows(LLApplicationContext *context, NSArray<LLCF *> *items,
                               NSArray<NSDictionary *> *metadata, size_t observedBytes) {
    if (!context || items.count != metadata.count) return LL_AX_INVALID;
    if (items.count > LL_WINDOWS_LIMIT || context.windowRegistry.count > LL_WINDOWS_REGISTRY_LIMIT || observedBytes > LL_WINDOWS_BYTES_LIMIT) return LL_AX_BOUNDS;
    NSMutableArray<LLWindowEntry *> *registry = [context.windowRegistry mutableCopy] ?: [NSMutableArray new];
    NSMutableArray<LLWindowEntry *> *live = [NSMutableArray new];
    NSMutableArray<NSDictionary *> *rows = [NSMutableArray new];
    for (NSUInteger index = 0; index < items.count; ++index) {
        for (NSUInteger prior = 0; prior < index; ++prior) if (sameElement(items[prior], items[index])) return LL_AX_INVALID;
        LLWindowEntry *entry = scopeLiveWindow(context.windowRegistry, items[index]);
        if (!entry) {
            if (registry.count >= LL_WINDOWS_REGISTRY_LIMIT || nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
            entry = [LLWindowEntry new];
            entry.element = items[index];
            entry.identity = nextIdentity++;
            entry.key = [NSString stringWithFormat:@"%@/%llu", context.nonce, (unsigned long long)entry.identity];
            [registry addObject:entry];
        }
        size_t length = [entry.key lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (!length || length > LL_TEXT_LIMIT || length > LL_WINDOWS_BYTES_LIMIT - observedBytes) return LL_AX_BOUNDS;
        observedBytes += length;
        [live addObject:entry];
        NSMutableDictionary *row = [metadata[index] mutableCopy];
        row[@"identity"] = @(entry.identity);
        row[@"key"] = entry.key;
        [rows addObject:row];
    }
    [rows sortUsingComparator:^NSComparisonResult(NSDictionary *left, NSDictionary *right) {
        return [left[@"identity"] compare:right[@"identity"]];
    }];
    for (LLWindowEntry *entry in registry) entry.alive = [live containsObject:entry];
    context.windowRegistry = registry;
    context.rows = rows;
    return LL_AX_OK;
}
static int scopeRefreshInner(LLApplicationContext *context, int32_t *detail) {
    int result = scopePermission(context, detail);
    if (result) return result;
    NSMutableArray<LLCF *> *windows = nil;
    result = children(context.application, kAXWindowsAttribute, context.pid, &windows, detail);
    if (result) return result;
    NSMutableArray<LLCF *> *candidates = [NSMutableArray new];
    NSMutableArray<NSDictionary *> *metadata = [NSMutableArray new];
    size_t bytes = 0;
    for (LLCF *window in windows) {
        // Finder also exposes its desktop AXScrollArea in AXWindows. Every
        // element still passed ownership checks; only AXWindow is selectable.
        NSString *role = nil;
        result = stringAttribute(window, kAXRoleAttribute, &role, detail);
        if (result) return controlsDiagnostic(result, *detail, "window candidate AXRole", nil);
        BOOL admitted = NO;
        result = scopeWindowRole(role, &admitted); if (result) return result;
        if (!admitted) continue;
        NSDictionary *row = nil;
        result = scopeWindowMetadata(window, &row, &bytes, detail);
        if (result) return result;
        [candidates addObject:window];
        [metadata addObject:row];
    }
    result = scopePermission(context, detail);
    if (result) return result;
    return scopeCommitWindows(context, candidates, metadata, bytes);
}
static int scopeRefresh(LLApplicationContext *context, int32_t *detail) {
    int result = scopeRefreshInner(context, detail);
    if (result && context && result != LL_AX_WRONG_THREAD) context.retired = YES;
    return result;
}
static int scopeSelect(LLApplicationContext *context, NSString *key, NSString *title,
                         NSString *identifier, NSString *document, LLWindowEntry **out) {
    if (!context || !out || (key.length && (title.length || identifier.length || document.length)) ||
        (!key.length && !title.length && !identifier.length && !document.length)) return LL_AX_INVALID;
    NSDictionary *selected = nil;
    for (NSDictionary *row in context.rows) {
        BOOL matches = key.length ? [row[@"key"] isEqualToString:key] :
            (!title.length || [row[@"title"] isEqualToString:title]) &&
            (!identifier.length || [row[@"identifier"] isEqualToString:identifier]) &&
            (!document.length || [row[@"document"] isEqual:document]);
        if (!matches) continue;
        if (selected) return LL_AX_INVALID;
        selected = row;
    }
    if (!selected) return LL_AX_UNAVAILABLE;
    uint64_t identity = [selected[@"identity"] unsignedLongLongValue];
    for (LLWindowEntry *entry in context.windowRegistry) {
        if (entry.alive && entry.identity == identity) { *out = entry; return LL_AX_OK; }
    }
    return LL_AX_STALE;
}
static int scopePublishWindows(LLApplicationContext *context, LLWindowRow *rows, size_t rowCapacity,
                                uint8_t *bytes, size_t byteCapacity, LLWindowInventory *out) {
    if (!context || !rows || !bytes || !out) return LL_AX_INVALID;
    if (context.rows.count > LL_WINDOWS_LIMIT || context.rows.count > rowCapacity) return LL_AX_BOUNDS;
    LLWindowRow staged[LL_WINDOWS_LIMIT] = {0};
    NSMutableData *data = [NSMutableData new];
    for (NSUInteger index = 0; index < context.rows.count; ++index) {
        NSDictionary *row = context.rows[index];
        BOOL present = row[@"document"] != [NSNull null];
        NSArray<NSString *> *strings = @[row[@"key"], row[@"title"], row[@"identifier"], present ? row[@"document"] : @""];
        size_t lengths[4] = {0};
        for (NSUInteger part = 0; part < strings.count; ++part) {
            NSData *text = nil;
            int result = utf8(strings[part], &text);
            if (result) return result;
            if (text.length > LL_WINDOWS_BYTES_LIMIT - data.length) return LL_AX_BOUNDS;
            lengths[part] = text.length;
            [data appendData:text];
        }
        staged[index] = (LLWindowRow){[row[@"identity"] unsignedLongLongValue], lengths[0], lengths[1], lengths[2], lengths[3], present ? 1 : 0};
    }
    size_t count = context.rows.count;
    size_t length = data.length;
    const void *source = data.bytes;
    if (length > byteCapacity) return LL_AX_BOUNDS;
    LLWindowInventory inventory = {count, length};
    // No Objective-C messages, allocations or AX calls after publication starts.
    memcpy(rows, staged, count * sizeof(LLWindowRow));
    if (length) memcpy(bytes, source, length);
    *out = inventory;
    return LL_AX_OK;
}
int32_t ll_application_connect(const uint8_t *bundle, size_t length, int32_t selectedPID,
                                void **out, int32_t *actualPID, int32_t *detail) {
    @autoreleasepool { @try {
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        if (!out || !actualPID || !detail || selectedPID < 0) return LL_AX_INVALID;
        *out = NULL; *actualPID = 0; *detail = 0;
        if (!AXIsProcessTrusted()) return LL_AX_PERMISSION;
        NSString *path = nil;
        int result = decode(bundle, length, &path);
        if (result) return result;
        LLApplicationContext *context = [LLApplicationContext new];
        context.nonce = [NSUUID UUID].UUIDString;
        result = scopeDiscoverApplication(context, path, selectedPID, detail);
        if (result) return result;
        context.application = [LLCF new];
        context.application->value = AXUIElementCreateApplication(context.pid);
        result = scopePermission(context, detail);
        if (result) return result;
        *actualPID = context.pid;
        *out = (__bridge_retained void *)context;
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_application_windows(void *raw, LLWindowRow *rows, size_t rowCapacity,
                                uint8_t *bytes, size_t byteCapacity,
                                LLWindowInventory *out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !rows || !bytes || !out || !detail) return LL_AX_INVALID;
        *detail = 0;
        LLApplicationContext *context = (__bridge LLApplicationContext *)raw;
        int result = scopeRefresh(context, detail);
        if (result) return result;
        result = scopePublishWindows(context, rows, rowCapacity, bytes, byteCapacity, out);
        if (result) context.retired = YES;
        return result;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLApplicationContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
int32_t ll_application_controls(void *raw, const uint8_t *keyBytes, size_t keyLength,
    const uint8_t *titleBytes, size_t titleLength, const uint8_t *identifierBytes, size_t identifierLength,
    const uint8_t *documentBytes, size_t documentLength, void **out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !out || !detail) return LL_AX_INVALID;
        *out = NULL; *detail = 0;
        NSString *key = nil, *title = nil, *identifier = nil, *document = nil;
        int result = decode(keyBytes, keyLength, &key);
        if (!result) result = decode(titleBytes, titleLength, &title);
        if (!result) result = decode(identifierBytes, identifierLength, &identifier);
        if (!result) result = decode(documentBytes, documentLength, &document);
        if (result) return result;
        if (containsNul(key) || containsNul(title) || containsNul(identifier) || containsNul(document)) return LL_AX_INVALID;
        if (document.length) {
            if (!document.isAbsolutePath) return LL_AX_INVALID;
            document = [NSURL fileURLWithPath:document].URLByResolvingSymlinksInPath.URLByStandardizingPath.absoluteString;
            result = utf8(document, NULL); if (result) return result;
        }
        LLApplicationContext *application = (__bridge LLApplicationContext *)raw;
        result = scopeRefresh(application, detail);
        if (result) return result;
        LLWindowEntry *window = nil;
        result = scopeSelect(application, key, title, identifier, document, &window);
        if (result) return result;
        LLControlsContext *context = [LLControlsContext new];
        context.kind = LL_CONTROLS_PINNED_WINDOW;
        context.pid = application.pid;
        context.startSeconds = application.startSeconds;
        context.startMicros = application.startMicros;
        context.executable = application.executable;
        context.application = application.application;
        context.window = window.element;
        context.nonce = [NSUUID UUID].UUIDString;
        if (nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
        context.identity = nextIdentity++;
        result = controlsRefresh(context, detail);
        if (result) return result;
        *out = (__bridge_retained void *)context;
        return LL_AX_OK;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLApplicationContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
