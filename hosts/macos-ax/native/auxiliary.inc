// Explicit secondary process identities for the opt-in widgets profile only.
// Included after scope.inc. No secondary AXApplication or global window read.
#include "widgets.h"

@interface LLAuxiliaryProcess : NSObject
@property(nonatomic) pid_t pid;
@property(nonatomic) uid_t uid;
@property(nonatomic) uint64_t startSeconds;
@property(nonatomic) uint32_t startMicros;
@property(nonatomic, copy) NSString *executable;
@property(nonatomic) NSUInteger edgeCount;
@end
@implementation LLAuxiliaryProcess
@end
@interface LLAuxiliaryPolicy : NSObject
@property(nonatomic, copy) NSArray<LLAuxiliaryProcess *> *processes;
@property(nonatomic) LLAuxiliaryGrantCheck check;
@property(nonatomic) const void *grantContext;
@property(nonatomic) BOOL revoked;
@end
@implementation LLAuxiliaryPolicy
@end

static BOOL auxiliaryIdentityMatches(LLAuxiliaryProcess *expected, LLAuxiliaryProcess *current) {
    return expected && current && expected.pid > 0 && expected.pid == current.pid &&
        expected.uid == geteuid() && current.uid == expected.uid &&
        expected.startSeconds == current.startSeconds && expected.startMicros == current.startMicros &&
        [expected.executable isEqualToString:current.executable];
}
static int auxiliaryCanonicalPath(NSString *path) {
    if (!path.length || !path.isAbsolutePath || containsNul(path)) return LL_AX_INVALID;
    int result = utf8(path, NULL); if (result) return result;
    NSString *canonical = path.stringByResolvingSymlinksInPath.stringByStandardizingPath;
    return [path isEqualToString:canonical] ? LL_AX_OK : LL_AX_INVALID;
}
static int auxiliaryReadProcess(pid_t pid, LLAuxiliaryProcess **out) {
    if (pid <= 0) return LL_AX_INVALID;
    struct proc_bsdinfo before = {0}, after = {0};
    NSString *firstPath = nil, *lastPath = nil;
    int result = processIdentity(pid, &before, &firstPath); if (result) return result;
    result = processIdentity(pid, &after, &lastPath); if (result) return result;
    // Two consistent local process reads prevent combining an old start time
    // with an executable path obtained after PID reuse or an exec transition.
    if (before.pbi_uid != after.pbi_uid || before.pbi_uid != geteuid() ||
        before.pbi_start_tvsec != after.pbi_start_tvsec || before.pbi_start_tvusec != after.pbi_start_tvusec ||
        ![firstPath isEqualToString:lastPath]) return LL_AX_STALE;
    if (!before.pbi_start_tvsec || before.pbi_start_tvusec >= 1000000) return LL_AX_INVALID;
    result = auxiliaryCanonicalPath(firstPath); if (result) return result;
    LLAuxiliaryProcess *process = [LLAuxiliaryProcess new];
    process.pid = pid; process.uid = before.pbi_uid;
    process.startSeconds = before.pbi_start_tvsec; process.startMicros = before.pbi_start_tvusec;
    process.executable = firstPath;
    *out = process; return LL_AX_OK;
}
static int auxiliaryCheckProcesses(NSArray<LLAuxiliaryProcess *> *processes, int32_t *detail) {
    for (LLAuxiliaryProcess *expected in processes) {
        LLAuxiliaryProcess *current = nil;
        int result = auxiliaryReadProcess(expected.pid, &current);
        if (result || !auxiliaryIdentityMatches(expected, current)) {
            if (detail) *detail = 0;
            return result ?: LL_AX_STALE;
        }
    }
    return LL_AX_OK;
}
static int auxiliaryCheckGrant(LLAuxiliaryPolicy *policy) {
    if (!policy) return LL_AX_OK;
    if (policy.revoked) return LL_AX_PERMISSION;
    if (!policy.check || !policy.grantContext || policy.check(policy.grantContext) != 1) {
        policy.revoked = YES;
        return LL_AX_PERMISSION;
    }
    return LL_AX_OK;
}
static int auxiliaryCheck(LLAuxiliaryPolicy *policy, int32_t *detail) {
    int result = auxiliaryCheckGrant(policy);
    if (result) { if (detail) *detail = 0; return result; }
    return auxiliaryCheckProcesses(policy.processes, detail);
}
static int auxiliaryPolicy(pid_t primary, const LLAuxiliarySpec *specifications, size_t count,
    LLAuxiliaryGrantCheck check, const void *grantContext, LLAuxiliaryPolicy **out, int32_t *detail) {
    if (!specifications || !count || count > LL_AUXILIARY_LIMIT || !check || !grantContext) return LL_AX_INVALID;
    LLAuxiliaryPolicy *policy = [LLAuxiliaryPolicy new];
    policy.check = check; policy.grantContext = grantContext;
    int result = auxiliaryCheckGrant(policy); if (result) return result;
    NSMutableArray<LLAuxiliaryProcess *> *processes = [NSMutableArray new];
    for (size_t index = 0; index < count; ++index) {
        const LLAuxiliarySpec *spec = &specifications[index];
        if (spec->pid <= 0 || spec->pid == primary || !spec->start_seconds || spec->start_micros >= 1000000) return LL_AX_INVALID;
        for (LLAuxiliaryProcess *other in processes) if (other.pid == spec->pid) return LL_AX_INVALID;
        NSString *path = nil;
        result = decode(spec->executable, spec->executable_length, &path); if (result) return result;
        result = auxiliaryCanonicalPath(path); if (result) return result;
        LLAuxiliaryProcess *expected = [LLAuxiliaryProcess new];
        expected.pid = spec->pid; expected.uid = geteuid(); expected.startSeconds = spec->start_seconds;
        expected.startMicros = spec->start_micros; expected.executable = path;
        [processes addObject:expected];
    }
    policy.processes = processes;
    result = auxiliaryCheck(policy, detail); if (result) return result;
    *out = policy; return LL_AX_OK;
}
static BOOL auxiliaryAllowedPID(LLAuxiliaryPolicy *policy, pid_t primary, pid_t actual) {
    if (actual <= 0) return NO;
    if (actual == primary) return YES;
    for (LLAuxiliaryProcess *process in policy.processes) if (process.pid == actual) return YES;
    return NO;
}
static int auxiliaryElementPID(LLCF *item, pid_t *out, int32_t *detail) {
    if (!item || !item->value || CFGetTypeID(item->value) != AXUIElementGetTypeID()) return LL_AX_INVALID;
    pid_t actual = 0;
    int result = api(AXUIElementGetPid((AXUIElementRef)item->value, &actual), detail);
    if (result) return result;
    if (actual <= 0) return LL_AX_INVALID;
    *out = actual; return LL_AX_OK;
}
static int auxiliaryElement(LLAuxiliaryPolicy *policy, pid_t primary, LLCF *item,
    LLCF *parent, BOOL requireParent, int32_t *detail) {
    pid_t actual = 0;
    int result = auxiliaryElementPID(item, &actual, detail); if (result) return result;
    if (!auxiliaryAllowedPID(policy, primary, actual)) {
        if (detail) *detail = 0;
        return LL_AX_STALE;
    }
    result = element(item, actual, detail); if (result) return result;
    if (parent && requireParent) {
        pid_t parentPID = 0;
        result = auxiliaryElementPID(parent, &parentPID, detail); if (result) return result;
        if (parentPID != actual) {
            LLCF *observed = nil;
            result = attribute(item, kAXParentAttribute, &observed, detail); if (result) return result;
            if (CFGetTypeID(observed->value) != AXUIElementGetTypeID() || !sameElement(parent, observed)) {
                if (detail) *detail = 0;
                const char *diagnostics = getenv("LESELANG_AX_DIAGNOSTICS");
                if (diagnostics && strcmp(diagnostics, "1") == 0) {
                    fprintf(stderr, "AX cross-process parent refused: parent_pid=%d, child_pid=%d\n", parentPID, actual);
                }
                return LL_AX_STALE;
            }
        }
    }
    return LL_AX_OK;
}
static int auxiliarySelectWindow(LLApplicationContext *application,
    const uint8_t *keyBytes, size_t keyLength, const uint8_t *titleBytes, size_t titleLength,
    const uint8_t *identifierBytes, size_t identifierLength, const uint8_t *documentBytes, size_t documentLength,
    LLWindowEntry **out, int32_t *detail) {
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
    result = scopeRefresh(application, detail); if (result) return result;
    return scopeSelect(application, key, title, identifier, document, out);
}

typedef struct {
    pid_t primary;
    __unsafe_unretained NSMutableArray<LLAuxiliaryProcess *> *processes;
} LLAuxiliaryFrontier;
static int auxiliaryFrontierElement(LLCF *item, void *opaque, int32_t *detail) {
    LLAuxiliaryFrontier *frontier = opaque;
    pid_t actual = 0;
    int result = auxiliaryElementPID(item, &actual, detail); if (result) return result;
    if (actual == frontier->primary) return element(item, actual, detail);
    // A foreign reference is a leaf: only local process metadata is read.
    // No AX role, parent, children or other property is requested from it.
    LLAuxiliaryProcess *candidate = nil;
    result = auxiliaryReadProcess(actual, &candidate); if (result) return result;
    for (LLAuxiliaryProcess *previous in frontier->processes) {
        if (previous.pid != actual) continue;
        if (!auxiliaryIdentityMatches(previous, candidate)) return LL_AX_STALE;
        ++previous.edgeCount; return LL_AX_OK;
    }
    if (frontier->processes.count >= LL_AUXILIARY_LIMIT) return LL_AX_BOUNDS;
    candidate.edgeCount = 1; [frontier->processes addObject:candidate];
    return LL_AX_OK;
}
static int auxiliaryFrontier(LLApplicationContext *application, LLCF *window,
    NSArray<LLAuxiliaryProcess *> **out, int32_t *detail) {
    NSMutableArray<LLAuxiliaryProcess *> *processes = [NSMutableArray new];
    LLAuxiliaryFrontier frontier = {application.pid, processes};
    NSMutableArray<LLCF *> *tree = [NSMutableArray arrayWithObject:window];
    NSMutableArray<NSNumber *> *depths = [NSMutableArray arrayWithObject:@0];
    for (NSUInteger index = 0; index < tree.count; ++index) {
        pid_t owner = 0;
        int result = auxiliaryElementPID(tree[index], &owner, detail); if (result) return result;
        if (owner != application.pid) continue;
        NSString *role = nil;
        result = stringAttribute(tree[index], kAXRoleAttribute, &role, detail); if (result) return result;
        if (!role.length || containsNul(role)) return LL_AX_INVALID;
        if (!index && ![role isEqualToString:@"AXWindow"]) return LL_AX_STALE;
        NSMutableArray<LLCF *> *descendants = nil;
        result = childrenChecked(tree[index], kAXChildrenAttribute, auxiliaryFrontierElement, &frontier, &descendants, detail);
        if (result) return result;
        if ([role isEqualToString:@"AXPopUpButton"] || [role isEqualToString:@"AXMenuButton"]) {
            LLCF *shown = nil;
            result = attribute(tree[index], kAXShownMenuUIElementAttribute, &shown, detail);
            if (!controlsAbsent(result, *detail)) {
                if (result) return result;
                BOOL present = NO;
                for (LLCF *child in descendants) if (sameElement(child, shown)) present = YES;
                if (!present) {
                    result = auxiliaryFrontierElement(shown, &frontier, detail); if (result) return result;
                    [descendants addObject:shown];
                }
            }
        }
        NSUInteger depth = depths[index].unsignedIntegerValue;
        if ((depth >= LL_WIDGET_DEPTH_LIMIT && descendants.count) || tree.count + descendants.count > LL_WIDGET_NODE_LIMIT) return LL_AX_BOUNDS;
        for (LLCF *child in descendants) {
            for (LLCF *seen in tree) if (sameElement(child, seen)) return LL_AX_INVALID;
            [tree addObject:child]; [depths addObject:@(depth + 1)];
        }
    }
    int result = scopePermission(application, detail); if (result) return result;
    NSMutableArray<LLCF *> *windows = nil; LLCF *confirmed = nil;
    result = children(application.application, kAXWindowsAttribute, application.pid, &windows, detail); if (result) return result;
    result = controlsPinnedWindow(window, windows, &confirmed); if (result) return result;
    result = auxiliaryCheckProcesses(processes, detail); if (result) return result;
    [processes sortUsingComparator:^NSComparisonResult(LLAuxiliaryProcess *left, LLAuxiliaryProcess *right) {
        return [@(left.pid) compare:@(right.pid)];
    }];
    *out = processes; return LL_AX_OK;
}
static int auxiliaryPublish(NSArray<LLAuxiliaryProcess *> *processes,
    LLAuxiliaryRow *rows, size_t rowCapacity, uint8_t *bytes, size_t byteCapacity, LLAuxiliaryInventory *out) {
    if (!rows || !bytes || !out) return LL_AX_INVALID;
    if (processes.count > LL_AUXILIARY_LIMIT || processes.count > rowCapacity) return LL_AX_BOUNDS;
    LLAuxiliaryRow staged[LL_AUXILIARY_LIMIT] = {0};
    NSMutableData *data = [NSMutableData new];
    for (NSUInteger index = 0; index < processes.count; ++index) {
        LLAuxiliaryProcess *process = processes[index]; NSData *path = nil;
        int result = utf8(process.executable, &path); if (result) return result;
        if (path.length > LL_AUXILIARY_BYTES_LIMIT - data.length) return LL_AX_BOUNDS;
        [data appendData:path];
        staged[index] = (LLAuxiliaryRow){process.pid, process.startSeconds, process.startMicros, path.length, process.edgeCount};
    }
    size_t count = processes.count, length = data.length; const void *source = data.bytes;
    if (length > byteCapacity) return LL_AX_BOUNDS;
    LLAuxiliaryInventory inventory = {count, length};
    memcpy(rows, staged, count * sizeof(LLAuxiliaryRow));
    if (length) memcpy(bytes, source, length);
    *out = inventory; return LL_AX_OK;
}
int32_t ll_application_auxiliary_processes(void *raw,
    const uint8_t *key, size_t keyLength, const uint8_t *title, size_t titleLength,
    const uint8_t *identifier, size_t identifierLength, const uint8_t *document, size_t documentLength,
    LLAuxiliaryRow *rows, size_t rowCapacity, uint8_t *bytes, size_t byteCapacity,
    LLAuxiliaryInventory *out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !rows || !bytes || !out || !detail) return LL_AX_INVALID;
        *detail = 0;
        LLApplicationContext *application = (__bridge LLApplicationContext *)raw;
        LLWindowEntry *window = nil;
        int result = auxiliarySelectWindow(application, key, keyLength, title, titleLength,
            identifier, identifierLength, document, documentLength, &window, detail);
        if (result) return result;
        NSArray<LLAuxiliaryProcess *> *processes = nil;
        result = auxiliaryFrontier(application, window.element, &processes, detail); if (result) return result;
        return auxiliaryPublish(processes, rows, rowCapacity, bytes, byteCapacity, out);
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLApplicationContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
