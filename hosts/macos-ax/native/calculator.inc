// Included once by bridge.m; shares its bounded readers and ARC CF ownership.
#include "calculator.h"

@interface LLCalculatorContext : LLAXContext
@property(nonatomic, strong) LLCF *mode;
@property(nonatomic, strong) LLCF *displayArea;
@property(nonatomic, strong) LLCF *display;
@property(nonatomic, copy) NSArray<LLCF *> *buttons;
@property(nonatomic, copy) NSArray<NSNumber *> *buttonIdentities;
@end
@implementation LLCalculatorContext
@end

@interface LLCalculatorSelection : NSObject
@property(nonatomic, strong) LLCF *window;
@property(nonatomic, strong) LLCF *mode;
@property(nonatomic, strong) LLCF *displayArea;
@property(nonatomic, strong) LLCF *display;
@property(nonatomic, strong) LLCF *lastExpression;
@property(nonatomic, copy) NSArray<LLCF *> *buttons;
@end
@implementation LLCalculatorSelection
@end

static int optionalString(LLCF *item, CFStringRef name, NSString **out, int32_t *detail) {
    int result = stringAttribute(item, name, out, detail);
    if (result == LL_AX_API && detail &&
        (*detail == kAXErrorNoValue || *detail == kAXErrorAttributeUnsupported)) {
        *out = @"";
        return LL_AX_OK;
    }
    return result;
}
static BOOL sameElement(LLCF *left, LLCF *right) {
    return left && right && left->value && right->value && CFEqual(left->value, right->value);
}
static int pressActionNames(CFTypeRef raw, BOOL *supported) {
    if (!raw || CFGetTypeID(raw) != CFArrayGetTypeID()) return LL_AX_INVALID;
    CFArrayRef actions = (CFArrayRef)raw;
    CFIndex count = CFArrayGetCount(actions);
    if (count > 16) return LL_AX_BOUNDS;
    BOOL found = NO;
    for (CFIndex i = 0; i < count; ++i) {
        CFTypeRef action = CFArrayGetValueAtIndex(actions, i);
        if (!action || CFGetTypeID(action) != CFStringGetTypeID()) return LL_AX_INVALID;
        int result = utf8((__bridge NSString *)action, NULL);
        if (result) return result;
        if (CFEqual(action, kAXPressAction)) {
            if (found) return LL_AX_INVALID;
            found = YES;
        }
    }
    *supported = found;
    return LL_AX_OK;
}
static int buttonEnabled(LLCF *button, BOOL *out, int32_t *detail) {
    LLCF *enabled = nil;
    int result = attribute(button, kAXEnabledAttribute, &enabled, detail);
    if (result) return result;
    if (CFGetTypeID(enabled->value) != CFBooleanGetTypeID()) return LL_AX_INVALID;
    LLCF *actions = [LLCF new];
    CFArrayRef names = NULL;
    result = api(AXUIElementCopyActionNames((AXUIElementRef)button->value, &names), detail);
    actions->value = names;
    if (result) return result;
    BOOL pressable = NO;
    result = pressActionNames(actions->value, &pressable);
    if (result) return result;
    *out = CFBooleanGetValue((CFBooleanRef)enabled->value) && pressable;
    return LL_AX_OK;
}
static int calculatorSelection(LLCalculatorContext *context, LLCalculatorSelection **out,
                                int32_t *detail) {
    int result = permission(context, detail);
    if (result) return result;
    NSMutableArray<LLCF *> *windows = nil;
    result = children(context.application, kAXWindowsAttribute, context.pid, &windows, detail);
    if (result) return result;
    LLCF *window = nil;
    for (LLCF *candidate in windows) {
        NSString *identifier = nil, *role = nil;
        result = optionalString(candidate, kAXIdentifierAttribute, &identifier, detail);
        if (result) return result;
        result = stringAttribute(candidate, kAXRoleAttribute, &role, detail);
        if (result) return result;
        if ([identifier isEqualToString:@"main"] && [role isEqualToString:(__bridge NSString *)kAXWindowRole]) {
            if (window) return LL_AX_INVALID;
            window = candidate;
        }
    }
    if (!window) return LL_AX_UNAVAILABLE;
    NSMutableArray<LLCF *> *queue = [NSMutableArray arrayWithObject:window];
    NSMutableArray<NSNumber *> *depths = [NSMutableArray arrayWithObject:@0];
    NSMutableArray<NSNumber *> *insideDisplay = [NSMutableArray arrayWithObject:@NO];
    NSMutableArray<NSNumber *> *insideResult = [NSMutableArray arrayWithObject:@NO];
    LLCF *mode = nil, *displayArea = nil, *display = nil, *resultArea = nil, *lastExpression = nil;
    NSMutableArray *buttons = [NSMutableArray arrayWithObjects:[NSNull null], [NSNull null], [NSNull null], [NSNull null], nil];
    for (NSUInteger index = 0; index < queue.count; ++index) {
        LLCF *candidate = queue[index];
        NSUInteger depth = depths[index].unsignedIntegerValue;
        BOOL inDisplay = insideDisplay[index].boolValue;
        BOOL inResult = insideResult[index].boolValue;
        NSString *role = nil, *identifier = nil;
        result = stringAttribute(candidate, kAXRoleAttribute, &role, detail);
        if (result) return result;
        result = optionalString(candidate, kAXIdentifierAttribute, &identifier, detail);
        if (result) return result;
        if ([role isEqualToString:(__bridge NSString *)kAXButtonRole]) {
            if ([identifier hasPrefix:@"Mode:"]) {
                // This is an explicit version/layout profile, not a guess at
                // scientific mode or conversion controls with similar labels.
                if (![identifier isEqualToString:@"Mode: basic; unitConversion: false"] || mode) return LL_AX_INVALID;
                mode = candidate;
            }
            NSInteger slot = -1;
            if ([identifier isEqualToString:@"One"]) slot = 0;
            else if ([identifier isEqualToString:@"Two"]) slot = 2;
            else {
                NSString *title = nil, *description = nil;
                result = optionalString(candidate, kAXTitleAttribute, &title, detail);
                if (result) return result;
                result = optionalString(candidate, kAXDescriptionAttribute, &description, detail);
                if (result) return result;
                BOOL add = [title isEqualToString:@"Add"] || [description isEqualToString:@"Add"];
                BOOL equals = [title isEqualToString:@"Equals"] || [description isEqualToString:@"Equals"];
                if (add && equals) return LL_AX_INVALID;
                if (add) slot = 1;
                else if (equals) slot = 3;
            }
            if (slot >= 0) {
                if (buttons[(NSUInteger)slot] != [NSNull null]) return LL_AX_INVALID;
                buttons[(NSUInteger)slot] = candidate;
            }
        }
        if ([role isEqualToString:(__bridge NSString *)kAXScrollAreaRole] &&
            [identifier hasPrefix:@"StandardInputView;value:"]) {
            if (displayArea || inResult) return LL_AX_INVALID;
            displayArea = candidate;
            inDisplay = YES;
        } else if ([role isEqualToString:(__bridge NSString *)kAXScrollAreaRole] &&
                   [identifier isEqualToString:@"StandardResultView"]) {
            if (resultArea || inDisplay) return LL_AX_INVALID;
            resultArea = candidate;
            inResult = YES;
        } else if (inDisplay && [role isEqualToString:(__bridge NSString *)kAXStaticTextRole]) {
            if (display) return LL_AX_INVALID;
            display = candidate;
        } else if (inResult && [role isEqualToString:(__bridge NSString *)kAXStaticTextRole]) {
            if (lastExpression) return LL_AX_INVALID;
            lastExpression = candidate;
        }
        NSMutableArray<LLCF *> *descendants = nil;
        result = children(candidate, kAXChildrenAttribute, context.pid, &descendants, detail);
        if (result) return result;
        if ((depth >= LL_DEPTH_LIMIT && descendants.count) || queue.count + descendants.count > LL_NODE_LIMIT) return LL_AX_BOUNDS;
        for (LLCF *child in descendants) {
            for (LLCF *seen in queue) if (sameElement(seen, child)) return LL_AX_INVALID;
            [queue addObject:child];
            [depths addObject:@(depth + 1)];
            [insideDisplay addObject:@(inDisplay)];
            [insideResult addObject:@(inResult)];
        }
    }
    if (!mode || !displayArea || !display || [buttons containsObject:[NSNull null]]) return LL_AX_UNAVAILABLE;
    // The result area appears after Equals. Its content affects subsequent
    // operations even when the current display is unchanged. Once present it
    // must contain exactly one readable expression; never treat read failure
    // as absence. It is observed anew, not retained as an operation target.
    if (resultArea && !lastExpression) return LL_AX_INVALID;
    if (context.window && (!sameElement(context.window, window) || !sameElement(context.mode, mode) ||
        !sameElement(context.displayArea, displayArea) || !sameElement(context.display, display))) return LL_AX_STALE;
    if (context.buttons) {
        if (context.buttons.count != LL_CALC_BUTTONS) return LL_AX_INVALID;
        for (NSUInteger i = 0; i < LL_CALC_BUTTONS; ++i)
            if (!sameElement(context.buttons[i], buttons[i])) return LL_AX_STALE;
    }
    LLCalculatorSelection *selection = [LLCalculatorSelection new];
    selection.window = window; selection.mode = mode; selection.displayArea = displayArea;
    selection.display = display; selection.buttons = buttons;
    selection.lastExpression = lastExpression;
    *out = selection;
    return LL_AX_OK;
}
static int calculatorObserve(LLCalculatorContext *context, NSString **displayOut, NSString **lastExpressionOut,
                              LLCalcSnapshot *out, int32_t *detail) {
    LLCalculatorSelection *selection = nil;
    int result = calculatorSelection(context, &selection, detail);
    if (result) return result;
    NSString *display = nil;
    result = stringAttribute(selection.display, kAXValueAttribute, &display, detail);
    if (result) return result;
    NSString *lastExpression = nil;
    if (selection.lastExpression) {
        result = stringAttribute(selection.lastExpression, kAXValueAttribute, &lastExpression, detail);
        if (result) return result;
    }
    LLCalcSnapshot snapshot = {0};
    snapshot.identity = context.identity;
    snapshot.last_expression_present = lastExpression ? 1 : 0;
    for (NSUInteger i = 0; i < LL_CALC_BUTTONS; ++i) {
        BOOL enabled = NO;
        result = buttonEnabled(selection.buttons[i], &enabled, detail);
        if (result) return result;
        snapshot.button_enabled[i] = enabled ? 1 : 0;
        snapshot.button_identities[i] = context.buttonIdentities[i].unsignedLongLongValue;
    }
    result = permission(context, detail);
    if (result) return result;
    *displayOut = display;
    *lastExpressionOut = lastExpression;
    *out = snapshot;
    return LL_AX_OK;
}
static int calculatorExpected(LLCalculatorContext *context, const LLCalcSnapshot *expected,
                               NSString *expectedDisplay, NSString *currentDisplay,
                               NSString *expectedLastExpression, NSString *currentLastExpression,
                               const LLCalcSnapshot *current, uint32_t button) {
    if (!expected || button >= LL_CALC_BUTTONS || !expected->identity ||
        expected->identity != context.identity || expected->identity != current->identity) return LL_AX_STALE;
    for (NSUInteger i = 0; i < LL_CALC_BUTTONS; ++i) {
        if (!expected->button_identities[i] || expected->button_identities[i] != current->button_identities[i] ||
            (expected->button_enabled[i] != 0 && expected->button_enabled[i] != 1) ||
            expected->button_enabled[i] != current->button_enabled[i]) return LL_AX_STALE;
    }
    if (![expectedDisplay isEqualToString:currentDisplay]) return LL_AX_STALE;
    if ((expected->last_expression_present != 0 && expected->last_expression_present != 1) ||
        (!expected->last_expression_present && expected->last_expression_length != 0)) return LL_AX_INVALID;
    if (expected->last_expression_present != current->last_expression_present ||
        (expected->last_expression_present && ![expectedLastExpression isEqualToString:currentLastExpression])) return LL_AX_STALE;
    if (!current->button_enabled[button]) return LL_AX_NOT_EDITABLE;
    return LL_AX_OK;
}

int32_t ll_calc_connect(void **out, int32_t *detail) {
    @autoreleasepool { @try {
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        if (!out || !detail) return LL_AX_INVALID;
        *out = NULL; *detail = 0;
        if (!AXIsProcessTrusted()) return LL_AX_PERMISSION;
        LLCalculatorContext *context = [LLCalculatorContext new];
        int result = discoverApp(context, @"/System/Applications/Calculator.app", @"com.apple.calculator", detail);
        if (result) return result;
        context.application = [LLCF new];
        context.application->value = AXUIElementCreateApplication(context.pid);
        LLCalculatorSelection *selection = nil;
        result = calculatorSelection(context, &selection, detail);
        if (result) return result;
        if (nextIdentity > UINT64_MAX - (LL_CALC_BUTTONS + 1)) return LL_AX_BOUNDS;
        context.identity = nextIdentity++;
        NSMutableArray<NSNumber *> *identities = [NSMutableArray new];
        for (NSUInteger i = 0; i < LL_CALC_BUTTONS; ++i) [identities addObject:@(nextIdentity++)];
        context.buttonIdentities = identities;
        context.window = selection.window; context.mode = selection.mode;
        context.displayArea = selection.displayArea; context.display = selection.display;
        context.buttons = selection.buttons;
        NSString *display = nil, *lastExpression = nil;
        LLCalcSnapshot snapshot = {0};
        result = calculatorObserve(context, &display, &lastExpression, &snapshot, detail);
        if (result) return result;
        *out = (__bridge_retained void *)context;
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_calc_snapshot(void *raw, uint8_t *bytes, size_t capacity,
                         LLCalcSnapshot *out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !bytes || !out || !detail) return LL_AX_INVALID;
        *detail = 0;
        LLCalculatorContext *context = (__bridge LLCalculatorContext *)raw;
        NSString *display = nil, *lastExpression = nil;
        LLCalcSnapshot snapshot = {0};
        int result = calculatorObserve(context, &display, &lastExpression, &snapshot, detail);
        if (result) return result;
        NSData *data = nil;
        result = utf8(display, &data);
        if (result) return result;
        NSData *expressionBytes = nil;
        if (lastExpression) {
            result = utf8(lastExpression, &expressionBytes);
            if (result) return result;
        }
        if (data.length + expressionBytes.length > capacity) return LL_AX_BOUNDS;
        snapshot.display_length = data.length;
        snapshot.last_expression_length = expressionBytes.length;
        if (data.length) memcpy(bytes, data.bytes, data.length);
        if (expressionBytes.length) memcpy(bytes + data.length, expressionBytes.bytes, expressionBytes.length);
        *out = snapshot;
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return LL_AX_API; } }
}
int32_t ll_calc_press(void *raw, const LLCalcSnapshot *expected,
                      const uint8_t *display, size_t display_length,
                      const uint8_t *last_expression, size_t last_expression_length,
                      uint32_t button, int32_t *detail) {
    BOOL entered = NO;
    @autoreleasepool { @try {
        if (!raw || !expected || !detail || button >= LL_CALC_BUTTONS) return LL_AX_INVALID;
        *detail = 0;
        LLCalculatorContext *context = (__bridge LLCalculatorContext *)raw;
        int result = permission(context, detail);
        if (result) return result;
        if (expected->display_length != display_length || expected->last_expression_length != last_expression_length ||
            (expected->last_expression_present != 0 && expected->last_expression_present != 1) ||
            (!expected->last_expression_present && last_expression_length != 0)) return LL_AX_INVALID;
        NSString *wanted = nil, *wantedExpression = nil;
        result = decode(display, display_length, &wanted);
        if (result) return result;
        if (expected->last_expression_present) {
            result = decode(last_expression, last_expression_length, &wantedExpression);
            if (result) return result;
        }
        NSString *currentDisplay = nil, *currentExpression = nil;
        LLCalcSnapshot current = {0};
        result = calculatorObserve(context, &currentDisplay, &currentExpression, &current, detail);
        if (result) return result;
        result = calculatorExpected(context, expected, wanted, currentDisplay, wantedExpression, currentExpression, &current, button);
        if (result) return result;
        // AX offers no atomic compare-and-press. Any failure once entered can
        // coexist with a completed keypress; never replay automatically.
        entered = YES;
        AXError error = AXUIElementPerformAction((AXUIElementRef)context.buttons[button]->value, kAXPressAction);
        if (error != kAXErrorSuccess) { *detail = (int32_t)error; return LL_AX_UNCERTAIN; }
        return LL_AX_OK;
    } @catch (NSException *exception) { (void)exception; if (detail) *detail = -1; return entered ? LL_AX_UNCERTAIN : LL_AX_API; } }
}
