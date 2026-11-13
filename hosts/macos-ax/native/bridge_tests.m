// Pure boundary tests only. This translation unit makes no AX reads or writes,
// does not prompt for permission and never controls any application.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <libproc.h>
#include <sys/proc_info.h>
#include <unistd.h>
#include <string.h>
static BOOL auxiliaryTreeActive = NO;
static BOOL auxiliaryAllowParentRead = NO;
static AXUIElementRef auxiliaryApp = NULL, auxiliaryWindow = NULL, auxiliaryForeign = NULL;
static int auxiliaryForeignReads = 0;
typedef struct {
    pid_t pid;
    uid_t uid;
    uint64_t seconds;
    uint32_t micros;
    const char *path;
} LLTestProcess;
static LLTestProcess processFixtures[5] = {0};
static int processInfoCalls = 0, processPathCalls = 0, processReadFault = 0;
static LLTestProcess *testProcess(pid_t pid) {
    for (size_t index = 0; index < 5; ++index) if (processFixtures[index].pid == pid) return &processFixtures[index];
    return NULL;
}
static int fakeProcessInfo(int pid, int flavor, uint64_t argument, void *buffer, int size) {
    (void)argument; ++processInfoCalls;
    LLTestProcess *fixture = testProcess(pid);
    if (!fixture || flavor != PROC_PIDTBSDINFO || size != sizeof(struct proc_bsdinfo) || processReadFault == 3) return 0;
    struct proc_bsdinfo info = {0};
    info.pbi_uid = fixture->uid; info.pbi_pid = (uint32_t)pid;
    info.pbi_start_tvsec = fixture->seconds; info.pbi_start_tvusec = fixture->micros;
    if (processReadFault == 1 && processInfoCalls > 1) ++info.pbi_start_tvsec;
    memcpy(buffer, &info, sizeof(info)); return sizeof(info);
}
static int fakeProcessPath(int pid, void *buffer, uint32_t capacity) {
    ++processPathCalls;
    LLTestProcess *fixture = testProcess(pid);
    if (!fixture || !fixture->path || processReadFault == 4) return 0;
    const char *path = processReadFault == 2 && processPathCalls > 1 ? "/usr/bin/false" : fixture->path;
    size_t length = strlen(path);
    if (length + 1 > capacity) return 0;
    memcpy(buffer, path, length + 1); return (int)length;
}
static Boolean fakeTrusted(void) { return true; }
static AXError countError = kAXErrorSuccess;
static CFIndex countValue = 0;
static int copyCalls = 0;
static AXError attributeError = kAXErrorFailure;
static CFTypeRef attributeValue = NULL;
static int attributeCalls = 0;
static AXError pidError = kAXErrorSuccess;
static pid_t pidValue = 0;
static AXError timeoutError = kAXErrorSuccess;
static int timeoutCalls = 0;
static AXError fakePid(AXUIElementRef element, pid_t *out) {
    if (auxiliaryTreeActive) {
        if (element == auxiliaryApp || element == auxiliaryWindow) { *out = 100; return kAXErrorSuccess; }
        if (element == auxiliaryForeign) { *out = 101; return kAXErrorSuccess; }
    }
    (void)element; *out = pidValue; return pidError;
}
static AXError fakeTimeout(AXUIElementRef element, float seconds) {
    (void)element; (void)seconds; ++timeoutCalls; return timeoutError;
}
static AXError fakeAttribute(AXUIElementRef element, CFStringRef name, CFTypeRef *out) {
    if (auxiliaryTreeActive) {
        if (element == auxiliaryForeign) {
            ++auxiliaryForeignReads;
            if (auxiliaryAllowParentRead && CFEqual(name, kAXParentAttribute)) {
                if (attributeError == kAXErrorSuccess && attributeValue) *out = CFRetain(attributeValue);
                return attributeError;
            }
            return kAXErrorFailure;
        }
        if (element == auxiliaryWindow && CFEqual(name, kAXRoleAttribute)) { *out = CFRetain(CFSTR("AXWindow")); return kAXErrorSuccess; }
        return kAXErrorAttributeUnsupported;
    }
    (void)element; (void)name;
    ++attributeCalls;
    if (attributeError == kAXErrorSuccess && attributeValue) *out = CFRetain(attributeValue);
    return attributeError;
}
static AXError fakeCount(AXUIElementRef element, CFStringRef name, CFIndex *count) {
    if (auxiliaryTreeActive) {
        if (element == auxiliaryForeign) { ++auxiliaryForeignReads; return kAXErrorFailure; }
        *count = (element == auxiliaryApp && CFEqual(name, kAXWindowsAttribute)) ||
            (element == auxiliaryWindow && CFEqual(name, kAXChildrenAttribute)) ? 1 : 0;
        return kAXErrorSuccess;
    }
    (void)element; (void)name; *count = countValue; return countError;
}
static AXError fakeCopy(AXUIElementRef element, CFStringRef name, CFIndex index,
                       CFIndex max, CFArrayRef *array) {
    if (auxiliaryTreeActive) {
        if (element == auxiliaryForeign) { ++auxiliaryForeignReads; return kAXErrorFailure; }
        AXUIElementRef value = element == auxiliaryApp && CFEqual(name, kAXWindowsAttribute) ? auxiliaryWindow : auxiliaryForeign;
        const void *values[] = {value};
        *array = CFArrayCreate(kCFAllocatorDefault, values, 1, &kCFTypeArrayCallBacks);
        return kAXErrorSuccess;
    }
    (void)element; (void)name; (void)index; (void)max; (void)array;
    ++copyCalls;
    return kAXErrorIllegalArgument;
}
#define AXUIElementGetAttributeValueCount fakeCount
#define AXUIElementCopyAttributeValues fakeCopy
#define AXUIElementCopyAttributeValue fakeAttribute
#define AXUIElementGetPid fakePid
#define AXUIElementSetMessagingTimeout fakeTimeout
#define AXIsProcessTrusted fakeTrusted
#define proc_pidinfo fakeProcessInfo
#define proc_pidpath fakeProcessPath
#import "bridge.m"
#undef AXUIElementGetAttributeValueCount
#undef AXUIElementCopyAttributeValues
#undef AXUIElementCopyAttributeValue
#undef AXUIElementGetPid
#undef AXUIElementSetMessagingTimeout
#undef AXIsProcessTrusted
#undef proc_pidinfo
#undef proc_pidpath
#include <assert.h>
#include <stdio.h>

// CFString-backed test identities are used only by pure CFEqual helpers. No
// fake object is ever passed to a remote AX operation.
static LLCF *pureIdentity(NSString *value) {
    LLCF *item = [LLCF new];
    item->value = CFRetain((__bridge CFStringRef)value);
    return item;
}
static NSDictionary *pureControl(NSString *label) {
    return @{ @"role": @"AXButton", @"identifier": label, @"label": label,
              @"value": [NSNull null], @"enabled": @YES, @"editable": @NO, @"pressable": @YES };
}

#include "scope_tests.inc"
#include "widgets_tests.inc"
#include "auxiliary_tests.inc"

int main(void) {
    @autoreleasepool {
        NSString *ascii = [@"" stringByPaddingToLength:4096 withString:@"a" startingAtIndex:0];
        NSData *bytes = nil;
        assert(utf8(ascii, &bytes) == LL_AX_OK && bytes.length == 4096);
        assert(utf8([ascii stringByAppendingString:@"a"], &bytes) == LL_AX_BOUNDS);
        NSString *emoji = [@"" stringByPaddingToLength:2048 withString:@"💾" startingAtIndex:0];
        assert(utf8(emoji, &bytes) == LL_AX_OK && bytes.length == 4096);
        assert(utf8([emoji stringByAppendingString:@"x"], &bytes) == LL_AX_BOUNDS);
        uint8_t invalid[] = {0xff};
        NSString *decoded = nil;
        assert(decode(invalid, 1, &decoded) == LL_AX_INVALID);
        assert(decode(NULL, 1, &decoded) == LL_AX_BOUNDS);
        assert(decode((const uint8_t *)"ok", 2, &decoded) == LL_AX_OK && [decoded isEqualToString:@"ok"]);
        puts("PASS bounded UTF-8 ingress and publication copies");

        NSString *url = nil, *same = nil;
        assert(canonicalFileURL(@"https://example.com/document.txt", &url) == LL_AX_INVALID);
        assert(canonicalFileURL(@"file://remote/tmp/document.txt", &url) == LL_AX_INVALID);
        assert(canonicalFileURL(@"file:///tmp/document.txt?query=1", &url) == LL_AX_INVALID);
        assert(canonicalFileURL(@"file:///tmp/document.txt#fragment", &url) == LL_AX_INVALID);
        assert(canonicalFileURL(@"file:///tmp/document%00.txt", &url) == LL_AX_INVALID);
        assert(canonicalFileURL(@"file:///tmp/leselang-ax-boundary.txt", &url) == LL_AX_OK);
        assert(canonicalFileURL(@"file://localhost/tmp/leselang-ax-boundary.txt", &same) == LL_AX_OK);
        assert([url isEqualToString:same]);
        puts("PASS canonical local document URLs and scope rejection");

        int32_t detail = 123;
        LLAXSnapshot snapshot = {7, 11, 13, 1};
        uint8_t output[8];
        memset(output, 0x5a, sizeof(output));
        assert(ll_ax_snapshot(NULL, output, sizeof(output), &snapshot, &detail) == LL_AX_INVALID);
        assert(snapshot.identity == 7 && snapshot.document_length == 11 && snapshot.value_length == 13 && snapshot.editable == 1);
        for (size_t index = 0; index < sizeof(output); ++index) assert(output[index] == 0x5a);
        assert(ll_ax_set_text(NULL, 1, NULL, 0, NULL, 0, 1, NULL, 0, &detail) == LL_AX_INVALID);
        assert(ll_ax_connect(NULL, 0, NULL, &detail) == LL_AX_INVALID);
        puts("PASS invalid handles refuse without partial output publication");

        assert(api(kAXErrorAPIDisabled, &detail) == LL_AX_PERMISSION);
        assert(api(kAXErrorInvalidUIElement, &detail) == LL_AX_STALE);
        assert(api(kAXErrorCannotComplete, &detail) == LL_AX_API && detail == kAXErrorCannotComplete);
        assert(api(kAXErrorSuccess, &detail) == LL_AX_OK);
        LLCF *fake = [LLCF new];
        NSMutableArray<LLCF *> *items = nil;
        // Real AX rejects index zero for an empty array: never issue that call.
        assert(children(fake, kAXChildrenAttribute, 1, &items, &detail) == LL_AX_OK && items.count == 0);
        assert(copyCalls == 0);
        countValue = LL_CHILD_LIMIT + 1;
        assert(children(fake, kAXChildrenAttribute, 1, &items, &detail) == LL_AX_BOUNDS && copyCalls == 0);
        countValue = 1;
        assert(children(fake, kAXChildrenAttribute, 1, &items, &detail) == LL_AX_API && detail == kAXErrorIllegalArgument);
        assert(copyCalls == 1); // Do not swallow a real nonempty-array error.
        countError = kAXErrorAttributeUnsupported;
        assert(children(fake, kAXChildrenAttribute, 1, &items, &detail) == LL_AX_OK && copyCalls == 1);
        countError = kAXErrorIllegalArgument;
        assert(children(fake, kAXChildrenAttribute, 1, &items, &detail) == LL_AX_API && copyCalls == 1);
        puts("PASS AX errors, empty-array ingress and discovery bounds");

        BOOL pressable = YES;
        assert(pressActionNames((__bridge CFArrayRef)@[], &pressable) == LL_AX_OK && !pressable);
        assert(pressActionNames((__bridge CFArrayRef)@[@"AXPress"], &pressable) == LL_AX_OK && pressable);
        assert(pressActionNames((__bridge CFArrayRef)@[@"AXShowMenu"], &pressable) == LL_AX_OK && !pressable);
        assert(pressActionNames((__bridge CFArrayRef)@[@"AXPress", @"AXPress"], &pressable) == LL_AX_INVALID);
        assert(pressActionNames((__bridge CFArrayRef)@[@1], &pressable) == LL_AX_INVALID);
        NSMutableArray *tooManyActions = [NSMutableArray new];
        for (NSUInteger i = 0; i < 17; ++i) [tooManyActions addObject:@"AXPress"];
        assert(pressActionNames((__bridge CFArrayRef)tooManyActions, &pressable) == LL_AX_BOUNDS);
        puts("PASS bounded, typed Calculator action names");

        LLCalculatorContext *calculator = [LLCalculatorContext new];
        calculator.identity = 11;
        LLCalcSnapshot expected = {.identity = 11, .display_length = 1, .button_identities = {12, 13, 14, 15}, .button_enabled = {1, 1, 1, 1}};
        LLCalcSnapshot current = expected;
        assert(calculatorExpected(calculator, &expected, @"0", @"0", nil, nil, &current, 0) == LL_AX_OK);
        assert(calculatorExpected(calculator, &expected, @"0", @"1", nil, nil, &current, 0) == LL_AX_STALE);
        current.button_identities[3] = 16;
        assert(calculatorExpected(calculator, &expected, @"0", @"0", nil, nil, &current, 0) == LL_AX_STALE);
        current = expected;
        current.button_enabled[3] = 0;
        assert(calculatorExpected(calculator, &expected, @"0", @"0", nil, nil, &current, 0) == LL_AX_STALE);
        expected.last_expression_present = 1; expected.last_expression_length = 5;
        current = expected;
        assert(calculatorExpected(calculator, &expected, @"3", @"3", @"1 + 2", @"1 + 2", &current, 0) == LL_AX_OK);
        assert(calculatorExpected(calculator, &expected, @"3", @"3", @"1 + 2", @"2 + 1", &current, 0) == LL_AX_STALE);
        expected.last_expression_present = 0;
        assert(calculatorExpected(calculator, &expected, @"3", @"3", nil, @"1 + 2", &current, 0) == LL_AX_INVALID);
        expected.last_expression_length = 0;
        assert(calculatorExpected(calculator, &expected, @"3", @"3", nil, @"1 + 2", &current, 0) == LL_AX_STALE);
        expected.button_enabled[0] = 0; current = expected;
        assert(calculatorExpected(calculator, &expected, @"0", @"0", nil, nil, &current, 0) == LL_AX_NOT_EDITABLE);
        LLCalcSnapshot untouched = {.identity = 99, .display_length = 7, .button_identities = {1, 2, 3, 4}, .button_enabled = {1, 1, 1, 1}};
        assert(ll_calc_snapshot(NULL, output, sizeof(output), &untouched, &detail) == LL_AX_INVALID);
        assert(untouched.identity == 99 && untouched.display_length == 7);
        for (size_t i = 0; i < sizeof(output); ++i) assert(output[i] == 0x5a);
        assert(ll_calc_press(NULL, &expected, NULL, 0, NULL, 0, 0, &detail) == LL_AX_INVALID);
        assert(ll_calc_connect(NULL, &detail) == LL_AX_INVALID);
        puts("PASS complete Calculator dependency comparisons and publication refusal");

        LLControlsContext *controls = [LLControlsContext new];
        controls.identity = 501;
        controls.kind = LL_CONTROLS_CALCULATOR;
        controls.nonce = @"00000000-0000-0000-0000-000000000000";
        LLCF *window = pureIdentity(@"window"), *replacementWindow = pureIdentity(@"replacement-window");
        LLCF *selectedWindow = nil;
        assert(controlsSelectWindow(controls, window, @"AXButton", @"main", &selectedWindow) == LL_AX_OK && !selectedWindow);
        assert(controlsSelectWindow(controls, window, @"AXWindow", @"other", &selectedWindow) == LL_AX_OK && !selectedWindow);
        assert(controlsSelectWindow(controls, window, @"AXWindow", @"main", &selectedWindow) == LL_AX_OK && selectedWindow == window);
        assert(controlsSelectWindow(controls, replacementWindow, @"AXWindow", @"main", &selectedWindow) == LL_AX_INVALID);
        controls.window = window;
        selectedWindow = nil;
        assert(controlsSelectWindow(controls, replacementWindow, @"AXWindow", @"main", &selectedWindow) == LL_AX_STALE);
        controls.kind = LL_CONTROLS_DOCUMENT;
        controls.document = @"file:///private/tmp/exact.txt";
        assert(controlsSelectWindow(controls, window, @"AXWindow", @"file:///private/tmp/other.txt", &selectedWindow) == LL_AX_OK && !selectedWindow);
        assert(controlsSelectWindow(controls, window, @"AXWindow", controls.document, &selectedWindow) == LL_AX_OK && selectedWindow == window);
        assert(controlsAdmittedRole(@"AXButton") && controlsAdmittedRole(@"AXTextArea") && controlsAdmittedRole(@"AXTextField") && controlsAdmittedRole(@"AXStaticText"));
        assert(!controlsAdmittedRole(@"AXMenuItem") && !controlsTextRole(@"AXButton"));
        assert(controlsDescribedRole(@"AXButton") && controlsDescribedRole(@"AXStaticText"));
        assert(!controlsDescribedRole(@"AXTextArea") && !controlsDescribedRole(@"AXTextField") && !controlsDescribedRole(@"AXGroup"));
        assert(controlsEffectiveEnabled(@"AXTextArea", NO, NO, YES));
        assert(controlsEffectiveEnabled(@"AXTextField", NO, NO, YES));
        assert(!controlsEffectiveEnabled(@"AXTextArea", YES, NO, YES));
        assert(!controlsEffectiveEnabled(@"AXTextArea", NO, NO, NO));
        assert(!controlsEffectiveEnabled(@"AXButton", NO, NO, YES));
        assert(controlsEffectiveEnabled(@"AXButton", YES, YES, NO));
        puts("PASS exact window selection, ambiguity and explicit control roles");

        NSString *typedString = nil;
        id typedDependency = nil;
        assert(controlsTypedValue(CFSTR(""), @"AXStaticText", &typedString, &typedDependency, &detail) == LL_AX_OK && [typedString isEqualToString:@""]);
        assert(([typedDependency isEqual:@[@"string", @""]]));
        assert(controlsTypedValue(kCFBooleanTrue, @"AXButton", &typedString, &typedDependency, &detail) == LL_AX_OK && !typedString);
        assert(([typedDependency isEqual:@[@"boolean", @YES]]));
        assert(controlsTypedValue((__bridge CFTypeRef)@8, @"AXButton", &typedString, &typedDependency, &detail) == LL_AX_OK && !typedString);
        assert(([typedDependency isEqual:@[@"number", @8]]));
        assert(controlsTypedValue((__bridge CFTypeRef)@8, @"AXTextArea", &typedString, &typedDependency, &detail) == LL_AX_INVALID && detail == -101);
        assert(controlsTypedValue((__bridge CFTypeRef)@(NAN), @"AXButton", &typedString, &typedDependency, &detail) == LL_AX_INVALID && detail == -102);
        assert(controlsTypedValue((__bridge CFTypeRef)[NSData data], @"AXButton", &typedString, &typedDependency, &detail) == LL_AX_INVALID && detail == -103);
        puts("PASS typed scalar dependencies without non-string value coercion");

        NSArray<NSString *> *names = nil;
        size_t nameBytes = 0;
        assert(controlsAttributeNames((__bridge CFTypeRef)@[@"AXValue", @"AXRole"], &names, &nameBytes) == LL_AX_OK);
        assert(([names isEqualToArray:@[@"AXRole", @"AXValue"]]) && nameBytes == 13);
        assert(controlsAttributeNames((__bridge CFTypeRef)@[@"AXRole", @"AXRole"], &names, &nameBytes) == LL_AX_INVALID);
        assert(controlsAttributeNames((__bridge CFTypeRef)@[@1], &names, &nameBytes) == LL_AX_INVALID);
        NSMutableArray *manyNames = [NSMutableArray new];
        for (NSUInteger index = 0; index < 129; ++index) [manyNames addObject:[NSString stringWithFormat:@"AX%lu", (unsigned long)index]];
        assert(controlsAttributeNames((__bridge CFTypeRef)manyNames, &names, &nameBytes) == LL_AX_BOUNDS);
        nameBytes = LL_CONTROLS_BYTES_LIMIT;
        assert(controlsAttributeNames((__bridge CFTypeRef)@[@"AXRole"], &names, &nameBytes) == LL_AX_BOUNDS);
        NSString *optional = nil;
        assert(optionalString(fake, kAXDescriptionAttribute, &optional, &detail) == LL_AX_API && detail == kAXErrorFailure && attributeCalls == 1);
        attributeError = kAXErrorAttributeUnsupported;
        assert(optionalString(fake, kAXDescriptionAttribute, &optional, &detail) == LL_AX_OK && optional.length == 0 && attributeCalls == 2);
        attributeError = kAXErrorNoValue;
        assert(optionalString(fake, kAXDescriptionAttribute, &optional, &detail) == LL_AX_OK && optional.length == 0 && attributeCalls == 3);
        attributeError = kAXErrorSuccess;
        attributeValue = kCFBooleanTrue;
        assert(optionalString(fake, kAXDescriptionAttribute, &optional, &detail) == LL_AX_INVALID && attributeCalls == 4);
        attributeValue = CFSTR("Last Expression");
        assert(optionalString(fake, kAXDescriptionAttribute, &optional, &detail) == LL_AX_OK && [optional isEqualToString:@"Last Expression"] && attributeCalls == 5);
        puts("PASS bounded advertised attributes and fail-closed optional reads");

        LLCF *button = pureIdentity(@"eight"), *display = pureIdentity(@"display"), *container = pureIdentity(@"container");
        NSArray *tree = @[window, container, button, display];
        NSArray *parents = @[@0, @0, @1, @1];
        NSArray *metadata = @[pureControl(@"Eight")];
        assert(controlsCommit(controls, @[button], metadata, @[@{@"display": @"0"}], tree, parents, 0) == LL_AX_OK);
        assert(controls.revision == 1 && controls.registry.count == 1 && controls.rows.count == 1);
        uint64_t original = controls.registry[0].identity;
        NSString *originalKey = controls.registry[0].key;
        assert(original && [originalKey hasPrefix:controls.nonce] && originalKey.length <= 57);
        assert(controlsCommit(controls, @[pureIdentity(@"eight")], metadata, @[@{@"display": @"0"}], tree, parents, 0) == LL_AX_OK);
        assert(controls.revision == 1 && controls.registry[0].identity == original);
        LLControlEntry *selectedEntry = nil;
        assert(controlsExpected(controls, controls.identity, 1, original, YES, &selectedEntry) == LL_AX_OK && selectedEntry.identity == original);
        assert(controlsExpected(controls, controls.identity, 1, original, NO, &selectedEntry) == LL_AX_NOT_EDITABLE);
        assert(controlsCommit(controls, @[button], metadata, @[@{@"display": @"8"}], tree, parents, 0) == LL_AX_OK);
        assert(controls.revision == 2);
        assert(controlsExpected(controls, controls.identity, 1, original, YES, &selectedEntry) == LL_AX_STALE);
        NSArray *changedContainer = @[window, pureIdentity(@"new-container"), button, display];
        assert(controlsCommit(controls, @[button], metadata, @[@{@"display": @"8"}], changedContainer, parents, 0) == LL_AX_OK && controls.revision == 3);
        assert(controlsCommit(controls, @[button], metadata, @[@{@"display": @"8"}], changedContainer, @[@0, @0, @0, @1], 0) == LL_AX_OK && controls.revision == 4);
        assert(controlsCommit(controls, @[], @[], @[], @[window], @[@0], 0) == LL_AX_OK);
        assert(!controls.registry[0].alive);
        assert(controlsExpected(controls, controls.identity, controls.revision, original, YES, &selectedEntry) == LL_AX_STALE);
        assert(controlsCommit(controls, @[button], metadata, @[], tree, parents, 0) == LL_AX_OK);
        assert(controls.registry.count == 2 && !controls.registry[0].alive && controls.registry[1].alive);
        assert(controls.registry[1].identity != original && ![controls.registry[1].key isEqualToString:originalKey]);
        assert(controlsCommit(controls, @[button, pureIdentity(@"eight")], @[pureControl(@"Eight"), pureControl(@"Eight")], @[], tree, parents, 0) == LL_AX_INVALID);
        puts("PASS stable CF identity, full scope epochs, topology and retired targets");

        uint64_t epoch = 99;
        assert(controlsEpoch(0, NO, &epoch) == LL_AX_OK && epoch == 1);
        assert(controlsEpoch(UINT64_MAX, NO, &epoch) == LL_AX_OK && epoch == UINT64_MAX);
        assert(controlsEpoch(UINT64_MAX, YES, &epoch) == LL_AX_BOUNDS && epoch == UINT64_MAX);
        controls.revision = UINT64_MAX;
        NSArray *savedRows = controls.rows;
        assert(controlsCommit(controls, @[], @[], @[], @[window], @[@0], 0) == LL_AX_BOUNDS);
        assert(controls.rows == savedRows && controls.registry[1].alive);
        controls.revision = 8;
        NSMutableArray *fullRegistry = [NSMutableArray new];
        for (NSUInteger index = 0; index < LL_CONTROLS_REGISTRY_LIMIT; ++index) {
            LLControlEntry *retired = [LLControlEntry new];
            retired.element = pureIdentity([NSString stringWithFormat:@"retired-%lu", (unsigned long)index]);
            [fullRegistry addObject:retired];
        }
        controls.registry = fullRegistry;
        assert(controlsCommit(controls, @[button], metadata, @[], tree, parents, 0) == LL_AX_BOUNDS);
        assert(controls.rows == savedRows && controls.registry.count == LL_CONTROLS_REGISTRY_LIMIT);
        NSMutableArray *tooManyControls = [NSMutableArray new], *tooManyMetadata = [NSMutableArray new];
        for (NSUInteger index = 0; index <= LL_CONTROLS_LIMIT; ++index) {
            [tooManyControls addObject:button]; [tooManyMetadata addObject:pureControl(@"Eight")];
        }
        assert(controlsCommit(controls, tooManyControls, tooManyMetadata, @[], tree, parents, 0) == LL_AX_BOUNDS);
        assert(controlsCommit(controls, @[], @[], @[], @[window], @[@0], LL_CONTROLS_BYTES_LIMIT + 1) == LL_AX_BOUNDS);
        LLControlRow rawRows[LL_CONTROLS_LIMIT];
        memset(rawRows, 0x5a, sizeof(rawRows));
        LLControlsSnapshot controlsOut = {71, 72, 73, 74};
        assert(controlsPublish(controls, rawRows, LL_CONTROLS_LIMIT, output, sizeof(output), &controlsOut) == LL_AX_BOUNDS);
        assert(controlsOut.identity == 71 && controlsOut.revision == 72 && controlsOut.count == 73 && controlsOut.bytes_length == 74);
        for (size_t i = 0; i < sizeof(rawRows); ++i) assert(((uint8_t *)rawRows)[i] == 0x5a);
        for (size_t i = 0; i < sizeof(output); ++i) assert(output[i] == 0x5a);
        assert(ll_controls_snapshot(NULL, rawRows, LL_CONTROLS_LIMIT, output, sizeof(output), &controlsOut, &detail) == LL_AX_INVALID);
        assert(ll_controls_press(NULL, 1, 1, 2, &detail) == LL_AX_INVALID);
        assert(ll_controls_set_text(NULL, 1, 1, 2, NULL, 0, &detail) == LL_AX_INVALID);
        assert(ll_controls_connect(0, NULL, 0, NULL, &detail) == LL_AX_INVALID);
        puts("PASS generic scope epoch overflow, lifetime bounds and atomic publication");
        runScopeTests();
        runWidgetTests();
        runAuxiliaryTests();
    }
    puts("Accessibility bridge boundaries: 37 groups passed (no remote GUI calls).");
    return 0;
}
