// Additive widget transport. Included after scope.inc; older control layouts and
// constructors retain their own role projection, budgets and operation catalog.
#include "widgets.h"

@interface LLWidgetsContext : LLControlsContext
@property(nonatomic, strong) LLAuxiliaryPolicy *auxiliary;
@end
@implementation LLWidgetsContext
@end

static int widgetsPermission(LLWidgetsContext *context, int32_t *detail) {
    // Grant callbacks read Rust Rc<Cell<bool>> storage: never touch them off
    // the main thread, including failure paths and explicit revocation.
    if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
    if (!context) return LL_AX_INVALID;
    int result = auxiliaryCheckGrant(context.auxiliary);
    if (!result) result = controlsPermission(context, detail);
    if (!result) result = auxiliaryCheck(context.auxiliary, detail);
    if (result) context.retired = YES;
    return result;
}
typedef struct {
    __unsafe_unretained LLWidgetsContext *context;
    __unsafe_unretained LLCF *parent;
    BOOL requireParent;
} LLWidgetsElementContext;
static int widgetsElement(LLCF *item, void *opaque, int32_t *detail) {
    LLWidgetsElementContext *ownership = opaque;
    return auxiliaryElement(ownership->context.auxiliary, ownership->context.pid,
        item, ownership->parent, ownership->requireParent, detail);
}
static int widgetsChildren(LLWidgetsContext *context, LLCF *parent,
    NSMutableArray<LLCF *> **out, int32_t *detail) {
    if (!context.auxiliary) return children(parent, kAXChildrenAttribute, context.pid, out, detail);
    LLWidgetsElementContext ownership = {context, parent, YES};
    return childrenChecked(parent, kAXChildrenAttribute, widgetsElement, &ownership, out, detail);
}

static BOOL widgetsRole(NSString *role) {
    return controlsAdmittedRole(role) || [@[@"AXCheckBox", @"AXPopUpButton", @"AXMenuButton",
        @"AXMenu", @"AXMenuItem", @"AXList", @"AXTable", @"AXOutline", @"AXRow", @"AXCell",
        @"AXScrollArea", @"AXScrollBar"] containsObject:role];
}
static BOOL widgetsCollection(NSString *role) {
    return [@[@"AXList", @"AXTable", @"AXOutline"] containsObject:role];
}
static BOOL widgetsSelectionRole(NSString *role, NSString *container) {
    if ([container isEqualToString:@"AXList"]) {
        return [@[@"AXGroup", @"AXRow", @"AXCell", @"AXTextField", @"AXStaticText"] containsObject:role];
    }
    return widgetsCollection(container) && [role isEqualToString:@"AXRow"];
}
static BOOL widgetsMenuOwner(NSString *role) {
    return [role isEqualToString:@"AXPopUpButton"] || [role isEqualToString:@"AXMenuButton"];
}
static int widgetsActions(CFTypeRef raw, NSArray<NSString *> **out) {
    if (!raw || CFGetTypeID(raw) != CFArrayGetTypeID()) return LL_AX_INVALID;
    CFIndex count = CFArrayGetCount((CFArrayRef)raw);
    if (count < 0 || count > 64) return LL_AX_BOUNDS;
    NSMutableArray<NSString *> *names = [NSMutableArray new];
    for (CFIndex index = 0; index < count; ++index) {
        CFTypeRef item = CFArrayGetValueAtIndex((CFArrayRef)raw, index);
        if (!item || CFGetTypeID(item) != CFStringGetTypeID()) return LL_AX_INVALID;
        NSString *name = (__bridge NSString *)item;
        int result = utf8(name, NULL);
        if (result) return result;
        if (!name.length || [names containsObject:name]) return LL_AX_INVALID;
        [names addObject:[name copy]];
    }
    *out = [names sortedArrayUsingSelector:@selector(compare:)];
    return LL_AX_OK;
}
static int widgetsNumber(CFTypeRef raw, double *out) {
    if (!raw || CFGetTypeID(raw) != CFNumberGetTypeID()) return LL_AX_INVALID;
    double number = 0;
    if (!CFNumberGetValue((CFNumberRef)raw, kCFNumberDoubleType, &number) || !isfinite(number)) return LL_AX_INVALID;
    *out = number;
    return LL_AX_OK;
}
static int widgetsCheckState(CFTypeRef raw, int32_t *out) {
    if (raw && CFGetTypeID(raw) == CFBooleanGetTypeID()) { *out = CFBooleanGetValue((CFBooleanRef)raw) ? 1 : 0; return LL_AX_OK; }
    double number = 0;
    int result = widgetsNumber(raw, &number);
    if (result || (number != 0 && number != 1 && number != 2)) return LL_AX_INVALID;
    *out = (int32_t)number;
    return LL_AX_OK;
}
static int widgetsPosition(double value, double minimum, double maximum, int32_t *out) {
    if (!isfinite(value) || !isfinite(minimum) || !isfinite(maximum) || minimum >= maximum || value < minimum || value > maximum) return LL_AX_INVALID;
    double span = maximum - minimum;
    if (!isfinite(span) || span <= 0) return LL_AX_INVALID;
    double normalized = (value - minimum) / span;
    if (!isfinite(normalized) || normalized < 0 || normalized > 1) return LL_AX_INVALID;
    *out = (int32_t)llround(normalized * 1000.0);
    return LL_AX_OK;
}
static int widgetsSettable(LLCF *item, CFStringRef name, BOOL *out, int32_t *detail) {
    Boolean settable = false;
    int result = api(AXUIElementIsAttributeSettable((AXUIElementRef)item->value, name, &settable), detail);
    if (controlsAbsent(result, *detail)) { *out = NO; return LL_AX_OK; }
    if (result) return result;
    *out = settable;
    return LL_AX_OK;
}
static int widgetsOptionalBoolean(LLCF *item, CFStringRef name, BOOL *present, BOOL *out, int32_t *detail) {
    LLCF *raw = nil;
    int result = attribute(item, name, &raw, detail);
    if (controlsAbsent(result, *detail)) { *present = NO; *out = NO; return LL_AX_OK; }
    if (result) return result;
    if (CFGetTypeID(raw->value) != CFBooleanGetTypeID()) return LL_AX_INVALID;
    *present = YES; *out = CFBooleanGetValue((CFBooleanRef)raw->value);
    return LL_AX_OK;
}
static int widgetsNumberAttribute(LLCF *item, CFStringRef name, BOOL *present, double *out, int32_t *detail) {
    LLCF *raw = nil;
    int result = attribute(item, name, &raw, detail);
    if (controlsAbsent(result, *detail)) { *present = NO; return LL_AX_OK; }
    if (result) return result;
    *present = YES;
    return widgetsNumber(raw->value, out);
}
static int widgetsMetadata(LLCF *item, NSMutableDictionary **out, size_t *bytes, int32_t *detail) {
    NSString *role = nil, *identifier = nil, *title = nil, *description = @"";
    int result = stringAttribute(item, kAXRoleAttribute, &role, detail);
    if (result) return controlsDiagnostic(result, *detail, "widget AXRole", role);
    if (!role.length) return LL_AX_INVALID;
    result = optionalString(item, kAXIdentifierAttribute, &identifier, detail);
    if (result) return controlsDiagnostic(result, *detail, "widget AXIdentifier", role);
    result = optionalString(item, kAXTitleAttribute, &title, detail);
    if (result) return controlsDiagnostic(result, *detail, "widget AXTitle", role);
    if (controlsDescribedRole(role) || [@[@"AXCheckBox", @"AXPopUpButton", @"AXMenuButton", @"AXMenuItem", @"AXScrollBar"] containsObject:role]) {
        result = optionalString(item, kAXDescriptionAttribute, &description, detail);
        if (result) return controlsDiagnostic(result, *detail, "widget AXDescription", role);
    }
    LLCF *names = [LLCF new]; CFArrayRef rawNames = NULL;
    result = api(AXUIElementCopyAttributeNames((AXUIElementRef)item->value, &rawNames), detail);
    names->value = rawNames;
    if (result) return result;
    NSArray<NSString *> *attributes = nil;
    result = controlsAttributeNames(rawNames, &attributes, bytes);
    if (result) return result;
    BOOL enabledPresent = NO, enabled = NO;
    result = widgetsOptionalBoolean(item, kAXEnabledAttribute, &enabledPresent, &enabled, detail);
    if (result) return controlsDiagnostic(result, *detail, "widget AXEnabled", role);
    BOOL editable = NO;
    if (controlsTextRole(role)) {
        result = widgetsSettable(item, kAXValueAttribute, &editable, detail);
        if (result) return result;
    }
    // Collection/scroll containers often omit AXEnabled; writable native
    // attributes remain the capability evidence. Explicit false always wins.
    NSString *value = nil; id typedValue = [NSNull null];
    if (controlsAdmittedRole(role) || widgetsMenuOwner(role) || [@[@"AXCheckBox", @"AXScrollBar", @"AXMenuItem"] containsObject:role]) {
        result = controlsValue(item, role, &value, &typedValue, detail);
        if (result) return controlsDiagnostic(result, *detail, "widget AXValue", role);
    }
    NSArray<NSString *> *actions = @[];
    if (widgetsRole(role)) {
        LLCF *storage = [LLCF new]; CFArrayRef actionNames = NULL;
        result = api(AXUIElementCopyActionNames((AXUIElementRef)item->value, &actionNames), detail);
        storage->value = actionNames;
        if (!controlsAbsent(result, *detail)) {
            if (result) return result;
            result = widgetsActions(actionNames, &actions);
            if (result) return result;
        }
    }
    BOOL pressable = [role isEqualToString:@"AXButton"] && [actions containsObject:@"AXPress"];
    int32_t checked = -1, selected = -1, position = -1;
    BOOL selectedPresent = NO, selectedValue = NO, selectedSettable = NO;
    if ([role isEqualToString:@"AXCheckBox"]) {
        LLCF *raw = nil; result = attribute(item, kAXValueAttribute, &raw, detail);
        if (result) return result;
        result = widgetsCheckState(raw->value, &checked); if (result) return result;
    }
    if ([role isEqualToString:@"AXGroup"] || [role isEqualToString:@"AXRow"] || [role isEqualToString:@"AXCell"] || [role isEqualToString:@"AXTextField"] || [role isEqualToString:@"AXStaticText"]) {
        result = widgetsOptionalBoolean(item, kAXSelectedAttribute, &selectedPresent, &selectedValue, detail);
        if (result) return result;
        if (selectedPresent) {
            selected = selectedValue ? 1 : 0;
            result = widgetsSettable(item, kAXSelectedAttribute, &selectedSettable, detail);
            if (result) return result;
        }
    }
    NSString *selectionAttribute = @"";
    BOOL collectionSettable = NO;
    if (widgetsCollection(role)) {
        CFStringRef attributeName = [role isEqualToString:@"AXList"] ? kAXSelectedChildrenAttribute : kAXSelectedRowsAttribute;
        result = widgetsSettable(item, attributeName, &collectionSettable, detail);
        if (result) return result;
        selectionAttribute = (__bridge NSString *)attributeName;
    }
    double numericValue = 0, minimum = 0, maximum = 1;
    BOOL scrollSettable = NO, minimumPresent = NO, maximumPresent = NO;
    NSString *orientation = @"";
    if ([role isEqualToString:@"AXScrollBar"]) {
        BOOL present = NO;
        result = widgetsNumberAttribute(item, kAXValueAttribute, &present, &numericValue, detail);
        if (result || !present) return result ?: LL_AX_INVALID;
        result = widgetsNumberAttribute(item, kAXMinValueAttribute, &minimumPresent, &minimum, detail); if (result) return result;
        result = widgetsNumberAttribute(item, kAXMaxValueAttribute, &maximumPresent, &maximum, detail); if (result) return result;
        if (minimumPresent != maximumPresent) return LL_AX_INVALID;
        // AXScrollBar's absent min/max denotes its conventional normalized
        // 0..1 AXValue; never infer a range for an arbitrary numeric role.
        result = widgetsPosition(numericValue, minimum, maximum, &position); if (result) return result;
        result = widgetsSettable(item, kAXValueAttribute, &scrollSettable, detail); if (result) return result;
        NSString *rawOrientation = nil;
        result = optionalString(item, kAXOrientationAttribute, &rawOrientation, detail); if (result) return result;
        if ([rawOrientation isEqualToString:@"AXVerticalOrientation"]) orientation = @"vertical";
        else if ([rawOrientation isEqualToString:@"AXHorizontalOrientation"]) orientation = @"horizontal";
        else if (rawOrientation.length && ![rawOrientation isEqualToString:@"AXUnknownOrientation"]) return LL_AX_INVALID;
    }
    BOOL nativeEnabled = enabledPresent ? enabled : editable || collectionSettable || scrollSettable || selectedSettable;
    uint32_t capabilities = 0;
    if (widgetsMenuOwner(role) && ([actions containsObject:@"AXShowMenu"] || [actions containsObject:@"AXPress"])) capabilities |= LL_WIDGET_OPEN_MENU;
    if ([role isEqualToString:@"AXCheckBox"] && [actions containsObject:@"AXPress"]) capabilities |= LL_WIDGET_SET_CHECKED;
    if (collectionSettable) capabilities |= LL_WIDGET_SELECT_ITEM;
    if (scrollSettable) capabilities |= LL_WIDGET_SCROLL;
    for (NSString *text in @[role, identifier, title, description, value ?: @"", orientation]) {
        size_t length = [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (*bytes > LL_CONTROLS_BYTES_LIMIT || length > LL_CONTROLS_BYTES_LIMIT - *bytes) return LL_AX_BOUNDS;
        *bytes += length;
    }
    for (NSString *action in actions) {
        size_t length = [action lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
        if (length > LL_CONTROLS_BYTES_LIMIT - *bytes) return LL_AX_BOUNDS;
        *bytes += length;
    }
    *out = [@{@"role":role, @"identifier":identifier, @"title":title, @"description":description,
        @"label":title.length ? title : description, @"value":value ?: (id)[NSNull null], @"typedValue":typedValue,
        @"attributes":attributes, @"actions":actions, @"enabled":@(nativeEnabled), @"enabledPresent":@(enabledPresent),
        @"editable":@(editable), @"pressable":@(pressable), @"checked":@(checked), @"selected":@(selected),
        @"nativeSelected":@(selected), @"selectedPresent":@(selectedPresent), @"selectedSettable":@(selectedSettable),
        @"selectionAttribute":selectionAttribute, @"collectionSettable":@(collectionSettable),
        @"position":@(position), @"numericValue":@(numericValue), @"minimum":@(minimum), @"maximum":@(maximum),
        @"minimumPresent":@(minimumPresent), @"maximumPresent":@(maximumPresent), @"orientation":orientation,
        @"capabilities":@(capabilities), @"parent":@0} mutableCopy];
    return LL_AX_OK;
}

static BOOL widgetsAncestor(NSArray<NSNumber *> *parents, NSUInteger child, NSUInteger ancestor) {
    if (child >= parents.count || ancestor >= parents.count || child == ancestor) return NO;
    for (NSUInteger steps = 0; steps < parents.count && child; ++steps) {
        child = parents[child].unsignedIntegerValue;
        if (child >= parents.count) return NO;
        if (child == ancestor) return YES;
    }
    return NO;
}
static NSInteger widgetsTreeIndex(NSArray<LLCF *> *tree, LLCF *item) {
    for (NSUInteger index = 0; index < tree.count; ++index) if (sameElement(tree[index], item)) return (NSInteger)index;
    return -1;
}
static int widgetsNearestParent(NSArray<LLCF *> *tree, NSArray<NSNumber *> *parents,
    NSArray<LLControlEntry *> *currentEntries, NSUInteger child, uint64_t *out) {
    if (tree.count != parents.count || child >= tree.count) return LL_AX_INVALID;
    NSUInteger ancestor = child;
    for (NSUInteger step = 0; step < tree.count; ++step) {
        if (!ancestor) { *out = 0; return LL_AX_OK; }
        ancestor = parents[ancestor].unsignedIntegerValue;
        if (ancestor >= tree.count || ancestor == child) return LL_AX_INVALID;
        LLControlEntry *parent = controlsLiveEntry(currentEntries, tree[ancestor]);
        if (parent) { *out = parent.identity; return LL_AX_OK; }
    }
    return LL_AX_INVALID;
}
static BOOL widgetsOwnedBy(NSDictionary *row, uint64_t owner) {
    return owner && row && [row[@"parent"] unsignedLongLongValue] == owner;
}
static int widgetsCancelMenu(NSArray<NSDictionary *> *rows, uint64_t owner, NSDictionary **out) {
    NSDictionary *menu = nil;
    for (NSDictionary *candidate in rows) {
        if (!widgetsOwnedBy(candidate, owner) || ![candidate[@"role"] isEqualToString:@"AXMenu"] ||
            ![candidate[@"actions"] containsObject:@"AXCancel"]) continue;
        if (menu) return LL_AX_INVALID;
        menu = candidate;
    }
    if (!menu) return LL_AX_NOT_EDITABLE;
    if (out) *out = menu;
    return LL_AX_OK;
}
static int widgetsSelectionChecked(LLCF *container, CFStringRef name, LLElementCheck check, void *ownership,
    NSMutableArray<LLCF *> **out, BOOL *known, int32_t *detail) {
    LLCF *storage = nil;
    int result = attribute(container, name, &storage, detail);
    if (result == LL_AX_API && *detail == kAXErrorAttributeUnsupported) {
        *known = NO; *out = [NSMutableArray new]; return LL_AX_OK;
    }
    if (result == LL_AX_API && *detail == kAXErrorNoValue) {
        *known = YES; *out = [NSMutableArray new]; return LL_AX_OK;
    }
    if (result) return result;
    if (CFGetTypeID(storage->value) != CFArrayGetTypeID()) return LL_AX_INVALID;
    CFArrayRef values = (CFArrayRef)storage->value;
    CFIndex count = CFArrayGetCount(values);
    if (count < 0 || count > LL_CHILD_LIMIT) return LL_AX_BOUNDS;
    NSMutableArray<LLCF *> *items = [NSMutableArray new];
    for (CFIndex index = 0; index < count; ++index) {
        CFTypeRef value = CFArrayGetValueAtIndex(values, index);
        if (!value) return LL_AX_INVALID;
        LLCF *item = [LLCF new]; item->value = CFRetain(value);
        result = check(item, ownership, detail); if (result) return result;
        [items addObject:item];
    }
    *known = YES; *out = items; return LL_AX_OK;
}
static int widgetsSelection(LLCF *container, CFStringRef name, pid_t pid,
    NSMutableArray<LLCF *> **out, BOOL *known, int32_t *detail) {
    return widgetsSelectionChecked(container, name, childSameProcess, &pid, out, known, detail);
}
static int widgetsRefreshInner(LLWidgetsContext *context, int32_t *detail) {
    int result = widgetsPermission(context, detail); if (result) return result;
    LLCF *window = nil;
    result = controlsWindow(context, &window, detail); if (result) return result;
    NSMutableArray<LLCF *> *tree = [NSMutableArray arrayWithObject:window];
    NSMutableArray<NSNumber *> *parents = [NSMutableArray arrayWithObject:@0];
    NSMutableArray<NSNumber *> *depths = [NSMutableArray arrayWithObject:@0];
    NSMutableArray<NSMutableDictionary *> *states = [NSMutableArray new];
    size_t bytes = 0;
    for (NSUInteger index = 0; index < tree.count; ++index) {
        pid_t owner = context.pid;
        if (context.auxiliary) {
            result = auxiliaryElementPID(tree[index], &owner, detail); if (result) return result;
            if (!auxiliaryAllowedPID(context.auxiliary, context.pid, owner)) return LL_AX_STALE;
        }
        NSMutableDictionary *state = nil;
        result = widgetsMetadata(tree[index], &state, &bytes, detail); if (result) return result;
        if (context.auxiliary) {
            pid_t confirmedOwner = 0;
            result = auxiliaryElementPID(tree[index], &confirmedOwner, detail); if (result) return result;
            if (confirmedOwner != owner) return LL_AX_STALE;
            state[@"ownerPID"] = @(owner);
        }
        if (!index) {
            if (![state[@"role"] isEqualToString:@"AXWindow"]) return LL_AX_STALE;
            LLCF *raw = nil; NSString *document = nil;
            result = attribute(tree[index], kAXDocumentAttribute, &raw, detail);
            if (!controlsAbsent(result, *detail)) {
                if (result) return result;
                result = scopeDocumentValue(raw->value, &document); if (result) return result;
            }
            size_t length = [document lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
            if (length > LL_CONTROLS_BYTES_LIMIT - bytes) return LL_AX_BOUNDS;
            bytes += length;
            state[@"document"] = document ?: (id)[NSNull null];
        }
        [states addObject:state];
        NSMutableArray<LLCF *> *descendants = nil;
        result = widgetsChildren(context, tree[index], &descendants, detail);
        if (result) return controlsDiagnostic(result, *detail, "widget AXChildren", state[@"role"]);
        // A shown popup menu can be exposed by this owner-specific attribute
        // instead of AXChildren. It is never taken from the global menu bar.
        if (widgetsMenuOwner(state[@"role"])) {
            LLCF *shown = nil;
            result = attribute(tree[index], kAXShownMenuUIElementAttribute, &shown, detail);
            if (!controlsAbsent(result, *detail)) {
                if (result) return result;
                result = context.auxiliary ? auxiliaryElement(context.auxiliary, context.pid, shown, tree[index], NO, detail) : element(shown, context.pid, detail);
                if (result) return result;
                BOOL present = NO;
                for (LLCF *child in descendants) if (sameElement(child, shown)) present = YES;
                if (!present) [descendants addObject:shown];
            }
        }
        NSUInteger depth = depths[index].unsignedIntegerValue;
        if ((depth >= LL_WIDGET_DEPTH_LIMIT && descendants.count) || tree.count + descendants.count > LL_WIDGET_NODE_LIMIT) return LL_AX_BOUNDS;
        for (LLCF *child in descendants) {
            for (LLCF *seen in tree) if (sameElement(seen, child)) return LL_AX_INVALID;
            [tree addObject:child]; [parents addObject:@(index)]; [depths addObject:@(depth + 1)];
        }
    }
    // Derive stable relationships from the retained CF tree, not labels or
    // output positions. Values in metadata are stable admitted native IDs.
    NSMutableArray<LLCF *> *items = [NSMutableArray new];
    NSMutableArray<NSMutableDictionary *> *metadata = [NSMutableArray new];
    NSMutableArray<NSDictionary *> *dependencies = [NSMutableArray new];
    NSMutableArray<NSNumber *> *admittedIndices = [NSMutableArray new];
    for (NSUInteger index = 0; index < tree.count; ++index) {
        BOOL collectionGroup = index && [states[index][@"role"] isEqualToString:@"AXGroup"] &&
            [states[parents[index].unsignedIntegerValue][@"role"] isEqualToString:@"AXList"];
        if (widgetsRole(states[index][@"role"]) || collectionGroup) {
            if (items.count >= LL_CONTROLS_LIMIT) return LL_AX_BOUNDS;
            [items addObject:tree[index]]; [metadata addObject:states[index]]; [admittedIndices addObject:@(index)];
        } else [dependencies addObject:states[index]];
    }
    // Reserve IDs for newly admitted nodes before relationships are projected.
    // Commit remains atomic: failure retires the connection and IDs never reuse.
    NSMutableArray<LLControlEntry *> *stagedRegistry = [context.registry mutableCopy] ?: [NSMutableArray new];
    NSMutableArray<LLControlEntry *> *currentEntries = [NSMutableArray new];
    for (LLCF *item in items) {
        LLControlEntry *entry = controlsLiveEntry(stagedRegistry, item);
        if (!entry) {
            if (stagedRegistry.count >= LL_CONTROLS_REGISTRY_LIMIT || nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
            entry = [LLControlEntry new]; entry.element = item; entry.identity = nextIdentity++;
            entry.key = [NSString stringWithFormat:@"%@/%llu", context.nonce, (unsigned long long)entry.identity];
            entry.alive = YES; [stagedRegistry addObject:entry];
        }
        [currentEntries addObject:entry];
    }
    for (NSUInteger itemIndex = 0; itemIndex < items.count; ++itemIndex) {
        NSUInteger index = admittedIndices[itemIndex].unsignedIntegerValue;
        NSMutableDictionary *state = metadata[itemIndex];
        // A historical entry stays alive until commit, but a role change can
        // remove it from this snapshot. Only current admitted parents count.
        uint64_t parent = 0;
        result = widgetsNearestParent(tree, parents, currentEntries, index, &parent);
        if (result) return result;
        state[@"parent"] = @(parent);
    }
    // Populate every admitted parent before reading collection membership or
    // deriving menu capabilities. Discovery and delivery share this ownership.
    for (NSUInteger itemIndex = 0; itemIndex < items.count; ++itemIndex) {
        NSUInteger index = admittedIndices[itemIndex].unsignedIntegerValue;
        NSMutableDictionary *state = metadata[itemIndex];
        if (widgetsCollection(state[@"role"])) {
            CFStringRef selectionAttribute = (__bridge CFStringRef)state[@"selectionAttribute"];
            NSMutableArray<LLCF *> *selected = nil;
            BOOL selectionKnown = NO;
            if (context.auxiliary) {
                LLWidgetsElementContext ownership = {context, nil, NO};
                result = widgetsSelectionChecked(items[itemIndex], selectionAttribute, widgetsElement, &ownership, &selected, &selectionKnown, detail);
            } else result = widgetsSelection(items[itemIndex], selectionAttribute, context.pid, &selected, &selectionKnown, detail);
            if (result) return result;
            state[@"selectionKnown"] = @(selectionKnown);
            NSMutableArray<NSNumber *> *selectedIDs = [NSMutableArray new];
            LLControlEntry *container = controlsLiveEntry(stagedRegistry, items[itemIndex]);
            for (LLCF *selection in selected) {
                NSInteger selectedIndex = widgetsTreeIndex(tree, selection);
                LLControlEntry *entry = controlsLiveEntry(stagedRegistry, selection);
                if (selectedIndex < 0 || !entry || !widgetsOwnedBy(states[(NSUInteger)selectedIndex], container.identity) ||
                    !widgetsSelectionRole(states[(NSUInteger)selectedIndex][@"role"], state[@"role"])) return LL_AX_INVALID;
                if ([selectedIDs containsObject:@(entry.identity)]) return LL_AX_INVALID;
                [selectedIDs addObject:@(entry.identity)];
            }
            [selectedIDs sortUsingSelector:@selector(compare:)];
            state[@"selectedIDs"] = selectedIDs;
            // AXSelected may be absent on a list child. A container's complete
            // selected-children/rows list then supplies the observed state.
            for (NSUInteger other = 0; other < items.count; ++other) {
                if (!widgetsOwnedBy(metadata[other], container.identity)) continue;
                // AXSelectedRows describes rows, not structural headers and
                // their buttons. Never synthesize selectable header objects.
                if (!widgetsSelectionRole(metadata[other][@"role"], state[@"role"])) {
                    metadata[other][@"selected"] = @(-1); continue;
                }
                LLControlEntry *child = controlsLiveEntry(stagedRegistry, items[other]);
                BOOL known = [metadata[other][@"selectedPresent"] boolValue];
                BOOL isSelected = [selectedIDs containsObject:@(child.identity)];
                if (selectionKnown && !known) metadata[other][@"selected"] = @(isSelected ? 1 : 0);
                else if (selectionKnown && [metadata[other][@"selected"] intValue] != (isSelected ? 1 : 0)) return LL_AX_INVALID;
                else if (!known && [metadata[other][@"role"] isEqualToString:@"AXGroup"]) return LL_AX_INVALID;
                if (![metadata[other][@"enabledPresent"] boolValue] && [state[@"collectionSettable"] boolValue]) metadata[other][@"enabled"] = state[@"enabled"];
                if ([metadata[other][@"selectedSettable"] boolValue]) {
                    state[@"capabilities"] = @([state[@"capabilities"] unsignedIntValue] | LL_WIDGET_SELECT_ITEM);
                    if (![state[@"enabledPresent"] boolValue]) state[@"enabled"] = @YES;
                }
            }
        }
        if (widgetsMenuOwner(state[@"role"])) {
            uint32_t capabilities = [state[@"capabilities"] unsignedIntValue];
            for (NSUInteger other = 0; other < items.count; ++other) {
                NSUInteger otherIndex = admittedIndices[other].unsignedIntegerValue;
                if (!widgetsAncestor(parents, otherIndex, index)) continue;
                NSDictionary *child = metadata[other];
                if ([child[@"role"] isEqualToString:@"AXMenuItem"] && [child[@"actions"] containsObject:@"AXPress"]) capabilities |= LL_WIDGET_CHOOSE_MENU;
            }
            LLControlEntry *owner = controlsLiveEntry(stagedRegistry, items[itemIndex]);
            if (widgetsCancelMenu(metadata, owner.identity, NULL) == LL_AX_OK) capabilities |= LL_WIDGET_CANCEL_MENU;
            state[@"capabilities"] = @(capabilities);
        }
    }
    for (NSMutableDictionary *state in metadata) {
        uint64_t parentIdentity = [state[@"parent"] unsignedLongLongValue];
        BOOL collectionParent = NO;
        for (NSUInteger index = 0; index < items.count; ++index) {
            LLControlEntry *entry = controlsLiveEntry(stagedRegistry, items[index]);
            if (entry.identity == parentIdentity) collectionParent = widgetsCollection(metadata[index][@"role"]);
        }
        if (!collectionParent) state[@"selected"] = @(-1);
    }
    result = widgetsPermission(context, detail); if (result) return result;
    LLCF *confirmed = nil;
    result = controlsWindow(context, &confirmed, detail); if (result) return result;
    if (!sameElement(window, confirmed)) return LL_AX_STALE;
    // The registry changes are published together with the complete snapshot.
    // Prior rows still determine whether this refresh advances the epoch.
    context.registry = stagedRegistry;
    result = controlsCommitBounded(context, items, metadata, dependencies, tree, parents, bytes, LL_WIDGET_NODE_LIMIT);
    if (result) return result;
    context.window = window;
    return LL_AX_OK;
}
static int widgetsRefresh(LLWidgetsContext *context, int32_t *detail) {
    int result = widgetsRefreshInner(context, detail);
    if (result && context && result != LL_AX_WRONG_THREAD) context.retired = YES;
    return result;
}
static NSDictionary *widgetsRow(LLWidgetsContext *context, uint64_t identity) {
    for (NSDictionary *row in context.rows) if ([row[@"identity"] unsignedLongLongValue] == identity) return row;
    return nil;
}
static LLControlEntry *widgetsEntry(LLWidgetsContext *context, uint64_t identity) {
    for (LLControlEntry *entry in context.registry) if (entry.alive && entry.identity == identity) return entry;
    return nil;
}
static BOOL widgetsDescendant(LLWidgetsContext *context, uint64_t child, uint64_t ancestor) {
    if (!child || !ancestor || child == ancestor) return NO;
    for (NSUInteger step = 0; step < LL_CONTROLS_LIMIT; ++step) {
        NSDictionary *row = widgetsRow(context, child);
        if (!row) return NO;
        child = [row[@"parent"] unsignedLongLongValue];
        if (!child) return NO;
        if (child == ancestor) return YES;
    }
    return NO;
}
static int widgetsPublish(LLWidgetsContext *context, LLWidgetRow *rows, size_t rowCapacity,
                           uint8_t *bytes, size_t byteCapacity, LLControlsSnapshot *out) {
    if (!context || !rows || !bytes || !out || !context.identity || !context.revision) return LL_AX_INVALID;
    if (context.rows.count > LL_CONTROLS_LIMIT || context.rows.count > rowCapacity) return LL_AX_BOUNDS;
    LLWidgetRow staged[LL_CONTROLS_LIMIT] = {0};
    NSMutableData *data = [NSMutableData new];
    for (NSUInteger index = 0; index < context.rows.count; ++index) {
        NSDictionary *row = context.rows[index]; LLWidgetRow *target = &staged[index];
        BOOL present = row[@"value"] != [NSNull null];
        NSArray<NSString *> *texts = @[row[@"key"], row[@"role"], row[@"identifier"], row[@"label"], present ? row[@"value"] : @"", row[@"orientation"]];
        size_t lengths[6] = {0};
        for (NSUInteger part = 0; part < texts.count; ++part) {
            NSData *encoded = nil; int result = utf8(texts[part], &encoded); if (result) return result;
            if (encoded.length > LL_CONTROLS_BYTES_LIMIT - data.length) return LL_AX_BOUNDS;
            lengths[part] = encoded.length; [data appendData:encoded];
        }
        target->control = (LLControlRow){[row[@"identity"] unsignedLongLongValue], lengths[0], lengths[1], lengths[2], lengths[3], lengths[4], present ? 1 : 0, [row[@"enabled"] boolValue] ? 1 : 0, [row[@"editable"] boolValue] ? 1 : 0, [row[@"pressable"] boolValue] ? 1 : 0};
        target->parent = [row[@"parent"] unsignedLongLongValue];
        target->checked = [row[@"checked"] intValue]; target->selected = [row[@"selected"] intValue]; target->position = [row[@"position"] intValue];
        target->capabilities = [row[@"capabilities"] unsignedIntValue]; target->orientation_length = lengths[5];
    }
    size_t count = context.rows.count, length = data.length; const void *source = data.bytes;
    if (length > byteCapacity) return LL_AX_BOUNDS;
    LLControlsSnapshot snapshot = {context.identity, context.revision, count, length};
    memcpy(rows, staged, count * sizeof(LLWidgetRow));
    if (length) memcpy(bytes, source, length);
    *out = snapshot;
    return LL_AX_OK;
}

static int widgetsPreflight(LLWidgetsContext *context, uint64_t scope, uint64_t revision,
    uint64_t target, uint32_t operation, uint64_t item, int32_t number,
    LLControlEntry **targetOut, LLControlEntry **itemOut, BOOL *noOp) {
    if (!context || !scope || scope != context.identity || !revision || revision != context.revision) return LL_AX_STALE;
    LLControlEntry *entry = widgetsEntry(context, target); NSDictionary *row = widgetsRow(context, target);
    if (!entry || !row) return LL_AX_STALE;
    if (![row[@"enabled"] boolValue]) return LL_AX_NOT_EDITABLE;
    NSString *role = row[@"role"]; uint32_t capabilities = [row[@"capabilities"] unsignedIntValue];
    *noOp = NO; *itemOut = nil;
    switch (operation) {
        case LL_WIDGET_PRESS:
            if (![role isEqualToString:@"AXButton"] || ![row[@"pressable"] boolValue]) return LL_AX_NOT_EDITABLE;
            break;
        case LL_WIDGET_SET_TEXT:
            if (!controlsTextRole(role) || ![row[@"editable"] boolValue]) return LL_AX_NOT_EDITABLE;
            break;
        case LL_WIDGET_OPEN:
            if (!widgetsMenuOwner(role) || !(capabilities & LL_WIDGET_OPEN_MENU)) return LL_AX_NOT_EDITABLE;
            break;
        case LL_WIDGET_CHOOSE: {
            if (!widgetsMenuOwner(role) || !(capabilities & LL_WIDGET_CHOOSE_MENU)) return LL_AX_NOT_EDITABLE;
            LLControlEntry *chosen = widgetsEntry(context, item); NSDictionary *selected = widgetsRow(context, item);
            if (!chosen || !selected || !widgetsDescendant(context, item, target)) return LL_AX_STALE;
            if (![selected[@"role"] isEqualToString:@"AXMenuItem"] || ![selected[@"enabled"] boolValue] || ![selected[@"actions"] containsObject:@"AXPress"]) return LL_AX_NOT_EDITABLE;
            *itemOut = chosen;
            break;
        }
        case LL_WIDGET_CANCEL: {
            if (!widgetsMenuOwner(role) || !(capabilities & LL_WIDGET_CANCEL_MENU)) return LL_AX_NOT_EDITABLE;
            NSDictionary *menu = nil;
            int result = widgetsCancelMenu(context.rows, target, &menu); if (result) return result;
            LLControlEntry *chosen = widgetsEntry(context, [menu[@"identity"] unsignedLongLongValue]);
            if (!chosen) return LL_AX_STALE;
            *itemOut = chosen;
            break;
        }
        case LL_WIDGET_CHECK: {
            if (![role isEqualToString:@"AXCheckBox"] || !(capabilities & LL_WIDGET_SET_CHECKED) || (number != 0 && number != 1)) return LL_AX_INVALID;
            int32_t checked = [row[@"checked"] intValue];
            if (checked != 0 && checked != 1) return LL_AX_INVALID;
            *noOp = checked == number;
            break;
        }
        case LL_WIDGET_SELECT: {
            if (!widgetsCollection(role) || !(capabilities & LL_WIDGET_SELECT_ITEM)) return LL_AX_NOT_EDITABLE;
            LLControlEntry *chosen = widgetsEntry(context, item); NSDictionary *selected = widgetsRow(context, item);
            if (!chosen || !widgetsOwnedBy(selected, target)) return LL_AX_STALE;
            if (![selected[@"enabled"] boolValue] || [selected[@"selected"] intValue] < 0) return LL_AX_NOT_EDITABLE;
            if (![row[@"collectionSettable"] boolValue] && ![selected[@"selectedSettable"] boolValue]) return LL_AX_NOT_EDITABLE;
            *itemOut = chosen;
            break;
        }
        case LL_WIDGET_POSITION:
            if (![role isEqualToString:@"AXScrollBar"] || !(capabilities & LL_WIDGET_SCROLL) || number < 0 || number > 1000) return LL_AX_INVALID;
            break;
        default: return LL_AX_INVALID;
    }
    *targetOut = entry;
    return LL_AX_OK;
}
static int32_t widgetsConnect(void *raw, const uint8_t *keyBytes, size_t keyLength,
    const uint8_t *titleBytes, size_t titleLength, const uint8_t *identifierBytes, size_t identifierLength,
    const uint8_t *documentBytes, size_t documentLength, BOOL withAuxiliary,
    const LLAuxiliarySpec *specifications, size_t count, LLAuxiliaryGrantCheck check,
    const void *grantContext, void **out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !out || !detail) return LL_AX_INVALID;
        *out = NULL; *detail = 0;
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        LLApplicationContext *application = (__bridge LLApplicationContext *)raw;
        LLWindowEntry *window = nil;
        int result = auxiliarySelectWindow(application, keyBytes, keyLength, titleBytes, titleLength,
            identifierBytes, identifierLength, documentBytes, documentLength, &window, detail);
        if (result) return result;
        LLAuxiliaryPolicy *policy = nil;
        if (withAuxiliary) {
            result = auxiliaryPolicy(application.pid, specifications, count, check, grantContext, &policy, detail);
            if (result) return result;
        }
        LLWidgetsContext *context = [LLWidgetsContext new];
        context.kind = LL_CONTROLS_PINNED_WINDOW; context.pid = application.pid;
        context.startSeconds = application.startSeconds; context.startMicros = application.startMicros;
        context.executable = application.executable; context.application = application.application;
        context.window = window.element; context.nonce = [NSUUID UUID].UUIDString;
        context.auxiliary = policy;
        if (nextIdentity == UINT64_MAX) return LL_AX_BOUNDS;
        context.identity = nextIdentity++;
        result = widgetsRefresh(context, detail); if (result) return result;
        *out = (__bridge_retained void *)context;
        return LL_AX_OK;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLApplicationContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
int32_t ll_application_widgets(void *raw, const uint8_t *key, size_t keyLength,
    const uint8_t *title, size_t titleLength, const uint8_t *identifier, size_t identifierLength,
    const uint8_t *document, size_t documentLength, void **out, int32_t *detail) {
    return widgetsConnect(raw, key, keyLength, title, titleLength, identifier, identifierLength,
        document, documentLength, NO, NULL, 0, NULL, NULL, out, detail);
}
int32_t ll_application_widgets_with_auxiliary(void *raw,
    const uint8_t *key, size_t keyLength, const uint8_t *title, size_t titleLength,
    const uint8_t *identifier, size_t identifierLength, const uint8_t *document, size_t documentLength,
    const LLAuxiliarySpec *specifications, size_t count, LLAuxiliaryGrantCheck check,
    const void *grantContext, void **out, int32_t *detail) {
    return widgetsConnect(raw, key, keyLength, title, titleLength, identifier, identifierLength,
        document, documentLength, YES, specifications, count, check, grantContext, out, detail);
}
int32_t ll_widgets_permission(void *raw, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !detail) return LL_AX_INVALID;
        *detail = 0;
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        LLWidgetsContext *context = (__bridge LLWidgetsContext *)raw;
        // Auxiliary permission includes current reachability, including accept
        // of targetless discovery replies. The default keeps its old check.
        return context.auxiliary ? widgetsRefresh(context, detail) : controlsPermission(context, detail);
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLWidgetsContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
int32_t ll_widgets_revoke(void *raw, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !detail) return LL_AX_INVALID;
        *detail = 0;
        if (![NSThread isMainThread]) return LL_AX_WRONG_THREAD;
        LLWidgetsContext *context = (__bridge LLWidgetsContext *)raw;
        if (!context.auxiliary) return LL_AX_INVALID;
        context.auxiliary.revoked = YES; context.retired = YES;
        return LL_AX_OK;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        return LL_AX_API;
    } }
}
int32_t ll_widgets_snapshot(void *raw, LLWidgetRow *rows, size_t rowCapacity,
    uint8_t *bytes, size_t byteCapacity, LLControlsSnapshot *out, int32_t *detail) {
    @autoreleasepool { @try {
        if (!raw || !rows || !bytes || !out || !detail) return LL_AX_INVALID;
        *detail = 0; LLWidgetsContext *context = (__bridge LLWidgetsContext *)raw;
        int result = widgetsRefresh(context, detail); if (result) return result;
        return widgetsPublish(context, rows, rowCapacity, bytes, byteCapacity, out);
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLWidgetsContext *)raw).retired = YES;
        return LL_AX_API;
    } }
}
int32_t ll_widgets_perform(void *raw, uint64_t scope, uint64_t revision,
    uint64_t target, uint32_t operation, uint64_t item, int32_t number,
    const uint8_t *text, size_t textLength, int32_t *detail) {
    BOOL entered = NO;
    @autoreleasepool { @try {
        if (!raw || !detail) return LL_AX_INVALID;
        *detail = 0;
        NSString *replacement = nil;
        if (operation == LL_WIDGET_SET_TEXT) { int result = decode(text, textLength, &replacement); if (result) return result; }
        else if (textLength) return LL_AX_INVALID;
        LLWidgetsContext *context = (__bridge LLWidgetsContext *)raw;
        int result = widgetsRefresh(context, detail); if (result) return result;
        LLControlEntry *entry = nil, *chosen = nil; BOOL noOp = NO;
        result = widgetsPreflight(context, scope, revision, target, operation, item, number, &entry, &chosen, &noOp);
        if (result || noOp) return result;
        NSDictionary *row = widgetsRow(context, target);
        AXUIElementRef receiver = (AXUIElementRef)entry.element->value;
        CFStringRef action = NULL, attributeName = NULL;
        id value = nil;
        switch (operation) {
            case LL_WIDGET_PRESS: case LL_WIDGET_CHECK: action = kAXPressAction; break;
            case LL_WIDGET_SET_TEXT: attributeName = kAXValueAttribute; value = replacement; break;
            case LL_WIDGET_OPEN:
                action = [row[@"actions"] containsObject:@"AXShowMenu"] ? kAXShowMenuAction : kAXPressAction;
                break;
            case LL_WIDGET_CHOOSE:
                receiver = (AXUIElementRef)chosen.element->value; action = kAXPressAction; break;
            case LL_WIDGET_CANCEL: {
                receiver = (AXUIElementRef)chosen.element->value; action = kAXCancelAction;
                break;
            }
            case LL_WIDGET_SELECT:
                if ([row[@"collectionSettable"] boolValue]) {
                    attributeName = (__bridge CFStringRef)row[@"selectionAttribute"];
                    value = @[(__bridge id)chosen.element->value];
                } else {
                    receiver = (AXUIElementRef)chosen.element->value; attributeName = kAXSelectedAttribute; value = @YES;
                }
                break;
            case LL_WIDGET_POSITION: {
                double minimum = [row[@"minimum"] doubleValue], maximum = [row[@"maximum"] doubleValue];
                double desired = minimum + ((double)number / 1000.0) * (maximum - minimum);
                if (!isfinite(desired) || desired < minimum || desired > maximum) return LL_AX_INVALID;
                attributeName = kAXValueAttribute; value = @(desired); break;
            }
            default: return LL_AX_INVALID;
        }
        // Only this call can deliver a mutation. No retries, waits, coordinate
        // events, focus changes or automatic postcondition reads are hidden.
        if (context.auxiliary) {
            result = widgetsPermission(context, detail); if (result) return result;
        }
        entered = YES;
        AXError error = action ? AXUIElementPerformAction(receiver, action) :
            AXUIElementSetAttributeValue(receiver, attributeName, (__bridge CFTypeRef)value);
        if (error != kAXErrorSuccess) { context.retired = YES; *detail = error; return LL_AX_UNCERTAIN; }
        return LL_AX_OK;
    } @catch (NSException *exception) {
        (void)exception; if (detail) *detail = -1;
        if (raw && [NSThread isMainThread]) ((__bridge LLWidgetsContext *)raw).retired = YES;
        return entered ? LL_AX_UNCERTAIN : LL_AX_API;
    } }
}
