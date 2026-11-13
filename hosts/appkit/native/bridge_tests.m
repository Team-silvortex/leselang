// A main-thread native conformance binary, intentionally including the bridge
// in this translation unit to mutate private fixtures without public test hooks.
#import "bridge.m"
#include <assert.h>
#include <pthread.h>
#include <stdio.h>

static LlGuiSnapshot takeSnapshot(LlGui *gui, int slot, uint8_t *buffer) {
    LlGuiSnapshot snapshot;
    assert(ll_gui_snapshot(gui, slot, buffer, LL_GUI_MAX_TEXT_BYTES * 3, &snapshot) == LL_GUI_OK);
    return snapshot;
}
static void resetControls(LlGui *gui) {
    assert(ll_gui_replace(gui, 2) == LL_GUI_OK);
    assert(ll_gui_replace(gui, 0) == LL_GUI_OK);
    assert(ll_gui_replace(gui, 1) == LL_GUI_OK);
}
static void listAndEdit(LlGui *gui) {
    uint8_t packed[4096]; size_t length = 0, count = 0;
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_OK);
    assert(count == 3 && length == strlen("input") + strlen("save") + strlen("other") + 3);
    int32_t slot = -1; uintptr_t identity = 0;
    assert(ll_gui_find(gui, (const uint8_t *)"input", 5, &slot, &identity) == LL_GUI_OK && slot == 2);
    uint8_t buffer[LL_GUI_MAX_TEXT_BYTES * 3];
    LlGuiSnapshot input = takeSnapshot(gui, 2, buffer);
    assert(input.identity == identity && input.role == 2 && input.editable == 1 && input.value_length == 0);
    LlGuiSnapshot save = takeSnapshot(gui, 0, buffer);
    const char *message = "LeseLang ✓";
    assert(ll_gui_set_text(gui, 2, input.identity, (const uint8_t *)message, strlen(message)) == LL_GUI_OK);
    input = takeSnapshot(gui, 2, buffer);
    assert(input.mutation == 1 && input.value_length == strlen(message));
    assert(memcmp(buffer + input.identifier_length + input.title_length, message, strlen(message)) == 0);
    LlGuiSnapshot changed = takeSnapshot(gui, 0, buffer);
    assert(changed.mutation == save.mutation + 1 && changed.value_length == 0 && changed.clicks == 0);
    assert(ll_gui_click(gui, 0, save.identity) == LL_GUI_OK);
    changed = takeSnapshot(gui, 0, buffer);
    assert(changed.clicks == 1 && changed.value_length == strlen(message));
    assert(memcmp(buffer + changed.identifier_length + changed.title_length, message, strlen(message)) == 0);
    assert(ll_gui_set_text(gui, 0, save.identity, (const uint8_t *)message, strlen(message)) == LL_GUI_BAD_INPUT);
    assert(ll_gui_set_editable(gui, 2, 0) == LL_GUI_OK);
    assert(ll_gui_set_text(gui, 2, input.identity, (const uint8_t *)message, strlen(message)) == LL_GUI_BAD_INPUT);
    assert(ll_gui_set_editable(gui, 2, 1) == LL_GUI_OK);
    assert(ll_gui_set_enabled(gui, 2, 0) == LL_GUI_OK);
    assert(ll_gui_set_text(gui, 2, input.identity, (const uint8_t *)message, strlen(message)) == LL_GUI_BAD_INPUT);
    assert(ll_gui_set_enabled(gui, 2, 1) == LL_GUI_OK);
    uintptr_t old = input.identity;
    assert(ll_gui_replace(gui, 2) == LL_GUI_OK);
    input = takeSnapshot(gui, 2, buffer);
    assert(input.identity != old);
    assert(ll_gui_set_text(gui, 2, old, (const uint8_t *)message, strlen(message)) == LL_GUI_STALE_IDENTITY);
    puts("PASS real field notification, submit value, role/editable/identity fences");
}
static void discoveryFailures(LlGui *gui) {
    LlGuiContext *app = (__bridge LlGuiContext *)gui;
    uint8_t packed[4096]; memset(packed, 0xA5, sizeof(packed));
    size_t length = 123, count = 123;
    app.buttons[1].identifier = @"save";
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_AMBIGUOUS);
    assert(length == 123 && count == 123 && packed[0] == 0xA5);
    int32_t slot = -1; uintptr_t token = 0;
    assert(ll_gui_find(gui, (const uint8_t *)"save", 4, &slot, &token) == LL_GUI_AMBIGUOUS);
    app.buttons[1].identifier = @"other";
    NSButton *ordinary = [[NSButton alloc] initWithFrame:NSZeroRect];
    ordinary.identifier = @"save";
    [app.window.contentView addSubview:ordinary];
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_OK && count == 3);
    [ordinary removeFromSuperview];
    LlGuiButton *unregistered = [[LlGuiButton alloc] initWithFrame:NSZeroRect];
    unregistered.identifier = @"unregistered";
    [app.window.contentView addSubview:unregistered];
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_NOT_FOUND);
    [unregistered removeFromSuperview];
    app.input.identifier = [@"x" stringByPaddingToLength:4097 withString:@"x" startingAtIndex:0];
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_BAD_INPUT);
    app.input.identifier = @"input";
    // Even a selector matching early in the tree must reject an invalid cold tail.
    NSMutableArray<NSView *> *extra = [NSMutableArray array];
    for (NSUInteger index = 0; index < 65; index++) {
        NSView *view = [[NSView alloc] initWithFrame:NSZeroRect];
        [extra addObject:view]; [app.window.contentView addSubview:view];
    }
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_TREE_BOUNDS);
    assert(ll_gui_find(gui, (const uint8_t *)"save", 4, &slot, &token) == LL_GUI_TREE_BOUNDS);
    for (NSView *view in extra) [view removeFromSuperview];
    NSView *branch = [[NSView alloc] initWithFrame:NSZeroRect];
    [app.window.contentView addSubview:branch];
    NSView *current = branch;
    for (NSUInteger index = 0; index < 17; index++) {
        NSView *child = [[NSView alloc] initWithFrame:NSZeroRect];
        [current addSubview:child]; current = child;
    }
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_TREE_BOUNDS);
    [branch removeFromSuperview];
    assert(ll_gui_list(gui, packed, sizeof(packed), &length, &count) == LL_GUI_OK && count == 3);
    puts("PASS duplicate identifiers, registered-only metadata, complete tree bounds");
}
static void externalNotificationsAndBounds(LlGui *gui) {
    LlGuiContext *app = (__bridge LlGuiContext *)gui;
    uint8_t buffer[LL_GUI_MAX_TEXT_BYTES * 3];
    LlGuiSnapshot input = takeSnapshot(gui, 2, buffer);
    LlGuiSnapshot save = takeSnapshot(gui, 0, buffer);
    // Model the actual AppKit external-edit delivery, not the controlled setter.
    for (NSString *value in @[@"external", @""]) {
        app.input.stringValue = value;
        [NSNotificationCenter.defaultCenter postNotificationName:NSControlTextDidChangeNotification object:app.input];
    }
    LlGuiSnapshot after = takeSnapshot(gui, 2, buffer);
    assert(after.mutation == input.mutation + 2 && after.value_length == 0);
    assert(takeSnapshot(gui, 0, buffer).mutation == save.mutation + 2);
    uint8_t oversized[4097]; memset(oversized, 'x', sizeof(oversized));
    uint8_t invalid[] = { 0xFF };
    assert(ll_gui_set_text(gui, 2, input.identity, oversized, sizeof(oversized)) == LL_GUI_TEXT_BOUNDS);
    assert(ll_gui_set_text(gui, 2, input.identity, invalid, sizeof(invalid)) == LL_GUI_INVALID_UTF8);
    assert(takeSnapshot(gui, 2, buffer).mutation == after.mutation);
    LlGuiSnapshot output;
    memset(buffer, 0xA5, sizeof(buffer));
    assert(ll_gui_snapshot(gui, 2, buffer, 1, &output) == LL_GUI_TEXT_BOUNDS);
    assert(buffer[0] == 0xA5);
    app.input.stringValue = [@"x" stringByPaddingToLength:4097 withString:@"x" startingAtIndex:0];
    [NSNotificationCenter.defaultCenter postNotificationName:NSControlTextDidChangeNotification object:app.input];
    assert(ll_gui_snapshot(gui, 2, buffer, sizeof(buffer), &output) == LL_GUI_TEXT_BOUNDS);
    puts("PASS external editing ABA, UTF-8 and buffer bounds before publication");
}
static void *wrongThread(void *context) {
    LlGui *out = NULL;
    assert(ll_gui_create(&out) == LL_GUI_WRONG_THREAD && out == NULL);
    int32_t open = 0;
    assert(ll_gui_is_open((LlGui *)context, &open) == LL_GUI_WRONG_THREAD);
    return NULL;
}
int main(void) {
    @autoreleasepool {
        LlGui *gui = NULL;
        assert(ll_gui_create(&gui) == LL_GUI_OK);
        assert(ll_gui_show(gui) == LL_GUI_OK);
        listAndEdit(gui);
        resetControls(gui);
        discoveryFailures(gui);
        resetControls(gui);
        externalNotificationsAndBounds(gui);
        pthread_t worker;
        assert(pthread_create(&worker, NULL, wrongThread, gui) == 0);
        assert(pthread_join(worker, NULL) == 0);
        assert(ll_gui_close(gui) == LL_GUI_OK);
        uint8_t bytes[4096]; size_t length = 0, count = 0;
        assert(ll_gui_list(gui, bytes, sizeof(bytes), &length, &count) == LL_GUI_CLOSED);
        assert(ll_gui_destroy(gui) == LL_GUI_OK);
        puts("PASS main-thread and closed-window boundaries");
    }
    puts("AppKit native bridge: 4 groups passed.");
    return 0;
}
