// Local process and AX APIs are intercepted in bridge_tests.m.
static int auxiliaryGrantCalls = 0;
static int32_t testAuxiliaryGrant(const void *context) {
    ++auxiliaryGrantCalls; return *(const BOOL *)context ? 1 : 0;
}
static LLCF *testAXReference(pid_t pid) {
    LLCF *reference = [LLCF new]; reference->value = AXUIElementCreateApplication(pid); return reference;
}
static LLAuxiliarySpec testAuxiliarySpec(pid_t pid) {
    static const uint8_t path[] = "/usr/bin/true";
    return (LLAuxiliarySpec){pid, 1000, 123, path, sizeof(path) - 1};
}
static void resetAuxiliaryProcesses(void) {
    for (size_t index = 0; index < 5; ++index) processFixtures[index] = (LLTestProcess){100 + (pid_t)index, geteuid(), 1000, 123, "/usr/bin/true"};
    processInfoCalls = 0; processPathCalls = 0; processReadFault = 0;
}
static void runAuxiliaryTests(void) {
    resetAuxiliaryProcesses();
    BOOL granted = YES; int32_t detail = 0;
    LLAuxiliarySpec spec = testAuxiliarySpec(101);
    LLAuxiliaryPolicy *policy = nil;
    assert(auxiliaryPolicy(100, &spec, 1, testAuxiliaryGrant, &granted, &policy, &detail) == LL_AX_OK);
    assert(policy.processes.count == 1 && auxiliaryAllowedPID(policy, 100, 101));
    assert(auxiliaryAllowedPID(policy, 100, 100) && !auxiliaryAllowedPID(policy, 100, 102));
    assert(!auxiliaryAllowedPID(nil, 100, 101) && !auxiliaryAllowedPID(policy, 100, 0));
    LLAuxiliaryPolicy *untouchedPolicy = policy;
    LLAuxiliarySpec invalid = spec; invalid.pid = 100;
    assert(auxiliaryPolicy(100, &invalid, 1, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID && untouchedPolicy == policy);
    invalid = spec; invalid.start_micros = 1000000;
    assert(auxiliaryPolicy(100, &invalid, 1, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID);
    invalid = spec; invalid.start_seconds = 0;
    assert(auxiliaryPolicy(100, &invalid, 1, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID);
    LLAuxiliarySpec duplicate[] = {spec, spec};
    assert(auxiliaryPolicy(100, duplicate, 2, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID);
    assert(auxiliaryPolicy(100, &spec, 0, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID);
    assert(auxiliaryPolicy(100, &spec, 5, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID);
    assert(auxiliaryPolicy(100, &spec, 1, NULL, &granted, &untouchedPolicy, &detail) == LL_AX_INVALID);
    assert(auxiliaryCanonicalPath(@"relative") == LL_AX_INVALID);
    assert(auxiliaryCanonicalPath(@"/usr/bin/../bin/true") == LL_AX_INVALID);
    assert(auxiliaryCanonicalPath(@"/usr/bin/true\0") == LL_AX_INVALID);
    assert(auxiliaryCanonicalPath(@"/usr/bin/true") == LL_AX_OK);
    puts("PASS explicit auxiliary identities reject aliases, primary PID, duplicates and invalid bounds");

    for (int fault = 1; fault <= 4; ++fault) {
        resetAuxiliaryProcesses(); processReadFault = fault;
        LLAuxiliaryProcess *process = nil;
        assert(auxiliaryReadProcess(101, &process) == LL_AX_STALE && !process);
    }
    resetAuxiliaryProcesses(); invalid = spec; ++invalid.start_seconds;
    assert(auxiliaryPolicy(100, &invalid, 1, testAuxiliaryGrant, &granted, &untouchedPolicy, &detail) == LL_AX_STALE);
    processFixtures[1].uid = geteuid() + 1;
    assert(auxiliaryCheckProcesses(policy.processes, &detail) == LL_AX_STALE);
    resetAuxiliaryProcesses(); processFixtures[1].path = "/usr/bin/false";
    assert(auxiliaryCheckProcesses(policy.processes, &detail) == LL_AX_STALE);
    resetAuxiliaryProcesses(); ++processFixtures[1].micros;
    assert(auxiliaryCheckProcesses(policy.processes, &detail) == LL_AX_STALE);
    resetAuxiliaryProcesses(); processFixtures[1].pid = 0; detail = kAXErrorAttributeUnsupported;
    assert(auxiliaryCheckProcesses(policy.processes, &detail) == LL_AX_STALE && detail == 0);
    resetAuxiliaryProcesses(); assert(auxiliaryCheckProcesses(policy.processes, &detail) == LL_AX_OK);
    puts("PASS double process reads reject exit, PID reuse, executable drift and UID changes");

    LLCF *applicationRef = testAXReference(100), *windowRef = testAXReference(200), *foreignRef = testAXReference(101);
    auxiliaryApp = (AXUIElementRef)applicationRef->value; auxiliaryWindow = (AXUIElementRef)windowRef->value; auxiliaryForeign = (AXUIElementRef)foreignRef->value;
    auxiliaryTreeActive = YES; auxiliaryForeignReads = 0;
    LLApplicationContext *application = [LLApplicationContext new];
    application.pid = 100; application.application = applicationRef;
    application.executable = @"/usr/bin/true"; application.startSeconds = 1000; application.startMicros = 123;
    NSArray<LLAuxiliaryProcess *> *frontier = nil;
    assert(auxiliaryFrontier(application, windowRef, &frontier, &detail) == LL_AX_OK);
    assert(frontier.count == 1 && frontier[0].pid == 101 && frontier[0].edgeCount == 1 && auxiliaryForeignReads == 0);
    processFixtures[1].uid = geteuid() + 1;
    NSArray<LLAuxiliaryProcess *> *untouchedFrontier = frontier;
    assert(auxiliaryFrontier(application, windowRef, &untouchedFrontier, &detail) == LL_AX_STALE && untouchedFrontier == frontier && auxiliaryForeignReads == 0);
    resetAuxiliaryProcesses();
    puts("PASS frontier discovery stops before every foreign AX property and subtree");

    AXError savedError = attributeError; CFTypeRef savedValue = attributeValue;
    auxiliaryAllowParentRead = YES; attributeError = kAXErrorSuccess; attributeValue = windowRef->value;
    assert(auxiliaryElement(policy, 100, foreignRef, windowRef, YES, &detail) == LL_AX_OK && auxiliaryForeignReads == 1);
    assert(auxiliaryElement(nil, 100, foreignRef, windowRef, YES, &detail) == LL_AX_STALE && auxiliaryForeignReads == 1);
    attributeValue = applicationRef->value;
    assert(auxiliaryElement(policy, 100, foreignRef, windowRef, YES, &detail) == LL_AX_STALE);
    attributeValue = CFSTR("invalid parent");
    assert(auxiliaryElement(policy, 100, foreignRef, windowRef, YES, &detail) == LL_AX_STALE);
    attributeError = kAXErrorCannotComplete;
    assert(auxiliaryElement(policy, 100, foreignRef, windowRef, YES, &detail) == LL_AX_API && detail == kAXErrorCannotComplete);
    int parentReads = auxiliaryForeignReads;
    assert(auxiliaryElement(policy, 100, windowRef, applicationRef, YES, &detail) == LL_AX_OK && auxiliaryForeignReads == parentReads);
    assert(auxiliaryElement(policy, 100, foreignRef, windowRef, NO, &detail) == LL_AX_OK && auxiliaryForeignReads == parentReads);
    auxiliaryAllowParentRead = NO; auxiliaryTreeActive = NO; attributeError = savedError; attributeValue = savedValue;
    puts("PASS cross-process child edges require the exact observed parent and an explicit PID grant");

    LLAuxiliaryRow rows[LL_AUXILIARY_LIMIT]; uint8_t bytes[128];
    memset(rows, 0x5a, sizeof(rows)); memset(bytes, 0x5a, sizeof(bytes));
    LLAuxiliaryInventory inventory = {77, 88};
    assert(auxiliaryPublish(frontier, rows, 0, bytes, sizeof(bytes), &inventory) == LL_AX_BOUNDS);
    assert(auxiliaryPublish(frontier, rows, LL_AUXILIARY_LIMIT, bytes, 1, &inventory) == LL_AX_BOUNDS);
    assert(inventory.count == 77 && inventory.bytes_length == 88);
    for (size_t index = 0; index < sizeof(rows); ++index) assert(((uint8_t *)rows)[index] == 0x5a);
    for (size_t index = 0; index < sizeof(bytes); ++index) assert(bytes[index] == 0x5a);
    assert(auxiliaryPublish(frontier, rows, LL_AUXILIARY_LIMIT, bytes, sizeof(bytes), &inventory) == LL_AX_OK);
    assert(inventory.count == 1 && rows[0].pid == 101 && rows[0].start_seconds == 1000 && rows[0].start_micros == 123 && rows[0].edge_count == 1);
    assert(inventory.bytes_length == strlen("/usr/bin/true") && memcmp(bytes, "/usr/bin/true", inventory.bytes_length) == 0);
    assert(ll_application_auxiliary_processes(NULL, NULL, 0, NULL, 0, NULL, 0, NULL, 0, rows, LL_AUXILIARY_LIMIT, bytes, sizeof(bytes), &inventory, &detail) == LL_AX_INVALID);
    puts("PASS bounded auxiliary metadata publishes atomically after complete validation");

    LLWidgetsContext *revocable = [LLWidgetsContext new]; revocable.auxiliary = policy;
    granted = NO;
    assert(widgetsPermission(revocable, &detail) == LL_AX_PERMISSION && revocable.retired && policy.revoked);
    granted = YES; assert(widgetsPermission(revocable, &detail) == LL_AX_PERMISSION);
    LLAuxiliaryPolicy *fresh = [LLAuxiliaryPolicy new];
    fresh.check = testAuxiliaryGrant; fresh.grantContext = &granted; fresh.processes = policy.processes;
    LLWidgetsContext *explicitRevocation = [LLWidgetsContext new]; explicitRevocation.auxiliary = fresh;
    int callsBeforeThread = auxiliaryGrantCalls;
    dispatch_semaphore_t completed = dispatch_semaphore_create(0);
    __block int permissionResult = 0, revokeResult = 0;
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
        int32_t threadDetail = 0;
        permissionResult = ll_widgets_permission((__bridge void *)explicitRevocation, &threadDetail);
        revokeResult = ll_widgets_revoke((__bridge void *)explicitRevocation, &threadDetail);
        dispatch_semaphore_signal(completed);
    });
    assert(dispatch_semaphore_wait(completed, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) == 0);
    assert(permissionResult == LL_AX_WRONG_THREAD && revokeResult == LL_AX_WRONG_THREAD && auxiliaryGrantCalls == callsBeforeThread && !fresh.revoked);
    assert(ll_widgets_revoke((__bridge void *)explicitRevocation, &detail) == LL_AX_OK && fresh.revoked && explicitRevocation.retired);
    assert(ll_widgets_permission((__bridge void *)explicitRevocation, &detail) == LL_AX_PERMISSION);
    assert(ll_widgets_revoke(NULL, &detail) == LL_AX_INVALID);
    puts("PASS auxiliary revocation is permanent and wrong-thread calls never read the grant callback");

    LLWidgetsContext *ownershipEpoch = [LLWidgetsContext new]; ownershipEpoch.nonce = @"aux-test";
    LLCF *widget = pureIdentity(@"unchanged-widget");
    NSMutableDictionary *state = [pureWidget(1, 0, @"AXTextField") mutableCopy]; state[@"ownerPID"] = @100;
    assert(controlsCommitBounded(ownershipEpoch, @[widget], @[state], @[], @[widget], @[@0], 0, LL_WIDGET_NODE_LIMIT) == LL_AX_OK);
    uint64_t oldIdentity = ownershipEpoch.registry[0].identity; assert(ownershipEpoch.revision == 1);
    state[@"ownerPID"] = @101;
    assert(controlsCommitBounded(ownershipEpoch, @[widget], @[state], @[], @[widget], @[@0], 0, LL_WIDGET_NODE_LIMIT) == LL_AX_OK);
    assert(ownershipEpoch.revision == 2 && ownershipEpoch.registry[0].identity == oldIdentity);
    NSDictionary *dependency = @{@"role": @"AXGroup", @"ownerPID": @100};
    assert(controlsCommitBounded(ownershipEpoch, @[widget], @[state], @[dependency], @[widget], @[@0], 0, LL_WIDGET_NODE_LIMIT) == LL_AX_OK);
    dependency = @{@"role": @"AXGroup", @"ownerPID": @101};
    assert(controlsCommitBounded(ownershipEpoch, @[widget], @[state], @[dependency], @[widget], @[@0], 0, LL_WIDGET_NODE_LIMIT) == LL_AX_OK && ownershipEpoch.revision == 4);
    puts("PASS widget and structural-node ownership changes advance scope revisions");
    auxiliaryApp = NULL; auxiliaryWindow = NULL; auxiliaryForeign = NULL;
}
