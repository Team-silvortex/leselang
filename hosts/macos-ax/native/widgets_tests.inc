// Pure widget policy and publication tests. CFString-backed identities never
// reach an AX call; the tests do not operate applications or request access.
#include <float.h>

static NSDictionary *pureWidget(uint64_t identity, uint64_t parent, NSString *role) {
    return @{ @"identity": @(identity), @"key": [NSString stringWithFormat:@"widgets/%llu", (unsigned long long)identity],
        @"role": role, @"identifier": @"id", @"label": @"label", @"value": [NSNull null],
        @"enabled": @YES, @"editable": @NO, @"pressable": @NO,
        @"parent": @(parent), @"checked": @(-1), @"selected": @(-1), @"position": @(-1),
        @"capabilities": @0, @"orientation": @"" };
}

static void runWidgetTests(void) {
    // Creating a local AX reference makes no remote call. PID/timeout reads
    // are intercepted above, so ownership policy is tested without any app.
    LLCF *foreignProcess = [LLCF new];
    foreignProcess->value = AXUIElementCreateApplication(42);
    pidValue = 43; timeoutCalls = 0;
    int32_t ownershipDetail = kAXErrorAttributeUnsupported;
    assert(element(foreignProcess, 42, &ownershipDetail) == LL_AX_STALE && ownershipDetail == 0 && timeoutCalls == 0);
    pidError = kAXErrorInvalidUIElement;
    assert(element(foreignProcess, 42, &ownershipDetail) == LL_AX_STALE && ownershipDetail == kAXErrorInvalidUIElement && timeoutCalls == 0);
    pidError = kAXErrorSuccess; pidValue = 42;
    assert(element(foreignProcess, 42, &ownershipDetail) == LL_AX_OK && timeoutCalls == 1);
    timeoutError = kAXErrorCannotComplete;
    assert(element(foreignProcess, 42, &ownershipDetail) == LL_AX_API && ownershipDetail == kAXErrorCannotComplete && timeoutCalls == 2);
    timeoutError = kAXErrorSuccess; pidValue = 0; timeoutCalls = 0;
    puts("PASS foreign process ownership refuses without retaining unrelated AX error details");

    NSArray<NSString *> *actions = nil;
    assert(widgetsActions((__bridge CFArrayRef)@[@"AXShowMenu", @"AXPress"], &actions) == LL_AX_OK);
    assert(([actions isEqualToArray:@[@"AXPress", @"AXShowMenu"]]));
    assert(widgetsActions((__bridge CFArrayRef)@[], &actions) == LL_AX_OK && actions.count == 0);
    assert(widgetsActions((__bridge CFArrayRef)@[@"AXPress", @"AXPress"], &actions) == LL_AX_INVALID);
    assert(widgetsActions((__bridge CFArrayRef)@[@1], &actions) == LL_AX_INVALID);
    assert(widgetsActions((__bridge CFArrayRef)@[@""], &actions) == LL_AX_INVALID);
    assert(widgetsActions(CFSTR("AXPress"), &actions) == LL_AX_INVALID);
    assert(widgetsActions(NULL, &actions) == LL_AX_INVALID);
    NSMutableArray *many = [NSMutableArray new];
    for (NSUInteger index = 0; index < 65; ++index) [many addObject:[NSString stringWithFormat:@"action-%lu", (unsigned long)index]];
    assert(widgetsActions((__bridge CFArrayRef)many, &actions) == LL_AX_BOUNDS);
    NSString *longName = [@"" stringByPaddingToLength:4097 withString:@"x" startingAtIndex:0];
    assert(widgetsActions((__bridge CFArrayRef)@[longName], &actions) == LL_AX_BOUNDS);
    puts("PASS typed, unique and bounded widget action names");

    int32_t check = -7;
    assert(widgetsCheckState(kCFBooleanFalse, &check) == LL_AX_OK && check == 0);
    assert(widgetsCheckState(kCFBooleanTrue, &check) == LL_AX_OK && check == 1);
    for (NSNumber *number in @[@0, @1, @2]) {
        assert(widgetsCheckState((__bridge CFNumberRef)number, &check) == LL_AX_OK && check == number.intValue);
    }
    for (NSNumber *number in @[@(-1), @3, @0.5, @(NAN), @(INFINITY)]) {
        check = -7;
        assert(widgetsCheckState((__bridge CFNumberRef)number, &check) == LL_AX_INVALID && check == -7);
    }
    assert(widgetsCheckState(CFSTR("1"), &check) == LL_AX_INVALID);
    assert(widgetsCheckState(NULL, &check) == LL_AX_INVALID);
    double numeric = 123;
    assert(widgetsNumber(kCFBooleanTrue, &numeric) == LL_AX_INVALID && numeric == 123);
    puts("PASS checkbox state preserves mixed values and rejects invalid numeric types");

    int32_t position = -7;
    assert(widgetsPosition(0, 0, 1, &position) == LL_AX_OK && position == 0);
    assert(widgetsPosition(1, 0, 1, &position) == LL_AX_OK && position == 1000);
    assert(widgetsPosition(15, 10, 20, &position) == LL_AX_OK && position == 500);
    assert(widgetsPosition(-5, -10, 0, &position) == LL_AX_OK && position == 500);
    const double invalid[][3] = {{0, 0, 0}, {0, 1, 0}, {-1, 0, 1}, {2, 0, 1},
        {NAN, 0, 1}, {INFINITY, 0, 1}, {0, -INFINITY, 1}, {0, 0, INFINITY},
        {0, -DBL_MAX, DBL_MAX}};
    for (size_t index = 0; index < sizeof(invalid) / sizeof(invalid[0]); ++index) {
        position = -7;
        assert(widgetsPosition(invalid[index][0], invalid[index][1], invalid[index][2], &position) == LL_AX_INVALID && position == -7);
    }
    puts("PASS finite scrollbar ranges, normalized positions and overflow refusal");

    NSArray<NSNumber *> *parents = @[@0, @0, @1, @2, @0];
    assert(widgetsAncestor(parents, 3, 1));
    assert(!widgetsAncestor(parents, 1, 3));
    assert(!widgetsAncestor(parents, 3, 3));
    assert(!widgetsAncestor(parents, 4, 1));
    assert(!widgetsAncestor(parents, 5, 0));
    assert(!widgetsAncestor(@[@0, @99, @1], 2, 0));
    assert(!widgetsAncestor(@[@0, @2, @1, @0], 2, 3));
    NSArray<LLCF *> *tree = @[pureIdentity(@"window"), pureIdentity(@"owner"), pureIdentity(@"item")];
    assert(widgetsTreeIndex(tree, pureIdentity(@"item")) == 2);
    assert(widgetsTreeIndex(tree, pureIdentity(@"foreign")) == -1);
    LLWidgetsContext *context = [LLWidgetsContext new];
    context.identity = 900; context.revision = 1;
    context.rows = @[pureWidget(1, 0, @"AXPopUpButton"), pureWidget(2, 1, @"AXMenu"), pureWidget(3, 2, @"AXMenuItem"), pureWidget(4, 0, @"AXPopUpButton")];
    assert(widgetsDescendant(context, 3, 1));
    assert(!widgetsDescendant(context, 3, 4));
    assert(!widgetsDescendant(context, 3, 3));
    assert(!widgetsDescendant(context, 99, 1));
    context.rows = @[pureWidget(1, 2, @"AXMenu"), pureWidget(2, 1, @"AXMenuItem"), pureWidget(3, 0, @"AXPopUpButton")];
    assert(!widgetsDescendant(context, 1, 3));
    puts("PASS native widget ancestry, foreign membership and bounded cycle refusal");

    NSArray<LLCF *> *changingTree = @[pureIdentity(@"window"), pureIdentity(@"list"), pureIdentity(@"former-group"), pureIdentity(@"text")];
    NSArray<NSNumber *> *changingParents = @[@0, @0, @1, @2];
    NSMutableArray<LLControlEntry *> *historicalEntries = [NSMutableArray new];
    for (NSUInteger index = 1; index < changingTree.count; ++index) {
        LLControlEntry *entry = [LLControlEntry new];
        entry.element = changingTree[index]; entry.identity = index; entry.alive = YES;
        [historicalEntries addObject:entry];
    }
    uint64_t parentIdentity = 99;
    assert(widgetsNearestParent(changingTree, changingParents, historicalEntries, 3, &parentIdentity) == LL_AX_OK && parentIdentity == 2);
    // The former group's CF identity remains in the tree and lifetime registry,
    // but its changed role no longer belongs to the current admitted snapshot.
    NSArray<LLControlEntry *> *currentEntries = @[historicalEntries[0], historicalEntries[2]];
    assert(widgetsNearestParent(changingTree, changingParents, currentEntries, 3, &parentIdentity) == LL_AX_OK && parentIdentity == 1);
    assert(historicalEntries[1].alive);
    assert(widgetsNearestParent(changingTree, changingParents, @[historicalEntries[2]], 3, &parentIdentity) == LL_AX_OK && parentIdentity == 0);
    parentIdentity = 99;
    assert(widgetsNearestParent(changingTree, @[@0, @0, @99, @2], currentEntries, 3, &parentIdentity) == LL_AX_INVALID && parentIdentity == 99);
    assert(widgetsNearestParent(changingTree, @[@0, @0, @3, @2], @[], 3, &parentIdentity) == LL_AX_INVALID && parentIdentity == 99);
    puts("PASS role retirement projects parents only from the current admitted snapshot");

    // Attribute reads are intercepted by fakeAttribute in bridge_tests.m.
    // Malformed elements fail their CF type check before any native PID call.
    AXError savedAttributeError = attributeError;
    CFTypeRef savedAttributeValue = attributeValue;
    LLCF *selectionContainer = pureIdentity(@"selection-container");
    NSMutableArray<LLCF *> *selection = nil;
    BOOL selectionKnown = YES;
    int32_t selectionDetail = 0;
    attributeValue = NULL;
    attributeError = kAXErrorAttributeUnsupported;
    assert(widgetsSelection(selectionContainer, kAXSelectedChildrenAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_OK && !selectionKnown && selection.count == 0);
    attributeError = kAXErrorNoValue;
    assert(widgetsSelection(selectionContainer, kAXSelectedChildrenAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_OK && selectionKnown && selection.count == 0);
    attributeError = kAXErrorSuccess;
    attributeValue = (__bridge CFArrayRef)@[];
    assert(widgetsSelection(selectionContainer, kAXSelectedRowsAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_OK && selectionKnown && selection.count == 0);
    NSMutableArray<LLCF *> *untouchedSelection = [NSMutableArray arrayWithObject:selectionContainer];
    selection = untouchedSelection; selectionKnown = NO;
    attributeValue = CFSTR("not an array");
    assert(widgetsSelection(selectionContainer, kAXSelectedChildrenAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_INVALID && selection == untouchedSelection && !selectionKnown);
    attributeValue = (__bridge CFArrayRef)@[@1];
    assert(widgetsSelection(selectionContainer, kAXSelectedChildrenAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_INVALID && selection == untouchedSelection && !selectionKnown);
    attributeValue = (__bridge CFArrayRef)many;
    assert(widgetsSelection(selectionContainer, kAXSelectedChildrenAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_BOUNDS && selection == untouchedSelection && !selectionKnown);
    attributeError = kAXErrorCannotComplete;
    assert(widgetsSelection(selectionContainer, kAXSelectedChildrenAttribute, 1, &selection, &selectionKnown, &selectionDetail) == LL_AX_API && selectionDetail == kAXErrorCannotComplete && selection == untouchedSelection && !selectionKnown);
    attributeError = savedAttributeError; attributeValue = savedAttributeValue;
    puts("PASS selection attributes distinguish unsupported, empty, malformed and failed reads");

    LLControlsContext *oldBudget = [LLControlsContext new];
    LLWidgetsContext *widgetBudget = [LLWidgetsContext new];
    NSMutableArray<LLCF *> *boundedTree = [NSMutableArray new];
    NSMutableArray<NSNumber *> *boundedParents = [NSMutableArray new];
    for (NSUInteger index = 0; index <= LL_WIDGET_NODE_LIMIT; ++index) {
        [boundedTree addObject:pureIdentity([NSString stringWithFormat:@"bounded-%lu", (unsigned long)index])];
        [boundedParents addObject:@0];
    }
    NSArray<LLCF *> *oldTree = [boundedTree subarrayWithRange:NSMakeRange(0, LL_NODE_LIMIT)];
    NSArray<NSNumber *> *oldParents = [boundedParents subarrayWithRange:NSMakeRange(0, LL_NODE_LIMIT)];
    assert(controlsCommit(oldBudget, @[], @[], @[], oldTree, oldParents, 0) == LL_AX_OK);
    assert(controlsCommit(oldBudget, @[], @[], @[], [boundedTree subarrayWithRange:NSMakeRange(0, LL_NODE_LIMIT + 1)], [boundedParents subarrayWithRange:NSMakeRange(0, LL_NODE_LIMIT + 1)], 0) == LL_AX_BOUNDS);
    assert(oldBudget.tree == oldTree && oldBudget.parents == oldParents && oldBudget.revision == 1);
    NSArray<LLCF *> *widgetTree = [boundedTree subarrayWithRange:NSMakeRange(0, LL_WIDGET_NODE_LIMIT)];
    NSArray<NSNumber *> *widgetParents = [boundedParents subarrayWithRange:NSMakeRange(0, LL_WIDGET_NODE_LIMIT)];
    assert(controlsCommitBounded(widgetBudget, @[], @[], @[], widgetTree, widgetParents, 0, LL_WIDGET_NODE_LIMIT) == LL_AX_OK);
    assert(controlsCommitBounded(widgetBudget, @[], @[], @[], boundedTree, boundedParents, 0, LL_WIDGET_NODE_LIMIT) == LL_AX_BOUNDS);
    assert(widgetBudget.tree == widgetTree && widgetBudget.parents == widgetParents && widgetBudget.revision == 1);
    puts("PASS widget traversal extends to 512 while legacy control commit retains its 128-node bound");

    NSMutableDictionary *owner = [pureWidget(1, 0, @"AXPopUpButton") mutableCopy];
    owner[@"capabilities"] = @(LL_WIDGET_OPEN_MENU | LL_WIDGET_CHOOSE_MENU | LL_WIDGET_CANCEL_MENU);
    NSMutableDictionary *menu = [pureWidget(2, 1, @"AXMenu") mutableCopy];
    menu[@"actions"] = @[@"AXCancel"];
    NSMutableDictionary *choice = [pureWidget(3, 2, @"AXMenuItem") mutableCopy];
    choice[@"actions"] = @[@"AXPress"];
    NSMutableDictionary *checkRow = [pureWidget(4, 0, @"AXCheckBox") mutableCopy];
    checkRow[@"capabilities"] = @(LL_WIDGET_SET_CHECKED); checkRow[@"checked"] = @0;
    NSMutableDictionary *list = [pureWidget(5, 0, @"AXList") mutableCopy];
    list[@"capabilities"] = @(LL_WIDGET_SELECT_ITEM); list[@"collectionSettable"] = @YES;
    NSMutableDictionary *listItem = [pureWidget(6, 5, @"AXRow") mutableCopy];
    listItem[@"selected"] = @0;
    NSMutableDictionary *bar = [pureWidget(7, 0, @"AXScrollBar") mutableCopy];
    bar[@"capabilities"] = @(LL_WIDGET_SCROLL); bar[@"position"] = @500;
    context.rows = @[owner, menu, choice, checkRow, list, listItem, bar];
    NSMutableArray<LLControlEntry *> *registry = [NSMutableArray new];
    for (NSDictionary *row in context.rows) {
        LLControlEntry *entry = [LLControlEntry new];
        entry.identity = [row[@"identity"] unsignedLongLongValue]; entry.key = row[@"key"];
        entry.element = pureIdentity(entry.key); entry.alive = YES; [registry addObject:entry];
    }
    context.registry = registry;
    NSDictionary *cancelMenu = nil;
    NSMutableDictionary *nestedMenu = [pureWidget(8, 3, @"AXMenu") mutableCopy];
    nestedMenu[@"actions"] = @[@"AXCancel"];
    NSMutableDictionary *secondMenu = [pureWidget(9, 1, @"AXMenu") mutableCopy];
    secondMenu[@"actions"] = @[@"AXCancel"];
    assert(widgetsOwnedBy(listItem, 5) && !widgetsOwnedBy(choice, 1));
    assert(!widgetsOwnedBy(nil, 5) && !widgetsOwnedBy(owner, 0));
    assert(widgetsSelectionRole(@"AXGroup", @"AXList"));
    assert(widgetsSelectionRole(@"AXRow", @"AXOutline"));
    assert(widgetsSelectionRole(@"AXRow", @"AXTable"));
    assert(!widgetsSelectionRole(@"AXGroup", @"AXOutline"));
    assert(!widgetsSelectionRole(@"AXButton", @"AXOutline"));
    assert(!widgetsSelectionRole(@"AXButton", @"AXList"));
    assert(widgetsCancelMenu(@[owner, menu, choice], 1, &cancelMenu) == LL_AX_OK && cancelMenu == menu);
    cancelMenu = choice;
    assert(widgetsCancelMenu(@[owner, choice, nestedMenu], 1, &cancelMenu) == LL_AX_NOT_EDITABLE && cancelMenu == choice);
    assert(widgetsCancelMenu(@[owner, menu, secondMenu], 1, &cancelMenu) == LL_AX_INVALID && cancelMenu == choice);
    assert(widgetsCancelMenu(@[owner, menu, nestedMenu], 1, &cancelMenu) == LL_AX_OK && cancelMenu == menu);
    assert(widgetsCancelMenu(@[owner, menu, nestedMenu], 4, NULL) == LL_AX_NOT_EDITABLE);
    menu[@"actions"] = @[@"AXPress"];
    assert(widgetsCancelMenu(@[owner, menu, nestedMenu], 1, NULL) == LL_AX_NOT_EDITABLE);
    menu[@"actions"] = @[@"AXCancel"];
    puts("PASS direct collection ownership and unique immediate menu cancellation discovery");

    LLControlEntry *target = nil, *selectedItem = nil; BOOL noOp = NO;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_OPEN, 0, 0, &target, &selectedItem, &noOp) == LL_AX_OK && target.identity == 1 && !selectedItem && !noOp);
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CHOOSE, 3, 0, &target, &selectedItem, &noOp) == LL_AX_OK && selectedItem.identity == 3);
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CHOOSE, 6, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    choice[@"enabled"] = @NO;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CHOOSE, 3, 0, &target, &selectedItem, &noOp) == LL_AX_NOT_EDITABLE);
    choice[@"enabled"] = @YES;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CANCEL, 0, 0, &target, &selectedItem, &noOp) == LL_AX_OK && selectedItem.identity == 2);
    menu[@"parent"] = @3;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CANCEL, 0, 0, &target, &selectedItem, &noOp) == LL_AX_NOT_EDITABLE);
    menu[@"parent"] = @1;
    registry[1].alive = NO;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CANCEL, 0, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    registry[1].alive = YES;
    assert(widgetsPreflight(context, 900, 1, 4, LL_WIDGET_CHECK, 0, 0, &target, &selectedItem, &noOp) == LL_AX_OK && noOp);
    assert(widgetsPreflight(context, 900, 1, 4, LL_WIDGET_CHECK, 0, 1, &target, &selectedItem, &noOp) == LL_AX_OK && !noOp);
    checkRow[@"checked"] = @2;
    assert(widgetsPreflight(context, 900, 1, 4, LL_WIDGET_CHECK, 0, 1, &target, &selectedItem, &noOp) == LL_AX_INVALID);
    checkRow[@"checked"] = @0;
    assert(widgetsPreflight(context, 900, 1, 4, LL_WIDGET_CHECK, 0, 2, &target, &selectedItem, &noOp) == LL_AX_INVALID);
    assert(widgetsPreflight(context, 900, 1, 5, LL_WIDGET_SELECT, 6, 0, &target, &selectedItem, &noOp) == LL_AX_OK && selectedItem.identity == 6);
    listItem[@"parent"] = @3;
    assert(widgetsPreflight(context, 900, 1, 5, LL_WIDGET_SELECT, 6, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    listItem[@"parent"] = @5;
    assert(widgetsPreflight(context, 900, 1, 5, LL_WIDGET_SELECT, 3, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    list[@"collectionSettable"] = @NO;
    assert(widgetsPreflight(context, 900, 1, 5, LL_WIDGET_SELECT, 6, 0, &target, &selectedItem, &noOp) == LL_AX_NOT_EDITABLE);
    listItem[@"selectedSettable"] = @YES;
    assert(widgetsPreflight(context, 900, 1, 5, LL_WIDGET_SELECT, 6, 0, &target, &selectedItem, &noOp) == LL_AX_OK);
    assert(widgetsPreflight(context, 900, 1, 7, LL_WIDGET_POSITION, 0, 1000, &target, &selectedItem, &noOp) == LL_AX_OK);
    assert(widgetsPreflight(context, 900, 1, 7, LL_WIDGET_POSITION, 0, 1001, &target, &selectedItem, &noOp) == LL_AX_INVALID);
    assert(widgetsPreflight(context, 900, 1, 7, LL_WIDGET_POSITION, 0, -1, &target, &selectedItem, &noOp) == LL_AX_INVALID);
    assert(widgetsPreflight(context, 900, 2, 1, LL_WIDGET_OPEN, 0, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    assert(widgetsPreflight(context, 901, 1, 1, LL_WIDGET_OPEN, 0, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    assert(widgetsPreflight(context, 900, 1, 99, LL_WIDGET_OPEN, 0, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    registry[2].alive = NO;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_CHOOSE, 3, 0, &target, &selectedItem, &noOp) == LL_AX_STALE);
    owner[@"enabled"] = @NO;
    assert(widgetsPreflight(context, 900, 1, 1, LL_WIDGET_OPEN, 0, 0, &target, &selectedItem, &noOp) == LL_AX_NOT_EDITABLE);
    puts("PASS widget preflight fences state, membership, capabilities and desired checkbox no-ops");

    context.rows = @[pureWidget(1, 0, @"AXCheckBox")];
    LLWidgetRow rows[LL_CONTROLS_LIMIT];
    uint8_t bytes[1024];
    memset(rows, 0x5a, sizeof(rows)); memset(bytes, 0x5a, sizeof(bytes));
    LLControlsSnapshot output = {71, 72, 73, 74};
    assert(widgetsPublish(context, rows, 0, bytes, sizeof(bytes), &output) == LL_AX_BOUNDS);
    assert(widgetsPublish(context, rows, LL_CONTROLS_LIMIT, bytes, 1, &output) == LL_AX_BOUNDS);
    assert(output.identity == 71 && output.revision == 72 && output.count == 73 && output.bytes_length == 74);
    for (size_t index = 0; index < sizeof(rows); ++index) assert(((uint8_t *)rows)[index] == 0x5a);
    for (size_t index = 0; index < sizeof(bytes); ++index) assert(bytes[index] == 0x5a);
    assert(widgetsPublish(context, rows, LL_CONTROLS_LIMIT, bytes, sizeof(bytes), &output) == LL_AX_OK);
    assert(output.identity == 900 && output.revision == 1 && output.count == 1);
    assert(rows[0].control.identity == 1 && rows[0].checked == -1 && rows[0].selected == -1 && rows[0].position == -1);
    assert(output.bytes_length == strlen("widgets/1AXCheckBoxidlabel"));
    assert(memcmp(bytes, "widgets/1AXCheckBoxidlabel", output.bytes_length) == 0);
    assert(widgetsPublish(NULL, rows, LL_CONTROLS_LIMIT, bytes, sizeof(bytes), &output) == LL_AX_INVALID);
    int32_t detail = 0;
    assert(ll_widgets_snapshot(NULL, rows, LL_CONTROLS_LIMIT, bytes, sizeof(bytes), &output, &detail) == LL_AX_INVALID);
    assert(ll_widgets_perform(NULL, 1, 1, 2, LL_WIDGET_CHECK, 0, 1, NULL, 0, &detail) == LL_AX_INVALID);
    assert(ll_application_widgets(NULL, NULL, 0, NULL, 0, NULL, 0, NULL, 0, NULL, &detail) == LL_AX_INVALID);
    puts("PASS staged widget snapshot publication and untouched failure outputs");
}
