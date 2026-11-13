#import "bridge.h"
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <libproc.h>
#include <sys/proc_info.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>

enum { LL_TEXT_LIMIT = 4096, LL_CHILD_LIMIT = 64, LL_NODE_LIMIT = 128, LL_DEPTH_LIMIT = 16 };
static uint64_t nextIdentity = 1; // Main-thread only; never wrap or reuse.

// ARC owns the holder; it releases its retained CF value even on an exception.
@interface LLCF : NSObject { @public CFTypeRef value; }
@end
@implementation LLCF
- (void)dealloc { if (value) CFRelease(value); }
@end

@interface LLAXContext : NSObject
@property(nonatomic, strong) LLCF *application;
@property(nonatomic, strong) LLCF *window;
@property(nonatomic, strong) LLCF *editor;
@property(nonatomic, copy) NSString *document;
@property(nonatomic, copy) NSString *executable;
@property(nonatomic) pid_t pid;
@property(nonatomic) uint64_t startSeconds;
@property(nonatomic) uint64_t startMicros;
@property(nonatomic) uint64_t identity;
@end
@implementation LLAXContext
@end

static int api(AXError error, int32_t *detail) {
    if (error == kAXErrorSuccess) return LL_AX_OK;
    if (detail) *detail = (int32_t)error;
    if (error == kAXErrorAPIDisabled) return LL_AX_PERMISSION;
    if (error == kAXErrorInvalidUIElement) return LL_AX_STALE;
    return LL_AX_API;
}
static int utf8(NSString *text, NSData **out) {
    if (![text isKindOfClass:[NSString class]]) return LL_AX_INVALID;
    // Check before allocating an encoded copy. UTF-16 length bounds the number
    // of code units traversed by the subsequent UTF-8 byte-length calculation.
    if (text.length > LL_TEXT_LIMIT ||
        [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > LL_TEXT_LIMIT) return LL_AX_BOUNDS;
    NSData *bytes = [text dataUsingEncoding:NSUTF8StringEncoding allowLossyConversion:NO];
    if (!bytes) return LL_AX_INVALID;
    if (out) *out = bytes;
    return LL_AX_OK;
}
static int decode(const uint8_t *bytes, size_t length, NSString **out) {
    if ((!bytes && length) || length > LL_TEXT_LIMIT) return LL_AX_BOUNDS;
    NSString *text = [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
    if (!text) return LL_AX_INVALID;
    *out = text;
    return LL_AX_OK;
}
static BOOL containsNul(NSString *text) {
    for (NSUInteger i = 0; i < text.length; ++i) if ([text characterAtIndex:i] == 0) return YES;
    return NO;
}
static int element(LLCF *item, pid_t pid, int32_t *detail) {
    if (!item || !item->value || CFGetTypeID(item->value) != AXUIElementGetTypeID()) return LL_AX_INVALID;
    AXUIElementRef raw = (AXUIElementRef)item->value;
    pid_t actual = 0;
    int result = api(AXUIElementGetPid(raw, &actual), detail);
    if (result) return result;
    if (actual != pid) {
        // This is a local ownership refusal, not the last optional AX error.
        // System panels can expose elements hosted by another process.
        if (detail) *detail = 0;
        const char *diagnostics = getenv("LESELANG_AX_DIAGNOSTICS");
        if (diagnostics && strcmp(diagnostics, "1") == 0) {
            fprintf(stderr, "AX element ownership refused: expected_pid=%d, actual_pid=%d\n", pid, actual);
        }
        return LL_AX_STALE;
    }
    // Timeouts belong to each AX ref; the app ref's timeout does not configure
    // newly discovered children. No failed AX call is retried automatically.
    return api(AXUIElementSetMessagingTimeout(raw, 0.5f), detail);
}
static int attribute(LLCF *item, CFStringRef name, LLCF **out, int32_t *detail) {
    LLCF *storage = [LLCF new];
    int result = api(AXUIElementCopyAttributeValue((AXUIElementRef)item->value, name, &storage->value), detail);
    if (result) return result;
    if (!storage->value) return LL_AX_INVALID;
    *out = storage;
    return LL_AX_OK;
}
static int stringAttribute(LLCF *item, CFStringRef name, NSString **out, int32_t *detail) {
    LLCF *storage = nil;
    int result = attribute(item, name, &storage, detail);
    if (result) return result;
    if (CFGetTypeID(storage->value) != CFStringGetTypeID()) return LL_AX_INVALID;
    NSString *text = (__bridge NSString *)storage->value;
    result = utf8(text, NULL);
    if (result) return result;
    *out = [text copy];
    return LL_AX_OK;
}
typedef int (*LLElementCheck)(LLCF *item, void *context, int32_t *detail);
static int childrenChecked(LLCF *item, CFStringRef name, LLElementCheck check, void *context,
                           NSMutableArray<LLCF *> **out, int32_t *detail) {
    CFIndex declaredCount = 0;
    AXError countError = AXUIElementGetAttributeValueCount((AXUIElementRef)item->value, name, &declaredCount);
    if (countError == kAXErrorNoValue || countError == kAXErrorAttributeUnsupported ||
        (countError == kAXErrorSuccess && declaredCount == 0)) {
        *out = [NSMutableArray new];
        return LL_AX_OK;
    }
    int countResult = api(countError, detail);
    if (countResult) return countResult;
    if (declaredCount < 0 || declaredCount > LL_CHILD_LIMIT) return LL_AX_BOUNDS;
    LLCF *storage = [LLCF new];
    CFArrayRef values = NULL;
    AXError error = AXUIElementCopyAttributeValues((AXUIElementRef)item->value, name, 0, LL_CHILD_LIMIT + 1, &values);
    storage->value = values;
    if (error == kAXErrorNoValue || error == kAXErrorAttributeUnsupported) {
        *out = [NSMutableArray new];
        return LL_AX_OK;
    }
    int result = api(error, detail);
    if (result) return result;
    if (!values || CFGetTypeID(values) != CFArrayGetTypeID()) return LL_AX_INVALID;
    CFIndex count = CFArrayGetCount(values);
    if (count < 0 || count > LL_CHILD_LIMIT) return LL_AX_BOUNDS;
    NSMutableArray<LLCF *> *items = [NSMutableArray new];
    for (CFIndex i = 0; i < count; ++i) {
        CFTypeRef child = CFArrayGetValueAtIndex(values, i);
        if (!child) return LL_AX_INVALID;
        LLCF *owned = [LLCF new];
        owned->value = CFRetain(child);
        result = check(owned, context, detail);
        if (result) return result;
        [items addObject:owned];
    }
    *out = items;
    return LL_AX_OK;
}
static int childSameProcess(LLCF *item, void *context, int32_t *detail) {
    return element(item, *(pid_t *)context, detail);
}
static int children(LLCF *item, CFStringRef name, pid_t pid,
                    NSMutableArray<LLCF *> **out, int32_t *detail) {
    return childrenChecked(item, name, childSameProcess, &pid, out, detail);
}
static int canonicalFileURL(NSString *text, NSString **out) {
    int result = utf8(text, NULL);
    if (result) return result;
    // NSURL can normalize away an escaped NUL. Refuse it before parsing so an
    // invalid remote spelling cannot alias a different permitted file path.
    if (containsNul(text) || [text rangeOfString:@"%00" options:NSCaseInsensitiveSearch].location != NSNotFound) return LL_AX_INVALID;
    NSURL *url = [NSURL URLWithString:text];
    if (!url.isFileURL || url.query || url.fragment || (url.host.length && ![url.host isEqualToString:@"localhost"])) return LL_AX_INVALID;
    if (!url.path.isAbsolutePath || containsNul(url.path)) return LL_AX_INVALID;
    // Normalize a local localhost URL to the same spelling as a file path.
    NSString *canonical = [NSURL fileURLWithPath:url.path].URLByResolvingSymlinksInPath.URLByStandardizingPath.absoluteString;
    result = utf8(canonical, NULL);
    if (result) return result;
    *out = canonical;
    return LL_AX_OK;
}
static int canonicalDocument(LLCF *window, NSString **out, int32_t *detail) {
    LLCF *storage = nil;
    int result = attribute(window, kAXDocumentAttribute, &storage, detail);
    if (result) return result;
    NSString *text = nil;
    if (CFGetTypeID(storage->value) == CFStringGetTypeID()) text = (__bridge NSString *)storage->value;
    else if (CFGetTypeID(storage->value) == CFURLGetTypeID()) text = [(__bridge NSURL *)storage->value absoluteString];
    else return LL_AX_INVALID;
    return canonicalFileURL(text, out);
}
static int processIdentity(pid_t pid, struct proc_bsdinfo *info, NSString **path) {
    int infoSize = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, info, sizeof(*info));
    if (infoSize != sizeof(*info)) return LL_AX_STALE;
    char buffer[PROC_PIDPATHINFO_MAXSIZE] = {0};
    int length = proc_pidpath(pid, buffer, sizeof(buffer));
    if (length <= 0 || length >= (int)sizeof(buffer)) return LL_AX_STALE;
    NSString *value = [[NSString alloc] initWithBytes:buffer length:strlen(buffer) encoding:NSUTF8StringEncoding];
    if (!value) return LL_AX_INVALID;
    int result = utf8(value, NULL);
    if (result) return result;
    *path = value.stringByResolvingSymlinksInPath.stringByStandardizingPath;
    return LL_AX_OK;
}
static int discoverApp(LLAXContext *context, NSString *bundlePath, NSString *bundleIdentifier,
                       int32_t *detail) {
    NSBundle *bundle = [NSBundle bundleWithPath:bundlePath];
    if (!bundle || ![bundle.bundleIdentifier isEqualToString:bundleIdentifier]) return LL_AX_UNAVAILABLE;
    NSString *expected = bundle.executableURL.path.stringByResolvingSymlinksInPath.stringByStandardizingPath;
    if (!expected || utf8(expected, NULL)) return LL_AX_UNAVAILABLE;
    // Launch Services may return processIdentifier == -1 for a visible app in
    // command-line contexts. Select the requested system executable directly;
    // retain the same PID/start/path guards, without focus or title fallbacks.
    enum { processLimit = 4096 };
    pid_t pids[processLimit + 1] = {0};
    int size = proc_listpids(PROC_UID_ONLY, (uint32_t)geteuid(), pids, sizeof(pids));
    if (size < 0 || size % sizeof(pid_t) != 0) { *detail = errno; return LL_AX_API; }
    if (size >= (int)sizeof(pids)) return LL_AX_BOUNDS;
    NSUInteger matches = 0;
    for (size_t index = 0; index < (size_t)size / sizeof(pid_t); ++index) {
        pid_t pid = pids[index];
        if (pid <= 0) continue;
        char pathBytes[PROC_PIDPATHINFO_MAXSIZE] = {0};
        int length = proc_pidpath(pid, pathBytes, sizeof(pathBytes));
        // An unrelated process can disappear during discovery. No text or
        // other application data is inspected; only executable paths compare.
        if (length <= 0 || length >= (int)sizeof(pathBytes)) continue;
        NSString *path = [[NSString alloc] initWithBytes:pathBytes length:strlen(pathBytes) encoding:NSUTF8StringEncoding];
        if (!path || ![path.stringByResolvingSymlinksInPath.stringByStandardizingPath isEqualToString:expected]) continue;
        struct proc_bsdinfo info = {0};
        NSString *actual = nil;
        int result = processIdentity(pid, &info, &actual);
        if (result) return result;
        if (info.pbi_uid != geteuid() || ![actual isEqualToString:expected]) return LL_AX_STALE;
        if (++matches > 1) return LL_AX_INVALID;
        context.pid = pid;
        context.executable = actual;
        context.startSeconds = info.pbi_start_tvsec;
        context.startMicros = info.pbi_start_tvusec;
    }
    return matches == 1 ? LL_AX_OK : LL_AX_UNAVAILABLE;
}
static int discoverProcess(LLAXContext *context, int32_t *detail) {
    return discoverApp(context, @"/System/Applications/TextEdit.app", @"com.apple.TextEdit", detail);
}
static int permission(LLAXContext *context, int32_t *detail) {
    if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
    if (!context) return LL_AX_INVALID;
    if (!AXIsProcessTrusted()) return LL_AX_PERMISSION;
    struct proc_bsdinfo info = {0};
    NSString *executable = nil;
    int result = processIdentity(context.pid, &info, &executable);
    if (result) return result;
    if (context.startSeconds != info.pbi_start_tvsec || context.startMicros != info.pbi_start_tvusec ||
        info.pbi_uid != geteuid() || ![context.executable isEqualToString:executable]) return LL_AX_STALE;
    return element(context.application, context.pid, detail);
}

// Complete bounded traversal: an ambiguous text area or an unvisited subtree
// exceeding the limit fails the whole selection, rather than choosing first.
static int findEditor(LLAXContext *context, LLCF **windowOut, LLCF **editorOut, int32_t *detail) {
    int result = permission(context, detail);
    if (result) return result;
    NSMutableArray<LLCF *> *windows = nil;
    result = children(context.application, kAXWindowsAttribute, context.pid, &windows, detail);
    if (result) return result;
    LLCF *window = nil;
    for (LLCF *candidate in windows) {
        NSString *document = nil;
        result = canonicalDocument(candidate, &document, detail);
        if (result == LL_AX_API && detail && (*detail == kAXErrorAttributeUnsupported || *detail == kAXErrorNoValue)) continue;
        if (result) return result;
        if ([document isEqualToString:context.document]) {
            if (window) return LL_AX_INVALID;
            window = candidate;
        }
    }
    if (!window) return LL_AX_UNAVAILABLE;
    NSMutableArray<LLCF *> *queue = [NSMutableArray arrayWithObject:window];
    NSMutableArray<NSNumber *> *depths = [NSMutableArray arrayWithObject:@0];
    LLCF *editor = nil;
    for (NSUInteger index = 0; index < queue.count; ++index) {
        LLCF *candidate = queue[index];
        NSUInteger depth = depths[index].unsignedIntegerValue;
        NSString *role = nil;
        result = stringAttribute(candidate, kAXRoleAttribute, &role, detail);
        if (result) return result;
        if ([role isEqualToString:(__bridge NSString *)kAXTextAreaRole]) {
            if (editor) return LL_AX_INVALID;
            editor = candidate;
        }
        NSMutableArray<LLCF *> *descendants = nil;
        result = children(candidate, kAXChildrenAttribute, context.pid, &descendants, detail);
        if (result) return result;
        if ((depth >= LL_DEPTH_LIMIT && descendants.count) || queue.count + descendants.count > LL_NODE_LIMIT) return LL_AX_BOUNDS;
        for (LLCF *child in descendants) {
            for (LLCF *seen in queue) {
                // Cycles/repeated children are malformed or ambiguous discovery.
                if (CFEqual(seen->value, child->value)) return LL_AX_INVALID;
            }
            [queue addObject:child];
            [depths addObject:@(depth + 1)];
        }
    }
    if (!editor) return LL_AX_UNAVAILABLE;
    if ((context.window && !CFEqual(context.window->value, window->value)) ||
        (context.editor && !CFEqual(context.editor->value, editor->value))) return LL_AX_STALE;
    *windowOut = window;
    *editorOut = editor;
    return LL_AX_OK;
}
static int observe(LLAXContext *context, NSString **valueOut, BOOL *editableOut, int32_t *detail) {
    LLCF *window = nil, *editor = nil;
    int result = findEditor(context, &window, &editor, detail);
    if (result) return result;
    NSString *value = nil;
    result = stringAttribute(editor, kAXValueAttribute, &value, detail);
    if (result) return result;
    Boolean editable = false;
    result = api(AXUIElementIsAttributeSettable((AXUIElementRef)editor->value, kAXValueAttribute, &editable), detail);
    if (result) return result;
    // Process exit/relaunch while the remote observation was collected also
    // invalidates it. Remote reads themselves are not an atomic snapshot.
    result = permission(context, detail);
    if (result) return result;
    *valueOut = value;
    *editableOut = editable;
    return LL_AX_OK;
}

int32_t ll_ax_trusted(void) {
    @autoreleasepool { @try { return AXIsProcessTrusted() ? 1 : 0; }
    @catch (NSException *exception) { (void)exception; return 0; } }
}
int32_t ll_ax_connect(const uint8_t *path, size_t length, void **out, int32_t *detail) {
    @autoreleasepool { @try {
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        if (!out || !detail) return LL_AX_INVALID;
        *out = NULL; *detail = 0;
        if (!AXIsProcessTrusted()) return LL_AX_PERMISSION;
        NSString *file = nil;
        int result = decode(path, length, &file);
        if (result) return result;
        if (!file.isAbsolutePath || containsNul(file)) return LL_AX_INVALID;
        NSString *document = [NSURL fileURLWithPath:file].URLByResolvingSymlinksInPath.URLByStandardizingPath.absoluteString;
        result = utf8(document, NULL);
        if (result) return result;
        LLAXContext *context = [LLAXContext new];
        context.document = document;
        result = discoverProcess(context, detail);
        if (result) return result;
        context.application = [LLCF new];
        context.application->value = AXUIElementCreateApplication(context.pid);
        LLCF *window = nil, *editor = nil;
        result = findEditor(context, &window, &editor, detail);
        if (result) return result;
        context.window = window;
        context.editor = editor;
        if (nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
        context.identity = nextIdentity++;
        *out = (__bridge_retained void *)context;
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_ax_destroy(void *raw) {
    @autoreleasepool { @try {
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        if (!raw) return LL_AX_INVALID;
        LLAXContext *context = (__bridge_transfer LLAXContext *)raw;
        (void)context;
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; return LL_AX_API; } }
}
int32_t ll_ax_permission(void *raw, int32_t *detail) {
    @autoreleasepool { @try {
        if (!detail) return LL_AX_INVALID;
        *detail = 0;
        return permission((__bridge LLAXContext *)raw, detail);
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_ax_snapshot(void *raw, uint8_t *bytes, size_t capacity,
                       LLAXSnapshot *out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!out || !detail || !bytes) return LL_AX_INVALID;
        *detail = 0;
        LLAXContext *context = (__bridge LLAXContext *)raw;
        NSString *value = nil;
        BOOL editable = NO;
        int result = observe(context, &value, &editable, detail);
        if (result) return result;
        NSData *documentBytes = nil, *valueBytes = nil;
        result = utf8(context.document, &documentBytes);
        if (result) return result;
        result = utf8(value, &valueBytes);
        if (result) return result;
        if (documentBytes.length + valueBytes.length > capacity) return LL_AX_BOUNDS;
        // Publication happens only after the entire snapshot has validated.
        memcpy(bytes, documentBytes.bytes, documentBytes.length);
        if (valueBytes.length) memcpy(bytes + documentBytes.length, valueBytes.bytes, valueBytes.length);
        *out = (LLAXSnapshot){context.identity, documentBytes.length, valueBytes.length, editable ? 1 : 0};
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_ax_set_text(void *raw, uint64_t identity,
                      const uint8_t *document, size_t document_length,
                      const uint8_t *expected_value, size_t expected_length,
                      int32_t expected_editable,
                      const uint8_t *text, size_t text_length, int32_t *detail) {
    BOOL entered = NO;
    @autoreleasepool { @try {
        if (!detail) return LL_AX_INVALID;
        *detail = 0;
        LLAXContext *context = (__bridge LLAXContext *)raw;
        int result = permission(context, detail);
        if (result) return result;
        if (identity != context.identity || expected_editable != 1) return LL_AX_STALE;
        NSString *wantedDocument = nil, *expected = nil, *replacement = nil;
        result = decode(document, document_length, &wantedDocument);
        if (result) return result;
        result = decode(expected_value, expected_length, &expected);
        if (result) return result;
        result = decode(text, text_length, &replacement);
        if (result) return result;
        if (![context.document isEqualToString:wantedDocument]) return LL_AX_STALE;
        NSString *current = nil;
        BOOL editable = NO;
        result = observe(context, &current, &editable, detail);
        if (result) return result;
        if (!editable) return LL_AX_NOT_EDITABLE;
        if (![current isEqualToString:expected]) return LL_AX_STALE;
        // Last point before the remote write. The external app can still change
        // between this check and mutation; AX supplies no compare-and-set API.
        entered = YES;
        AXError error = AXUIElementSetAttributeValue((AXUIElementRef)context.editor->value,
                                                    kAXValueAttribute, (__bridge CFStringRef)replacement);
        if (error != kAXErrorSuccess) { *detail = (int32_t)error; return LL_AX_UNCERTAIN; }
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return entered ? LL_AX_UNCERTAIN : LL_AX_API; } }
}

// A second scoped profile shares the same private transport ownership,
// process checks and bounded AX readers without changing the TextEdit ABI.
#include "calculator.inc"
#include "controls.inc"
#include "scope.inc"
#include "auxiliary.inc"
#include "widgets.inc"
