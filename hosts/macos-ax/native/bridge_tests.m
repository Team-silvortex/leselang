// Pure boundary tests only. This translation unit makes no AX reads or writes,
// does not prompt for permission and never controls any application.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
static AXError countError = kAXErrorSuccess;
static CFIndex countValue = 0;
static int copyCalls = 0;
static AXError fakeCount(AXUIElementRef element, CFStringRef name, CFIndex *count) {
    (void)element; (void)name; *count = countValue; return countError;
}
static AXError fakeCopy(AXUIElementRef element, CFStringRef name, CFIndex index,
                       CFIndex max, CFArrayRef *array) {
    (void)element; (void)name; (void)index; (void)max; (void)array;
    ++copyCalls;
    return kAXErrorIllegalArgument;
}
#define AXUIElementGetAttributeValueCount fakeCount
#define AXUIElementCopyAttributeValues fakeCopy
#import "bridge.m"
#undef AXUIElementGetAttributeValueCount
#undef AXUIElementCopyAttributeValues
#include <assert.h>
#include <stdio.h>

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
    }
    puts("Accessibility bridge boundaries: 4 groups passed (no remote GUI calls).");
    return 0;
}
