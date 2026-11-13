// Included by bridge_tests.m after its pure CF identity helpers. These checks
// exercise selection/registry contracts without remote AX calls or app launch.
static NSDictionary *testWindowMetadata(NSString *title, NSString *identifier, NSString *document) {
    return @{ @"title": title, @"identifier": identifier,
              @"document": document ?: (id)[NSNull null] };
}
static LLApplicationContext *testApplication(NSString *nonce) {
    LLApplicationContext *application = [LLApplicationContext new];
    application.nonce = nonce;
    return application;
}
static void runScopeTests(void) {
    NSString *executable = @"/Applications/Example.app/Contents/MacOS/Example";
    assert(scopeProcessCandidate(42, 0, geteuid(), executable, executable));
    assert(scopeProcessCandidate(42, 42, geteuid(), executable, executable));
    assert(!scopeProcessCandidate(42, 43, geteuid(), executable, executable));
    assert(!scopeProcessCandidate(42, 42, geteuid() + 1, executable, executable));
    assert(!scopeProcessCandidate(42, 42, geteuid(), @"/Applications/Other.app/Contents/MacOS/Other", executable));
    assert(!scopeProcessCandidate(0, 0, geteuid(), executable, executable));
    assert(!scopeProcessCandidate(-1, 0, geteuid(), executable, executable));
    assert(!scopeProcessCandidate(42, -1, geteuid(), executable, executable));
    assert(!scopeProcessCandidate(42, 0, geteuid(), nil, executable));
    NSString *ignoredExecutable = nil;
    assert(scopeApplicationExecutable(@"Example.app", &ignoredExecutable) == LL_AX_INVALID);
    assert(scopeApplicationExecutable(@"/Applications/Example", &ignoredExecutable) == LL_AX_INVALID);
    assert(scopeApplicationExecutable(@"/Applications/Example\0.app", &ignoredExecutable) == LL_AX_INVALID);
    puts("PASS explicit application executable, UID and PID candidate admission");

    BOOL admitted = NO;
    assert(scopeWindowRole(@"AXWindow", &admitted) == LL_AX_OK && admitted);
    assert(scopeWindowRole(@"AXScrollArea", &admitted) == LL_AX_OK && !admitted);
    assert(scopeWindowRole(@"AXSheet", &admitted) == LL_AX_OK && !admitted);
    admitted = YES;
    assert(scopeWindowRole(@"", &admitted) == LL_AX_INVALID && admitted);
    assert(scopeWindowRole(@"AXWindow\0", &admitted) == LL_AX_INVALID && admitted);
    assert(scopeWindowRole(nil, &admitted) == LL_AX_INVALID && admitted);
    AXError savedRoleError = attributeError;
    CFTypeRef savedRoleValue = attributeValue;
    attributeError = kAXErrorSuccess; attributeValue = CFSTR("AXScrollArea");
    NSDictionary *ignoredWindow = nil;
    size_t ignoredBytes = 0; int32_t roleDetail = 0;
    assert(scopeWindowMetadata(pureIdentity(@"desktop"), &ignoredWindow, &ignoredBytes, &roleDetail) == LL_AX_INVALID && !ignoredWindow);
    attributeError = kAXErrorCannotComplete;
    assert(scopeWindowMetadata(pureIdentity(@"window"), &ignoredWindow, &ignoredBytes, &roleDetail) == LL_AX_API && roleDetail == kAXErrorCannotComplete);
    attributeError = savedRoleError; attributeValue = savedRoleValue;
    puts("PASS window inventory excludes valid non-window roles while selected windows remain strict");

    NSString *document = nil, *canonical = nil;
    assert(scopeDocumentValue(CFSTR("https://example.invalid/document?q=one#two"), &document) == LL_AX_OK);
    assert([document isEqualToString:@"https://example.invalid/document?q=one#two"]);
    NSURL *remote = [NSURL URLWithString:@"https://example.invalid/document"];
    assert(scopeDocumentValue((__bridge CFURLRef)remote, &document) == LL_AX_OK && [document isEqualToString:remote.absoluteString]);
    assert(scopeDocumentValue(CFSTR("file://localhost/tmp/leselang-scope.txt"), &document) == LL_AX_OK);
    assert(canonicalFileURL(@"file:///tmp/leselang-scope.txt", &canonical) == LL_AX_OK && [document isEqualToString:canonical]);
    assert(scopeDocumentValue(CFSTR(""), &document) == LL_AX_OK && !document);
    for (NSString *bad in @[@"relative/document", @"file://remote/tmp/file", @"file:///tmp/file?query=1", @"file:///tmp/file%00.txt", @"https://example.invalid/%00"]) {
        assert(scopeDocumentValue((__bridge CFStringRef)bad, &document) == LL_AX_INVALID);
    }
    assert(scopeDocumentValue(kCFBooleanTrue, &document) == LL_AX_INVALID);
    assert(scopeDocumentValue(NULL, &document) == LL_AX_INVALID);
    NSString *oversized = [@"https://example.invalid/" stringByPaddingToLength:4097 withString:@"x" startingAtIndex:0];
    assert(scopeDocumentValue((__bridge CFStringRef)oversized, &document) == LL_AX_BOUNDS);
    puts("PASS bounded window document URLs without treating remote URLs as local files");

    LLApplicationContext *application = testApplication(@"00000000-0000-0000-0000-000000000001");
    LLCF *first = pureIdentity(@"window-a"), *second = pureIdentity(@"window-b");
    LLWindowEntry *selected = nil;
    assert(scopeCommitWindows(application, @[], @[], 0) == LL_AX_OK);
    assert(scopeSelect(application, @"", @"Missing", @"", @"", &selected) == LL_AX_UNAVAILABLE);
    assert(scopeSelect(application, @"", @"", @"", @"", &selected) == LL_AX_INVALID);
    NSArray *metadata = @[testWindowMetadata(@"Same title", @"editor-a", nil), testWindowMetadata(@"Same title", @"editor-b", canonical)];
    assert(scopeCommitWindows(application, @[first, second], metadata, 0) == LL_AX_OK);
    NSString *firstKey = application.windowRegistry[0].key;
    uint64_t firstIdentity = application.windowRegistry[0].identity;
    assert(firstIdentity && firstKey.length <= 57);
    assert(scopeSelect(application, @"", @"Same title", @"", @"", &selected) == LL_AX_INVALID);
    assert(scopeSelect(application, @"", @"Same title", @"editor-a", @"", &selected) == LL_AX_OK && selected.identity == firstIdentity);
    assert(scopeSelect(application, @"", @"Same title", @"editor-a", canonical, &selected) == LL_AX_UNAVAILABLE);
    assert(scopeSelect(application, @"", @"", @"", canonical, &selected) == LL_AX_OK && sameElement(selected.element, second));
    assert(scopeSelect(application, firstKey, @"", @"", @"", &selected) == LL_AX_OK && sameElement(selected.element, first));
    assert(scopeSelect(application, firstKey, @"Same title", @"", @"", &selected) == LL_AX_INVALID);
    assert(scopeSelect(application, @"", @"Same", @"", @"", &selected) == LL_AX_UNAVAILABLE);
    puts("PASS exact combined window selectors, empty inventory and duplicate-title refusal");

    NSArray *renamed = @[testWindowMetadata(@"Renamed note", @"editor-a", nil), metadata[1]];
    assert(scopeCommitWindows(application, @[first, second], renamed, 0) == LL_AX_OK);
    assert(application.windowRegistry[0].identity == firstIdentity && [application.windowRegistry[0].key isEqualToString:firstKey]);
    assert(scopeSelect(application, firstKey, @"", @"", @"", &selected) == LL_AX_OK && selected.identity == firstIdentity);
    assert(scopeSelect(application, @"", @"Same title", @"editor-a", @"", &selected) == LL_AX_UNAVAILABLE);
    assert(scopeSelect(application, @"", @"Renamed note", @"editor-a", @"", &selected) == LL_AX_OK && selected.identity == firstIdentity);
    assert(scopeCommitWindows(application, @[second, pureIdentity(@"window-a")], @[renamed[1], renamed[0]], 0) == LL_AX_OK);
    assert(application.windowRegistry.count == 2 && application.windowRegistry[0].identity == firstIdentity);
    LLApplicationContext *otherApplication = testApplication(@"00000000-0000-0000-0000-000000000002");
    assert(scopeCommitWindows(otherApplication, @[first], @[renamed[0]], 0) == LL_AX_OK);
    assert(scopeSelect(otherApplication, firstKey, @"", @"", @"", &selected) == LL_AX_UNAVAILABLE);
    assert(scopeCommitWindows(application, @[second], @[renamed[1]], 0) == LL_AX_OK);
    assert(!application.windowRegistry[0].alive);
    assert(scopeSelect(application, firstKey, @"", @"", @"", &selected) == LL_AX_UNAVAILABLE);
    assert(scopeCommitWindows(application, @[first, second], renamed, 0) == LL_AX_OK);
    assert(application.windowRegistry.count == 3 && !application.windowRegistry[0].alive);
    assert(application.windowRegistry[2].identity != firstIdentity && ![application.windowRegistry[2].key isEqualToString:firstKey]);
    assert(scopeSelect(application, firstKey, @"", @"", @"", &selected) == LL_AX_UNAVAILABLE);
    puts("PASS window keys survive rename/reorder, remain connection-local and never revive");

    LLCF *pinned = nil;
    assert(controlsPinnedWindow(first, @[first, second], &pinned) == LL_AX_OK && sameElement(pinned, first));
    assert(controlsPinnedWindow(first, @[second, pureIdentity(@"window-a")], &pinned) == LL_AX_OK && sameElement(pinned, first));
    // Titles are lookup metadata only: the pinned matcher accepts no selector
    // input and cannot choose the same-title replacement after the old one dies.
    LLCF *replacement = pureIdentity(@"replacement-window-a");
    pinned = first;
    assert(controlsPinnedWindow(first, @[replacement, second], &pinned) == LL_AX_STALE && pinned == first);
    assert(controlsPinnedWindow(first, @[], &pinned) == LL_AX_STALE && pinned == first);
    assert(controlsPinnedWindow(first, @[first, pureIdentity(@"window-a")], &pinned) == LL_AX_INVALID);
    assert(controlsPinnedWindow(first, @[first, second, pureIdentity(@"window-b")], &pinned) == LL_AX_INVALID);
    assert(controlsPinnedWindow(nil, @[first], &pinned) == LL_AX_INVALID);
    // AXDocument can name a directory package; it is still URL metadata and
    // uses the same exact canonical comparison as a regular document file.
    NSString *directoryURL = [NSURL fileURLWithPath:NSTemporaryDirectory() isDirectory:YES].absoluteString;
    assert(scopeDocumentValue((__bridge CFStringRef)directoryURL, &document) == LL_AX_OK);
    assert(canonicalFileURL(directoryURL, &canonical) == LL_AX_OK && [document isEqualToString:canonical]);
    LLControlsContext *documentControls = [LLControlsContext new];
    documentControls.identity = 1;
    documentControls.nonce = @"document-dependency";
    NSArray *unchangedTree = @[first];
    assert(controlsCommit(documentControls, @[], @[], @[@{@"document": @"https://example.invalid/one"}], unchangedTree, @[@0], 0) == LL_AX_OK);
    assert(documentControls.revision == 1);
    assert(controlsCommit(documentControls, @[], @[], @[@{@"document": @"https://example.invalid/two"}], unchangedTree, @[@0], 0) == LL_AX_OK);
    assert(documentControls.revision == 2);
    puts("PASS pinned window membership, replacements and document-only dependency changes");

    NSArray *savedRows = application.rows;
    assert(scopeCommitWindows(application, @[first, first], @[metadata[0], metadata[0]], 0) == LL_AX_INVALID);
    assert(scopeCommitWindows(application, @[first], @[], 0) == LL_AX_INVALID);
    assert(scopeCommitWindows(application, @[], @[], LL_WINDOWS_BYTES_LIMIT + 1) == LL_AX_BOUNDS);
    assert(application.rows == savedRows);
    NSMutableArray *tooManyWindows = [NSMutableArray new], *tooManyRows = [NSMutableArray new];
    for (NSUInteger index = 0; index <= LL_WINDOWS_LIMIT; ++index) {
        [tooManyWindows addObject:pureIdentity([NSString stringWithFormat:@"window-%lu", (unsigned long)index])];
        [tooManyRows addObject:metadata[0]];
    }
    assert(scopeCommitWindows(application, tooManyWindows, tooManyRows, 0) == LL_AX_BOUNDS && application.rows == savedRows);
    LLApplicationContext *exhausted = testApplication(@"00000000-0000-0000-0000-000000000003");
    NSMutableArray *retired = [NSMutableArray new];
    for (NSUInteger index = 0; index < LL_WINDOWS_REGISTRY_LIMIT; ++index) {
        LLWindowEntry *entry = [LLWindowEntry new];
        entry.element = pureIdentity([NSString stringWithFormat:@"retired-window-%lu", (unsigned long)index]);
        [retired addObject:entry];
    }
    exhausted.windowRegistry = retired;
    assert(scopeCommitWindows(exhausted, @[first], @[metadata[0]], 0) == LL_AX_BOUNDS && exhausted.rows == nil);
    uint64_t savedNextIdentity = nextIdentity;
    nextIdentity = UINT64_MAX;
    assert(scopeCommitWindows(otherApplication, @[replacement], @[metadata[0]], 0) == LL_AX_BOUNDS);
    assert(otherApplication.windowRegistry.count == 1 && otherApplication.windowRegistry[0].alive);
    nextIdentity = savedNextIdentity;
    puts("PASS bounded window inventories, lifetime registry and identity exhaustion");

    LLWindowRow rows[LL_WINDOWS_LIMIT];
    uint8_t output[LL_WINDOWS_BYTES_LIMIT];
    memset(rows, 0x5a, sizeof(rows)); memset(output, 0x5a, sizeof(output));
    LLWindowInventory inventory = {71, 72};
    assert(scopePublishWindows(application, rows, 0, output, sizeof(output), &inventory) == LL_AX_BOUNDS);
    assert(scopePublishWindows(application, rows, LL_WINDOWS_LIMIT, output, 1, &inventory) == LL_AX_BOUNDS);
    assert(inventory.count == 71 && inventory.bytes_length == 72);
    for (size_t index = 0; index < sizeof(rows); ++index) assert(((uint8_t *)rows)[index] == 0x5a);
    for (size_t index = 0; index < sizeof(output); ++index) assert(output[index] == 0x5a);
    assert(scopePublishWindows(application, rows, LL_WINDOWS_LIMIT, output, sizeof(output), &inventory) == LL_AX_OK);
    assert(inventory.count == 2 && inventory.bytes_length > 0 && rows[0].identity < rows[1].identity);
    assert(rows[0].document_present == 1 && rows[1].document_present == 0);
    LLApplicationContext *empty = testApplication(@"empty");
    assert(scopeCommitWindows(empty, @[], @[], 0) == LL_AX_OK);
    assert(scopePublishWindows(empty, rows, LL_WINDOWS_LIMIT, output, sizeof(output), &inventory) == LL_AX_OK && inventory.count == 0 && inventory.bytes_length == 0);
    int32_t detail = 0, actualPID = 17;
    void *connection = (void *)0x1234;
    assert(ll_application_connect(NULL, 0, -1, &connection, &actualPID, &detail) == LL_AX_INVALID);
    assert(connection == (void *)0x1234 && actualPID == 17);
    assert(ll_application_connect(NULL, 0, 0, NULL, &actualPID, &detail) == LL_AX_INVALID);
    assert(ll_application_windows(NULL, rows, LL_WINDOWS_LIMIT, output, sizeof(output), &inventory, &detail) == LL_AX_INVALID);
    assert(ll_application_controls(NULL, NULL, 0, NULL, 0, NULL, 0, NULL, 0, &connection, &detail) == LL_AX_INVALID);
    puts("PASS staged window publication and invalid-handle FFI refusal");
}
