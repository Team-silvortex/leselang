#import <AppKit/AppKit.h>
#include <limits.h>
#include <string.h>
#include "bridge.h"

@interface LlGuiButton : NSButton
@property(nonatomic) uintptr_t instance;
@property(nonatomic) uint64_t clicks;
@property(nonatomic) uint64_t mutation;
@property(nonatomic, copy) NSString *savedValue;
@end
@implementation LlGuiButton
@end

@interface LlGuiTextField : NSTextField
@property(nonatomic) uintptr_t instance;
@property(nonatomic) uint64_t mutation;
@end
@implementation LlGuiTextField
@end

@interface LlGuiContext : NSObject <NSWindowDelegate, NSTextFieldDelegate>
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic, strong) NSArray<LlGuiButton *> *buttons;
@property(nonatomic, strong) LlGuiTextField *input;
@property(nonatomic, strong) NSArray<NSTextField *> *counts;
@property(nonatomic, strong) NSTextField *saved;
@property(nonatomic, strong) NSTextField *status;
@property(nonatomic) uintptr_t lastInstance;
@property(nonatomic) uint64_t editNotifications;
@property(nonatomic) BOOL reservedTextMutation;
@property(nonatomic) BOOL trackingFailed;
@property(nonatomic) BOOL closed;
@property(nonatomic) int32_t actionStatus;
- (void)clicked:(LlGuiButton *)sender;
- (LlGuiButton *)newButton:(NSInteger)slot frame:(NSRect)frame;
- (LlGuiTextField *)newInput:(NSRect)frame;
- (BOOL)reserveInputChange;
@end

static NSTextField *label(NSString *text, NSRect frame, CGFloat size) {
    NSTextField *result = [NSTextField labelWithString:text];
    result.frame = frame;
    result.font = [NSFont systemFontOfSize:size];
    return result;
}

static NSString *inputValue(LlGuiTextField *input) {
    NSText *editor = input.currentEditor;
    return editor ? editor.string : input.stringValue;
}

@implementation LlGuiContext
- (LlGuiButton *)newButton:(NSInteger)slot frame:(NSRect)frame {
    if (self.lastInstance == UINTPTR_MAX) return nil;
    LlGuiButton *button = [[LlGuiButton alloc] initWithFrame:frame];
    button.instance = self.lastInstance + 1;
    self.lastInstance = button.instance;
    button.identifier = slot == 0 ? @"save" : @"other";
    button.accessibilityIdentifier = button.identifier;
    button.title = @"draft";
    button.savedValue = @"";
    button.bezelStyle = NSBezelStyleRounded;
    button.buttonType = NSButtonTypeMomentaryPushIn;
    button.target = self;
    button.action = @selector(clicked:);
    button.font = [NSFont systemFontOfSize:17 weight:NSFontWeightMedium];
    return button;
}
- (LlGuiTextField *)newInput:(NSRect)frame {
    if (self.lastInstance == UINTPTR_MAX) return nil;
    LlGuiTextField *input = [[LlGuiTextField alloc] initWithFrame:frame];
    input.instance = self.lastInstance + 1;
    self.lastInstance = input.instance;
    input.identifier = @"input";
    input.accessibilityIdentifier = input.identifier;
    input.placeholderString = @"Message to save";
    input.stringValue = @"";
    input.editable = YES;
    input.selectable = YES;
    input.delegate = self;
    input.font = [NSFont systemFontOfSize:17];
    return input;
}
- (BOOL)reserveInputChange {
    // Save consumes input text. Its observation must become stale when that
    // dependency changes, even if a later edit restores the same value.
    if (self.input.mutation == UINT64_MAX || self.buttons[0].mutation == UINT64_MAX)
        return NO;
    self.input.mutation += 1;
    self.buttons[0].mutation += 1;
    return YES;
}
- (void)controlTextDidChange:(NSNotification *)notification {
    if (notification.object != self.input || self.closed) return;
    if (self.editNotifications == UINT64_MAX ||
        (!self.reservedTextMutation && ![self reserveInputChange])) {
        // An external edit already happened. Refuse later snapshots and actions
        // rather than pretending its old revision remains current.
        self.trackingFailed = YES;
        self.actionStatus = LL_GUI_ERROR;
        return;
    }
    self.editNotifications += 1;
}
- (void)clicked:(LlGuiButton *)sender {
    NSUInteger slot = [self.buttons indexOfObjectIdenticalTo:sender];
    if (self.closed || self.trackingFailed || slot == NSNotFound || !sender.enabled ||
        sender.clicks == UINT64_MAX || sender.mutation == UINT64_MAX) {
        self.actionStatus = LL_GUI_ERROR;
        return;
    }
    NSString *value = slot == 0 ? inputValue(self.input) : @"";
    if ([value lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > LL_GUI_MAX_TEXT_BYTES) {
        self.actionStatus = LL_GUI_TEXT_BOUNDS;
        return;
    }
    sender.mutation += 1;
    sender.clicks += 1;
    sender.title = @"ready";
    if (slot == 0) {
        sender.savedValue = value;
        self.saved.stringValue = [@"Saved value: " stringByAppendingString:value];
    }
    self.counts[slot].stringValue =
        [NSString stringWithFormat:@"Target/action calls: %llu", (unsigned long long)sender.clicks];
    self.status.stringValue =
        [NSString stringWithFormat:@"%@ received its AppKit target/action.\nObserve the control again to verify the result.", sender.identifier];
}
- (void)windowWillClose:(NSNotification *)notification {
    if (notification.object == self.window) self.closed = YES;
}
@end

static int32_t context(LlGui *gui, LlGuiContext **out, BOOL permitClosed) {
    if (![NSThread isMainThread]) return LL_GUI_WRONG_THREAD;
    if (!gui) return LL_GUI_BAD_INPUT;
    LlGuiContext *value = (__bridge LlGuiContext *)gui;
    if (!permitClosed && value.closed) return LL_GUI_CLOSED;
    if (!permitClosed && value.trackingFailed) return LL_GUI_ERROR;
    *out = value;
    return LL_GUI_OK;
}

static int32_t widget(LlGuiContext *gui, int32_t slot, NSControl **out) {
    if (slot < 0 || slot >= LL_GUI_WIDGET_COUNT) return LL_GUI_BAD_INPUT;
    NSControl *control = slot == 2 ? (NSControl *)gui.input : gui.buttons[(NSUInteger)slot];
    if (![control isDescendantOf:gui.window.contentView]) return LL_GUI_NOT_FOUND;
    *out = control;
    return LL_GUI_OK;
}
static uintptr_t instance(NSControl *control) {
    return [control isKindOfClass:LlGuiButton.class] ? ((LlGuiButton *)control).instance : ((LlGuiTextField *)control).instance;
}
static int32_t reserve(LlGuiContext *gui, int32_t slot) {
    if (slot == 2) return [gui reserveInputChange] ? LL_GUI_OK : LL_GUI_ERROR;
    LlGuiButton *button = gui.buttons[(NSUInteger)slot];
    if (button.mutation == UINT64_MAX) return LL_GUI_ERROR;
    button.mutation += 1;
    return LL_GUI_OK;
}
static int32_t text(const uint8_t *bytes, size_t length, NSString **out) {
    if (length > LL_GUI_MAX_TEXT_BYTES) return LL_GUI_TEXT_BOUNDS;
    if (length && !bytes) return LL_GUI_BAD_INPUT;
    NSString *value = [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
    if (!value) return LL_GUI_INVALID_UTF8;
    *out = value;
    return LL_GUI_OK;
}

// Full physical traversal precedes publishing metadata. All returned rows must
// be unique registered controls; unknown labels/cells remain implementation UI.
static int32_t collect(LlGuiContext *gui, NSView *view, size_t depth, size_t *visited,
                       NSMutableArray<NSControl *> *controls, NSMutableSet<NSString *> *names) {
    if (depth > 16 || *visited >= 64) return LL_GUI_TREE_BOUNDS;
    *visited += 1;
    if ([view isKindOfClass:LlGuiButton.class] || [view isKindOfClass:LlGuiTextField.class]) {
        NSControl *control = (NSControl *)view;
        if (control != gui.input && [gui.buttons indexOfObjectIdenticalTo:(LlGuiButton *)control] == NSNotFound)
            return LL_GUI_NOT_FOUND;
        NSString *name = control.identifier;
        size_t length = [name lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (!length || length > LL_GUI_MAX_TEXT_BYTES) return LL_GUI_BAD_INPUT;
        NSData *utf8 = [name dataUsingEncoding:NSUTF8StringEncoding];
        if (!utf8 || memchr(utf8.bytes, 0, utf8.length)) return LL_GUI_BAD_INPUT;
        if ([names containsObject:name]) return LL_GUI_AMBIGUOUS;
        if (controls.count >= LL_GUI_WIDGET_COUNT) return LL_GUI_TREE_BOUNDS;
        [names addObject:name];
        [controls addObject:control];
    }
    for (NSView *child in view.subviews) {
        int32_t code = collect(gui, child, depth + 1, visited, controls, names);
        if (code != LL_GUI_OK) return code;
    }
    return LL_GUI_OK;
}
static int32_t registered(LlGuiContext *gui, NSMutableArray<NSControl *> **out) {
    size_t visited = 0;
    NSMutableArray<NSControl *> *controls = [NSMutableArray arrayWithCapacity:LL_GUI_WIDGET_COUNT];
    int32_t code = collect(gui, gui.window.contentView, 0, &visited, controls, [NSMutableSet set]);
    if (code != LL_GUI_OK) return code;
    *out = controls;
    return LL_GUI_OK;
}

#define LL_BEGIN @autoreleasepool { @try {
#define LL_END } @catch (NSException *exception) { (void)exception; return LL_GUI_ERROR; } }
#define LL_CONTEXT(allow_closed) \
    LlGuiContext *app = nil; \
    int32_t code = context(gui, &app, (allow_closed)); \
    if (code != LL_GUI_OK) return code
#define LL_WIDGET() \
    NSControl *control = nil; \
    code = widget(app, slot, &control); \
    if (code != LL_GUI_OK) return code

int32_t ll_gui_create(LlGui **out) {
    LL_BEGIN
    if (![NSThread isMainThread]) return LL_GUI_WRONG_THREAD;
    if (!out) return LL_GUI_BAD_INPUT;
    *out = NULL;
    static BOOL launched = NO;
    NSApplication *application = [NSApplication sharedApplication];
    if (!launched) {
        [application setActivationPolicy:NSApplicationActivationPolicyRegular];
        [application finishLaunching];
        launched = YES;
    }
    LlGuiContext *app = [[LlGuiContext alloc] init];
    app.window = [[NSWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, 720, 570)
                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable
                    backing:NSBackingStoreBuffered defer:NO];
    app.window.title = @"LeseLang · AppKit Host";
    app.window.releasedWhenClosed = NO;
    app.window.delegate = app;
    [app.window center];
    NSView *content = app.window.contentView;
    [content addSubview:label(@"LESELANG / NATIVE GUI", NSMakeRect(36, 518, 648, 24), 12)];
    [content addSubview:label(@"Discover → Edit → Submit → Verify", NSMakeRect(36, 474, 648, 34), 25)];
    NSTextField *description = label(@"Real AppKit controls. Source selects and edits a field, then submits it.", NSMakeRect(36, 438, 648, 24), 13);
    description.textColor = NSColor.secondaryLabelColor;
    [content addSubview:description];
    [content addSubview:label(@"Identifier: input", NSMakeRect(36, 400, 648, 24), 15)];
    app.input = [app newInput:NSMakeRect(36, 355, 648, 34)];
    if (!app.input) return LL_GUI_ERROR;
    [content addSubview:app.input];
    NSMutableArray<LlGuiButton *> *buttons = [NSMutableArray arrayWithCapacity:2];
    NSMutableArray<NSTextField *> *counts = [NSMutableArray arrayWithCapacity:2];
    for (NSInteger slot = 0; slot < 2; slot++) {
        CGFloat x = slot == 0 ? 36 : 384;
        [content addSubview:label(slot == 0 ? @"Identifier: save" : @"Identifier: other", NSMakeRect(x, 310, 280, 24), 15)];
        LlGuiButton *button = [app newButton:slot frame:NSMakeRect(x, 251, 280, 48)];
        if (!button) return LL_GUI_ERROR;
        [buttons addObject:button];
        [content addSubview:button];
        NSTextField *count = label(@"Target/action calls: 0", NSMakeRect(x, 221, 280, 24), 13);
        [counts addObject:count];
        [content addSubview:count];
    }
    app.buttons = buttons;
    app.counts = counts;
    app.saved = label(@"Saved value: ", NSMakeRect(36, 184, 648, 24), 14);
    app.saved.accessibilityIdentifier = @"saved-value";
    [content addSubview:app.saved];
    app.status = [NSTextField wrappingLabelWithString:@"Ready for an explicit LeseLang host session."];
    app.status.frame = NSMakeRect(36, 30, 648, 130);
    app.status.font = [NSFont monospacedSystemFontOfSize:14 weight:NSFontWeightRegular];
    app.status.accessibilityIdentifier = @"protocol-status";
    [content addSubview:app.status];
    *out = (LlGui *)(__bridge_retained void *)app;
    return LL_GUI_OK;
    LL_END
}

int32_t ll_gui_destroy(LlGui *gui) {
    LL_BEGIN
    LL_CONTEXT(YES);
    LlGuiContext *owned = (__bridge_transfer LlGuiContext *)gui;
    owned.window.delegate = nil;
    owned.input.delegate = nil;
    owned.closed = YES;
    [owned.window close];
    (void)app;
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_show(LlGui *gui) {
    LL_BEGIN
    LL_CONTEXT(NO);
    [app.window makeKeyAndOrderFront:nil];
    [NSApp activate];
    [app.window displayIfNeeded];
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_close(LlGui *gui) {
    LL_BEGIN
    LL_CONTEXT(YES);
    if (!app.closed) { app.closed = YES; [app.window close]; }
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_is_open(LlGui *gui, int32_t *open) {
    LL_BEGIN
    LL_CONTEXT(YES);
    if (!open) return LL_GUI_BAD_INPUT;
    *open = app.closed ? 0 : 1;
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_poll(LlGui *gui, int32_t *dispatched) {
    LL_BEGIN
    LL_CONTEXT(YES);
    if (!dispatched) return LL_GUI_BAD_INPUT;
    *dispatched = 0;
    if (app.closed) return LL_GUI_CLOSED;
    NSEvent *event = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:[NSDate distantPast]
                                          inMode:NSDefaultRunLoopMode dequeue:YES];
    app.actionStatus = LL_GUI_OK;
    if (event) { [NSApp sendEvent:event]; *dispatched = 1; }
    [NSApp updateWindows];
    return app.trackingFailed ? LL_GUI_ERROR : app.actionStatus;
    LL_END
}

int32_t ll_gui_list(LlGui *gui, uint8_t *buffer, size_t capacity, size_t *length, size_t *count) {
    LL_BEGIN
    LL_CONTEXT(NO);
    if (!length || !count || (capacity && !buffer)) return LL_GUI_BAD_INPUT;
    NSMutableArray<NSControl *> *controls = nil;
    code = registered(app, &controls);
    if (code != LL_GUI_OK) return code;
    NSMutableData *packed = [NSMutableData dataWithCapacity:64];
    uint8_t delimiter = 0;
    for (NSControl *control in controls) {
        NSData *bytes = [control.identifier dataUsingEncoding:NSUTF8StringEncoding];
        if (bytes.length + 1 > LL_GUI_MAX_TEXT_BYTES - packed.length) return LL_GUI_TEXT_BOUNDS;
        [packed appendData:bytes];
        [packed appendBytes:&delimiter length:1];
    }
    if (packed.length > capacity) return LL_GUI_TEXT_BOUNDS;
    if (packed.length) memcpy(buffer, packed.bytes, packed.length);
    *length = packed.length;
    *count = controls.count;
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_find(LlGui *gui, const uint8_t *name, size_t length, int32_t *slot, uintptr_t *identity) {
    LL_BEGIN
    LL_CONTEXT(NO);
    if (!slot || !identity) return LL_GUI_BAD_INPUT;
    NSString *identifier = nil;
    code = text(name, length, &identifier);
    if (code != LL_GUI_OK) return code;
    NSMutableArray<NSControl *> *controls = nil;
    code = registered(app, &controls);
    if (code != LL_GUI_OK) return code;
    for (NSControl *control in controls) {
        if (![control.identifier isEqualToString:identifier]) continue;
        *slot = control == app.input ? 2 : (int32_t)[app.buttons indexOfObjectIdenticalTo:(LlGuiButton *)control];
        *identity = instance(control);
        return LL_GUI_OK;
    }
    return LL_GUI_NOT_FOUND;
    LL_END
}
int32_t ll_gui_snapshot(LlGui *gui, int32_t slot, uint8_t *buffer, size_t capacity, LlGuiSnapshot *snapshot) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    if (!snapshot || (capacity && !buffer)) return LL_GUI_BAD_INPUT;
    BOOL isInput = slot == 2;
    LlGuiButton *button = isInput ? nil : (LlGuiButton *)control;
    NSString *title = isInput ? app.input.placeholderString ?: @"" : button.title;
    NSString *value = isInput ? inputValue(app.input) : button.savedValue;
    NSArray<NSString *> *strings = @[ control.identifier, title, value ];
    NSData *encoded[3];
    size_t lengths[3];
    size_t total = 0;
    for (NSUInteger index = 0; index < 3; index++) {
        // UTF-8 sizing precedes allocation of a byte copy.
        if ([strings[index] lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > LL_GUI_MAX_TEXT_BYTES)
            return LL_GUI_TEXT_BOUNDS;
        encoded[index] = [strings[index] dataUsingEncoding:NSUTF8StringEncoding];
        if (!encoded[index]) return LL_GUI_INVALID_UTF8;
        lengths[index] = encoded[index].length;
        total += lengths[index];
    }
    if (total > capacity) return LL_GUI_TEXT_BOUNDS;
    size_t offset = 0;
    for (NSUInteger index = 0; index < 3; index++) {
        if (lengths[index]) memcpy(buffer + offset, encoded[index].bytes, lengths[index]);
        offset += lengths[index];
    }
    *snapshot = (LlGuiSnapshot){ instance(control), isInput ? 0 : button.clicks,
        isInput ? app.input.mutation : button.mutation, lengths[0], lengths[1], lengths[2],
        isInput ? 2 : 1, control.enabled ? 1 : 0, isInput && app.input.editable ? 1 : 0 };
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_click(LlGui *gui, int32_t slot, uintptr_t expected_identity) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    if (slot == 2) return LL_GUI_BAD_INPUT;
    LlGuiButton *button = (LlGuiButton *)control;
    if (button.instance != expected_identity) return LL_GUI_STALE_IDENTITY;
    if (!button.enabled) return LL_GUI_BAD_INPUT;
    if (button.clicks == UINT64_MAX || button.mutation == UINT64_MAX) return LL_GUI_ERROR;
    uint64_t before = button.clicks;
    app.actionStatus = LL_GUI_OK;
    [button performClick:nil];
    [app.window displayIfNeeded];
    if (app.actionStatus != LL_GUI_OK) return app.actionStatus;
    return button.clicks == before + 1 ? LL_GUI_OK : LL_GUI_ERROR;
    LL_END
}
int32_t ll_gui_set_text(LlGui *gui, int32_t slot, uintptr_t expected_identity, const uint8_t *bytes, size_t length) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    if (slot != 2 || !control.enabled || !app.input.editable) return LL_GUI_BAD_INPUT;
    if (app.input.instance != expected_identity) return LL_GUI_STALE_IDENTITY;
    NSString *value = nil;
    code = text(bytes, length, &value);
    if (code != LL_GUI_OK) return code;
    if (app.editNotifications == UINT64_MAX || ![app reserveInputChange]) return LL_GUI_ERROR;
    uint64_t before = app.editNotifications;
    app.reservedTextMutation = YES;
    @try {
        NSText *editor = app.input.currentEditor;
        if (editor) editor.string = value;
        app.input.stringValue = value;
        // AppKit registers an NSTextField delegate for this notification. Use
        // the same delivery path as field-editor changes, not a model callback.
        NSDictionary *info = editor ? @{ @"NSFieldEditor": editor } : @{};
        [NSNotificationCenter.defaultCenter postNotificationName:NSControlTextDidChangeNotification
                                                          object:app.input userInfo:info];
    } @finally { app.reservedTextMutation = NO; }
    [app.window displayIfNeeded];
    return !app.trackingFailed && app.editNotifications == before + 1 ? LL_GUI_OK : LL_GUI_ERROR;
    LL_END
}
int32_t ll_gui_replace(LlGui *gui, int32_t slot) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    if (slot == 2) {
        if (![app reserveInputChange]) return LL_GUI_ERROR;
        LlGuiTextField *replacement = [app newInput:control.frame];
        if (!replacement) return LL_GUI_ERROR;
        app.input.delegate = nil;
        [app.window makeFirstResponder:nil];
        [app.window.contentView addSubview:replacement];
        [control removeFromSuperview];
        app.input = replacement;
    } else {
        LlGuiButton *replacement = [app newButton:slot frame:control.frame];
        if (!replacement) return LL_GUI_ERROR;
        [app.window.contentView addSubview:replacement];
        [control removeFromSuperview];
        NSMutableArray<LlGuiButton *> *buttons = [app.buttons mutableCopy];
        buttons[(NSUInteger)slot] = replacement;
        app.buttons = buttons;
        app.counts[(NSUInteger)slot].stringValue = @"Target/action calls: 0";
        if (slot == 0) app.saved.stringValue = @"Saved value: ";
    }
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_set_title(LlGui *gui, int32_t slot, const uint8_t *bytes, size_t length) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    NSString *value = nil;
    code = text(bytes, length, &value);
    if (code != LL_GUI_OK) return code;
    code = reserve(app, slot);
    if (code != LL_GUI_OK) return code;
    if (slot == 2) app.input.placeholderString = value;
    else ((LlGuiButton *)control).title = value;
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_set_enabled(LlGui *gui, int32_t slot, int32_t enabled) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    if (enabled != 0 && enabled != 1) return LL_GUI_BAD_INPUT;
    code = reserve(app, slot);
    if (code != LL_GUI_OK) return code;
    control.enabled = enabled != 0;
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_set_editable(LlGui *gui, int32_t slot, int32_t editable) {
    LL_BEGIN
    LL_CONTEXT(NO);
    LL_WIDGET();
    if (slot != 2 || (editable != 0 && editable != 1)) return LL_GUI_BAD_INPUT;
    code = reserve(app, slot);
    if (code != LL_GUI_OK) return code;
    app.input.editable = editable != 0;
    return LL_GUI_OK;
    LL_END
}
int32_t ll_gui_status(LlGui *gui, const uint8_t *bytes, size_t length) {
    LL_BEGIN
    LL_CONTEXT(NO);
    NSString *value = nil;
    code = text(bytes, length, &value);
    if (code != LL_GUI_OK) return code;
    app.status.stringValue = value;
    [app.window displayIfNeeded];
    return LL_GUI_OK;
    LL_END
}
