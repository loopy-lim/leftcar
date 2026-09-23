// CGVirtualDisplayBridge: 네이티브 가상(확장) 디스플레이 생성 브리지.
//
// CGVirtualDisplay 계열은 CoreGraphics의 private 클래스다(공개 헤더 없음).
// 그래서 모든 진입은 NSClassFromString + objc_msgSend 캐스트로 런타임에
// 일어난다: 클래스가 없거나 셀렉터가 바뀌면 "지원 안 됨"으로 판정될 뿐
// 로드 타임 크래시는 없다. 시그니처는 이 저장소에서 macOS 26.6.2 실측
// 레시피(tools/cgvd-spark, 2026-09-03)로 검증된 CGVD.h 계열을 따른다.
//
// 수명 모델: 파괴 호출이 없으므로 CGVirtualDisplay 객체를 레지스트리가
// 강하게 보유한다 — 객체 해제(destroy)가 곧 디스플레이 제거다. 프로세스
// 사망 시 WindowServer가 회수하므로 스테일 디스플레이는 남지 않는다.
//
// 반환 문자열은 strdup된 JSON이며 호출자(Rust/Swift)가 free 한다 —
// leftcar_capture_free_string과 같은 규약.

#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>
#import <objc/message.h>
#import <objc/runtime.h>
#import <dlfcn.h>
#import <stdio.h>
#import <stdlib.h>
#import <string.h>

// 공개 헤더에 선언이 없는 CoreGraphics 심볼(Swift 쪽 stableDisplaySourceID가
// 전이 임포트로 쓰는 것과 동일한 함수). 심볼 자체는 libCG에 존재한다.
extern CFUUIDRef CGDisplayCreateUUIDFromDisplayID(uint32_t display);

// Let ARC see the init method family. Calling init through an unannotated C
// function pointer loses its retained-return/consumed-self convention and can
// keep a display alive after its registry entry is removed.
@interface NSObject (LeftcarVirtualDisplayInitializers)
- (instancetype)initWithDescriptor:(id)descriptor;
- (instancetype)initWithWidth:(unsigned long)width height:(unsigned long)height refreshRate:(double)refresh;
@end

#pragma mark - 런타임 클래스/셀렉터 바인딩

static Class VDClass(void) { return NSClassFromString(@"CGVirtualDisplay"); }
static Class VDDescriptorClass(void) { return NSClassFromString(@"CGVirtualDisplayDescriptor"); }
static Class VDSettingsClass(void) { return NSClassFromString(@"CGVirtualDisplaySettings"); }
static Class VDModeClass(void) { return NSClassFromString(@"CGVirtualDisplayMode"); }

// 안전한 objc_msgSend 캐스트 — 타입이 다른 후보를 실수로 섞지 않게 케이스별 헬퍼.
static BOOL MsgApplySettings(id obj, id settings) {
    return ((BOOL (*)(id, SEL, id))objc_msgSend)(obj, sel_registerName("applySettings:"), settings);
}
static uint32_t MsgDisplayID(id obj) {
    return ((uint32_t (*)(id, SEL))objc_msgSend)(obj, sel_registerName("displayID"));
}
static void MsgSetName(id obj, NSString *value) {
    ((void (*)(id, SEL, id))objc_msgSend)(obj, sel_registerName("setName:"), value);
}
static void MsgSetVendorID(id obj, uint32_t value) {
    ((void (*)(id, SEL, uint32_t))objc_msgSend)(obj, sel_registerName("setVendorID:"), value);
}
static void MsgSetProductID(id obj, uint32_t value) {
    ((void (*)(id, SEL, uint32_t))objc_msgSend)(obj, sel_registerName("setProductID:"), value);
}
static void MsgSetSerialNum(id obj, uint32_t value) {
    ((void (*)(id, SEL, uint32_t))objc_msgSend)(obj, sel_registerName("setSerialNum:"), value);
}
static void MsgSetMaxPixelsWide(id obj, uint32_t value) {
    ((void (*)(id, SEL, uint32_t))objc_msgSend)(obj, sel_registerName("setMaxPixelsWide:"), value);
}
static void MsgSetMaxPixelsHigh(id obj, uint32_t value) {
    ((void (*)(id, SEL, uint32_t))objc_msgSend)(obj, sel_registerName("setMaxPixelsHigh:"), value);
}
static void MsgSetSizeInMillimeters(id obj, CGSize value) {
    ((void (*)(id, SEL, CGSize))objc_msgSend)(obj, sel_registerName("setSizeInMillimeters:"), value);
}
static void MsgSetDispatchQueue(id obj, dispatch_queue_t value) {
    ((void (*)(id, SEL, dispatch_queue_t))objc_msgSend)(
        obj, sel_registerName("setDispatchQueue:"), value);
}
static void MsgSetModes(id obj, NSArray *value) {
    ((void (*)(id, SEL, id))objc_msgSend)(obj, sel_registerName("setModes:"), value);
}
static void MsgSetHiDPI(id obj, uint32_t value) {
    ((void (*)(id, SEL, uint32_t))objc_msgSend)(obj, sel_registerName("setHiDPI:"), value);
}

// 심볼 존재 검사(프로브용): 클래스 4개 + 핵심 셀렉터 전부.
static BOOL VirtualDisplaySelectorsPresent(void) {
    Class display = VDClass();
    Class descriptor = VDDescriptorClass();
    Class settings = VDSettingsClass();
    Class mode = VDModeClass();
    if (!display || !descriptor || !settings || !mode) return NO;
    SEL checks[] = {
        sel_registerName("initWithDescriptor:"),
        sel_registerName("applySettings:"),
        sel_registerName("displayID"),
    };
    for (size_t i = 0; i < sizeof(checks) / sizeof(checks[0]); i++) {
        if (!class_getInstanceMethod(display, checks[i])) return NO;
    }
    SEL descriptorChecks[] = {
        sel_registerName("setName:"),
        sel_registerName("setVendorID:"),
        sel_registerName("setSerialNum:"),
        sel_registerName("setMaxPixelsWide:"),
        sel_registerName("setMaxPixelsHigh:"),
        sel_registerName("setSizeInMillimeters:"),
        sel_registerName("setDispatchQueue:"),
    };
    for (size_t i = 0; i < sizeof(descriptorChecks) / sizeof(descriptorChecks[0]); i++) {
        if (!class_getInstanceMethod(descriptor, descriptorChecks[i])) return NO;
    }
    SEL settingsChecks[] = {
        sel_registerName("setModes:"),
        sel_registerName("setHiDPI:"),
    };
    for (size_t i = 0; i < sizeof(settingsChecks) / sizeof(settingsChecks[0]); i++) {
        if (!class_getInstanceMethod(settings, settingsChecks[i])) return NO;
    }
    if (!class_getInstanceMethod(mode, sel_registerName("initWithWidth:height:refreshRate:"))) {
        return NO;
    }
    return YES;
}

#pragma mark - 상태 레지스트리 (displayID -> 강한 참조)

// 슬롯당 고정 identity: macOS가 (vendor, product, serial) 키로 배치·모드를
// 기억하므로, 고정 시리얼은 재생성을 "같은 모니터"로 인식시켜 어레인지먼트
// 연속성을 주고 ICC 프로필 누적도 1개로 막는다. v1은 단일 슬롯.
static const uint32_t kLeftcarVdispVendor = 0x4C50u;  // 'LP'
static const uint32_t kLeftcarVdispProduct = 0x4C43u; // 'LC'
static const uint32_t kLeftcarVdispSerial = 0x4C430001u;

static NSMutableDictionary *VDRegistry(void) {
    static NSMutableDictionary *registry;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        registry = [NSMutableDictionary new];
    });
    return registry;
}

static NSMutableDictionary *VDModes(void) {
    static NSMutableDictionary *modes;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ modes = [NSMutableDictionary new]; });
    return modes;
}

static dispatch_queue_t VDQueue(void) {
    static dispatch_queue_t queue;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        queue = dispatch_queue_create("dev.leftcar.vdisp", DISPATCH_QUEUE_SERIAL);
    });
    return queue;
}

static NSString *UUIDForDisplayID(CGDirectDisplayID displayID) {
    CFUUIDRef uuid = CGDisplayCreateUUIDFromDisplayID(displayID);
    if (!uuid) return nil;
    CFStringRef string = CFUUIDCreateString(nil, uuid);
    NSString *result = string ? (__bridge_transfer NSString *)string : nil;
    CFRelease(uuid);
    return result;
}

static BOOL DisplayIsActiveInList(CGDirectDisplayID displayID) {
    uint32_t count = 0;
    if (CGGetActiveDisplayList(0, nil, &count) != kCGErrorSuccess) return NO;
    if (count == 0) return NO;
    CGDirectDisplayID ids[count];
    uint32_t filled = 0;
    if (CGGetActiveDisplayList(count, ids, &filled) != kCGErrorSuccess) return NO;
    for (uint32_t i = 0; i < filled; i++) {
        if (ids[i] == displayID) return YES;
    }
    return NO;
}

// 짧은 간격 폴링 — WindowServer 등록/모드 적용/제거는 비동기다(실측 레시피).
// 대기는 런 루프를 돌리며 한다: 재구성 통지는 런 루프를 통해서만 도착하고,
// usleep으로 막으면 모드 조회가 오래된 값을 계속 돌려준다(실측 — 폴링이
// 영원히 실패하는 현상의 원인).
static BOOL PollUntil(double seconds, BOOL (^check)(void)) {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while (YES) {
        if (check()) return YES;
        if ([[NSDate date] compare:deadline] == NSOrderedDescending) return NO;
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode
                                  beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    }
}

static char *CopyJSONString(NSDictionary *payload) {
    NSError *error = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:payload options:0 error:&error];
    if (!data) {
        const char *fallback = "{\"ok\":false,\"error\":\"json-encoding-failed\"}";
        return strdup(fallback);
    }
    return strdup([[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] UTF8String]);
}

#pragma mark - 내부 헬퍼: 배치(미러 해제 포함)

// Keep the same mode in the placement transaction when the cached mode agrees
// with live bounds. After recreation the mode cache can be empty or stale; an
// origin-only transaction is safer than selecting a different resolution.
static int VDPlaceOriginAttempt(CGDirectDisplayID displayID, int32_t x, int32_t y) {
    CGDirectDisplayID mainBefore = CGMainDisplayID();
    CGRect before = CGDisplayBounds(displayID);
    if ((int32_t)before.origin.x == x && (int32_t)before.origin.y == y && !CGDisplayIsInMirrorSet(displayID)) return 0;
    CGDisplayConfigRef config = NULL;
    if (CGBeginDisplayConfiguration(&config) != kCGErrorSuccess) return 2;
    if ((CGDisplayIsInMirrorSet(displayID) &&
         CGConfigureDisplayMirrorOfDisplay(config, displayID, kCGNullDirectDisplay) != kCGErrorSuccess) ||
        CGConfigureDisplayOrigin(config, displayID, x, y) != kCGErrorSuccess) {
        CGCancelDisplayConfiguration(config);
        return 2;
    }
    CGDisplayModeRef mode = CGDisplayCopyDisplayMode(displayID);
    if (mode) {
        NSDictionary *expected;
        @synchronized (VDRegistry()) { expected = VDModes()[@(displayID)]; }
        size_t expectedScale = [expected[@"scale"] unsignedLongValue];
        BOOL sameSize = CGDisplayModeGetWidth(mode) == (size_t)before.size.width
            && CGDisplayModeGetHeight(mode) == (size_t)before.size.height
            && expected != nil
            && CGDisplayModeGetPixelWidth(mode) == [expected[@"width"] unsignedLongValue] * expectedScale
            && CGDisplayModeGetPixelHeight(mode) == [expected[@"height"] unsignedLongValue] * expectedScale;
        CGError modeResult = sameSize
            ? CGConfigureDisplayWithDisplayMode(config, displayID, mode, NULL)
            : kCGErrorSuccess;
        CFRelease(mode);
        if (modeResult != kCGErrorSuccess) { CGCancelDisplayConfiguration(config); return 2; }
    }
    CGError complete = CGCompleteDisplayConfiguration(config, kCGConfigureForSession);
    if (complete != kCGErrorSuccess) return 2;
    BOOL ok = PollUntil(2.0, ^BOOL(void) {
        CGRect bounds = CGDisplayBounds(displayID);
        return (int32_t)bounds.origin.x == x && (int32_t)bounds.origin.y == y;
    });
    if (CGMainDisplayID() != mainBefore) return 4;
    return ok ? 0 : 3;
}

static int VDPlaceOrigin(CGDirectDisplayID displayID, int32_t x, int32_t y) {
    int result = VDPlaceOriginAttempt(displayID, x, y);
    // WindowServer can normalize the first origin using the previous desktop
    // size. Once that transaction settles, one more position-only intent closes
    // the gap. Never loop indefinitely or retry a primary-display change.
    return result == 3 ? VDPlaceOriginAttempt(displayID, x, y) : result;
}

#pragma mark - 공개 C 심볼

char *leftcar_vdisp_probe_v1(void) {
    if (!VirtualDisplaySelectorsPresent()) {
        return CopyJSONString(@{@"supported": @NO, @"reason": @"unsupported-classes"});
    }
    // 자동화 셸(SSH 등)에는 GUI 세션이 없어 생성이 불가능하다(실측 전제).
    CFDictionaryRef session = CGSessionCopyCurrentDictionary();
    if (!session) {
        return CopyJSONString(@{@"supported": @NO, @"reason": @"no-gui-session"});
    }
    CFRelease(session);
    uint32_t count = 0;
    if (CGGetActiveDisplayList(0, nil, &count) != kCGErrorSuccess || count == 0) {
        return CopyJSONString(@{@"supported": @NO, @"reason": @"no-active-display"});
    }
    return CopyJSONString(@{@"supported": @YES, @"reason": @"", @"activeDisplays": @(count)});
}

char *leftcar_vdisp_create_v1(uint32_t logical_width, uint32_t logical_height,
                              uint32_t scale, const char *name) {
    CFAbsoluteTime started = CFAbsoluteTimeGetCurrent();
    uint32_t backing_width = logical_width * scale;
    uint32_t backing_height = logical_height * scale;
    NSDictionary *(^failure)(NSString *, NSString *) = ^NSDictionary *(NSString *stage, NSString *error) {
        return @{
            @"ok": @NO,
            @"stage": stage,
            @"error": error,
        };
    };
    if (scale != 1 && scale != 2) {
        return CopyJSONString(failure(@"validate", @"scale must be 1 or 2"));
    }
    if (logical_width < 640 || logical_height < 480 || logical_width > 4096 || logical_height > 4096) {
        return CopyJSONString(failure(@"validate", @"logical size out of range"));
    }
    if (backing_width > 8192 || backing_height > 8192) {
        return CopyJSONString(failure(@"validate", @"backing size out of range"));
    }
    if (!VirtualDisplaySelectorsPresent()) {
        return CopyJSONString(failure(@"probe", @"virtual display classes unavailable"));
    }
    @synchronized (VDRegistry()) {
        if (VDRegistry().count > 0) {
            return CopyJSONString(failure(@"validate", @"a Leftcar display already exists"));
        }
    }

    NSString *display_name = (name && *name) ? [NSString stringWithUTF8String:name] : @"Leftcar Display";

    id descriptor = [[VDDescriptorClass() alloc] init];
    if (!descriptor) return CopyJSONString(failure(@"init", @"descriptor alloc failed"));
    MsgSetName(descriptor, display_name);
    MsgSetVendorID(descriptor, kLeftcarVdispVendor);
    MsgSetProductID(descriptor, kLeftcarVdispProduct);
    MsgSetSerialNum(descriptor, kLeftcarVdispSerial);
    MsgSetMaxPixelsWide(descriptor, backing_width);
    MsgSetMaxPixelsHigh(descriptor, backing_height);
    // 물리 크기는 장변 520mm 기준 종횡비 비례 — 모니터급(비TV) 분류를 유지한다.
    CGFloat long_mm = 520.0;
    CGFloat short_mm = (CGFloat)MIN(logical_width, logical_height) / (CGFloat)MAX(logical_width, logical_height) * long_mm;
    BOOL landscape = logical_width >= logical_height;
    MsgSetSizeInMillimeters(descriptor, CGSizeMake(landscape ? long_mm : short_mm,
                                                    landscape ? short_mm : long_mm));
    MsgSetDispatchQueue(descriptor, VDQueue());

    id mode = [[VDModeClass() alloc] initWithWidth:logical_width height:logical_height refreshRate:60.0];
    if (!mode) return CopyJSONString(failure(@"init", @"mode alloc failed"));
    id settings = [[VDSettingsClass() alloc] init];
    if (!settings) return CopyJSONString(failure(@"init", @"settings alloc failed"));
    MsgSetModes(settings, @[mode]);
    MsgSetHiDPI(settings, scale == 2 ? 1u : 0u);

    id display = [[VDClass() alloc] initWithDescriptor:descriptor];
    if (!display) return CopyJSONString(failure(@"init", @"CGVirtualDisplay init returned nil"));
    if (!MsgApplySettings(display, settings)) {
        return CopyJSONString(failure(@"apply", @"applySettings was rejected"));
    }
    uint32_t display_id = MsgDisplayID(display);
    if (display_id == 0) {
        return CopyJSONString(failure(@"apply", @"displayID is zero"));
    }

    // 생성자 반환은 등록 보장이 아니다 — 활성화와 크기를 폴링으로 확인한다.
    // 주의: 생성 프로세스의 CGDisplayCopyDisplayMode 캐시는 부팅 모드에
    // 갇혀 갱신되지 않는다(실측). 대신 bounds는 즉시 갱신되므로(@2x 전환
    // 후 논리 크기로 좁아든다) 검증은 bounds로 한다.
    BOOL active = PollUntil(2.0, ^{ return (BOOL)CGDisplayIsActive(display_id); });
    if (!active) {
        return CopyJSONString(failure(@"online", @"display never became active"));
    }
    __block BOOL mode_verified = NO;
    BOOL (^LogicalBoundsMatch)(void) = ^BOOL(void) {
        CGRect bounds = CGDisplayBounds(display_id);
        return (uint32_t)bounds.size.width == logical_width
            && (uint32_t)bounds.size.height == logical_height;
    };
    PollUntil(1.0, ^BOOL(void) {
        if (LogicalBoundsMatch()) {
            mode_verified = YES;
            return YES;
        }
        return NO;
    });
    if (!mode_verified && getenv("LEFTCAR_VDISP_SKIP_REAPPLY") == NULL) {
        // 이 macOS는 새 가상 디스플레이를 최대 네이티브 모드로 부팅시킨다
        // (실측: 1280x800@2x 선언 → 2560x1600@1x 부팅, 외부에서는 @2x로
        // 확인). 설정 재적용이 선언 모드로 전환시킨다 — 실측으로 확인된
        // 유일한 안전 경로고, 공개 모드 API는 좀비 디스플레이 위험이 있다.
        MsgApplySettings(display, settings);
        // 재적용 후 확정은 수 초 걸린다(실측 ~2-5s).
        PollUntil(6.0, ^BOOL(void) {
            if (LogicalBoundsMatch()) {
                mode_verified = YES;
                return YES;
            }
            return NO;
        });
    }

    // macOS가 새 가상 디스플레이를 TV로 오판해 미러로 묶는 사례가 있다.
    // 세션 스코프 origin 재구성으로 해제하고, 아니면 사용자 arrangement를 존중한다.
    // (진단 스위치: 세션 재구성이 제거를 막는지 판별용 — 실측 중)
    int placement = 0;
    if (CGDisplayIsInMirrorSet(display_id) && getenv("LEFTCAR_VDISP_SKIP_PLACE") == NULL) {
        CGRect main_bounds = CGDisplayBounds(CGMainDisplayID());
        placement = VDPlaceOrigin(display_id,
                                  (int32_t)CGRectGetMaxX(main_bounds),
                                  (int32_t)main_bounds.origin.y);
    }

    NSString *uuid = UUIDForDisplayID(display_id);
    @synchronized (VDRegistry()) {
        VDRegistry()[@(display_id)] = display;
        VDModes()[@(display_id)] = @{@"width": @(logical_width), @"height": @(logical_height), @"scale": @(scale)};
    }

    return CopyJSONString(@{
        @"ok": @YES,
        @"displayId": @(display_id),
        @"uuid": uuid ?: @"",
        @"sourceId": uuid ? [NSString stringWithFormat:@"macos:display:%@", [uuid lowercaseString]] : @"",
        @"name": display_name,
        @"logicalWidth": @(logical_width),
        @"logicalHeight": @(logical_height),
        @"scale": @(scale),
        @"backingWidth": @(backing_width),
        @"backingHeight": @(backing_height),
        @"modeVerified": @(mode_verified),
        @"unmirrored": @(placement == 0 ? CGDisplayIsInMirrorSet(display_id) == NO : NO),
        @"placementResult": @(placement),
        @"createMs": @((NSInteger)((CFAbsoluteTimeGetCurrent() - started) * 1000.0)),
    });
}

char *leftcar_vdisp_status_v1(void) {
    NSMutableArray *entries = [NSMutableArray new];
    @synchronized (VDRegistry()) {
        for (NSNumber *key in VDRegistry()) {
            uint32_t display_id = key.unsignedIntValue;
            NSString *uuid = UUIDForDisplayID(display_id);
            // 저장해 둔 논리 크기는 레지스트리 값이 없으므로 현재 모드에서 읽는다.
            unsigned long logical_width = 0, logical_height = 0;
            size_t pixel_width = 0, pixel_height = 0;
            CGDisplayModeRef current = CGDisplayCopyDisplayMode(display_id);
            if (current) {
                logical_width = CGDisplayModeGetWidth(current);
                logical_height = CGDisplayModeGetHeight(current);
                pixel_width = CGDisplayModeGetPixelWidth(current);
                pixel_height = CGDisplayModeGetPixelHeight(current);
                CFRelease(current);
            }
            CGRect bounds = CGDisplayBounds(display_id);
            uint32_t scale = (logical_width > 0 && pixel_width > logical_width) ? 2u : 1u;
            [entries addObject:@{
                @"displayId": @(display_id),
                @"uuid": uuid ?: @"",
                @"sourceId": uuid ? [NSString stringWithFormat:@"macos:display:%@", [uuid lowercaseString]] : @"",
                @"name": @"Leftcar Display",
                @"logicalWidth": @(logical_width),
                @"logicalHeight": @(logical_height),
                @"backingWidth": @(pixel_width),
                @"backingHeight": @(pixel_height),
                @"scale": @(scale),
                @"active": @(CGDisplayIsActive(display_id)),
                @"online": @(CGDisplayIsOnline(display_id)),
                @"mirrored": @(CGDisplayIsInMirrorSet(display_id)),
                @"bounds": @{
                    @"x": @((NSInteger)bounds.origin.x),
                    @"y": @((NSInteger)bounds.origin.y),
                    @"width": @((NSInteger)bounds.size.width),
                    @"height": @((NSInteger)bounds.size.height),
                },
            }];
        }
    }
    return CopyJSONString(@{@"displays": entries});
}

int32_t leftcar_vdisp_place_v1(uint32_t display_id, int32_t x, int32_t y) {
    @synchronized (VDRegistry()) {
        if (VDRegistry()[@(display_id)] == nil) return 1;
    }
    return VDPlaceOrigin(display_id, x, y);
}

int32_t leftcar_vdisp_is_active_v1(uint32_t display_id) {
    return DisplayIsActiveInList(display_id) ? 1 : 0;
}

// 0=right, 1=left, 2=above, 3=below. Attach to the outermost existing
// display so a second physical monitor is never overlapped or moved.
int32_t leftcar_vdisp_arrange_v1(uint32_t display_id, uint32_t side) {
    if (side > 3 || display_id == CGMainDisplayID()) return 1;
    @synchronized (VDRegistry()) {
        if (VDRegistry()[@(display_id)] == nil) return 1;
    }
    uint32_t count = 0;
    if (CGGetActiveDisplayList(0, NULL, &count) != kCGErrorSuccess || count < 2) return 2;
    CGDirectDisplayID ids[count];
    if (CGGetActiveDisplayList(count, ids, &count) != kCGErrorSuccess) return 2;
    CGRect anchor = CGRectNull;
    CGFloat edge = 0;
    for (uint32_t i = 0; i < count; i++) {
        if (ids[i] == display_id) continue;
        CGRect candidate = CGDisplayBounds(ids[i]);
        CGFloat candidateEdge = side == 0 ? CGRectGetMaxX(candidate)
            : side == 1 ? CGRectGetMinX(candidate)
            : side == 2 ? CGRectGetMinY(candidate) : CGRectGetMaxY(candidate);
        BOOL farther = CGRectIsNull(anchor) ||
            ((side == 0 || side == 3) ? candidateEdge > edge : candidateEdge < edge);
        if (farther) { anchor = candidate; edge = candidateEdge; }
    }
    if (CGRectIsNull(anchor)) return 2;
    CGRect current = CGDisplayBounds(display_id);
    @synchronized (VDRegistry()) {
        NSDictionary *mode = VDModes()[@(display_id)];
        current.size = CGSizeMake([mode[@"width"] doubleValue], [mode[@"height"] doubleValue]);
    }
    int32_t x = (int32_t)anchor.origin.x, y = (int32_t)anchor.origin.y;
    if (side == 0) x = (int32_t)CGRectGetMaxX(anchor);
    if (side == 1) x = (int32_t)(CGRectGetMinX(anchor) - current.size.width);
    if (side == 2) y = (int32_t)(CGRectGetMinY(anchor) - current.size.height);
    if (side == 3) y = (int32_t)CGRectGetMaxY(anchor);
    return VDPlaceOrigin(display_id, x, y);
}

int32_t leftcar_vdisp_destroy_v1(uint32_t display_id) {
    @synchronized (VDRegistry()) {
        if (VDRegistry()[@(display_id)] == nil) return 1;
        [VDRegistry() removeObjectForKey:@(display_id)];
        [VDModes() removeObjectForKey:@(display_id)];
    }
    // 제거는 비동기다 — 실측 ~30s. 빈 세션 재구성 한 번이 WindowServer의
    // 고아 정리를 당긴다면 빠르게 끝난다(실험 중).
    BOOL gone = PollUntil(1.5, ^{ return (BOOL)!DisplayIsActiveInList(display_id); });
    if (!gone) {
        CGDisplayConfigRef sweep = NULL;
        if (CGBeginDisplayConfiguration(&sweep) == kCGErrorSuccess) {
            CGCompleteDisplayConfiguration(sweep, kCGConfigureForSession);
        }
        gone = PollUntil(8.0, ^{ return (BOOL)!DisplayIsActiveInList(display_id); });
    }
    return gone ? 0 : 2;
}

char *leftcar_vdisp_catalog_overrides_v1(void) {
    NSMutableArray *entries = [NSMutableArray new];
    @synchronized (VDRegistry()) {
        for (NSNumber *key in VDRegistry()) {
            uint32_t display_id = key.unsignedIntValue;
            NSString *uuid = UUIDForDisplayID(display_id);
            if (!uuid) continue;
            unsigned long logical_width = 0, logical_height = 0;
            size_t pixel_width = 0;
            CGDisplayModeRef current = CGDisplayCopyDisplayMode(display_id);
            if (current) {
                logical_width = CGDisplayModeGetWidth(current);
                logical_height = CGDisplayModeGetHeight(current);
                pixel_width = CGDisplayModeGetPixelWidth(current);
                CFRelease(current);
            }
            uint32_t scale = (logical_width > 0 && pixel_width > logical_width) ? 2u : 1u;
            [entries addObject:@{
                @"displayId": @(display_id),
                @"uuid": [uuid lowercaseString],
                @"name": @"Leftcar Display",
                @"logicalWidth": @(logical_width),
                @"logicalHeight": @(logical_height),
                @"scale": @(scale),
            }];
        }
    }
    char *json = CopyJSONString(@{@"displays": entries});
    // 배열을 기대하는 호출부(Swift 카탈로그)를 위해 최상위 배열로 다시 만든다.
    NSData *data = [[NSString stringWithUTF8String:json ?: ""] dataUsingEncoding:NSUTF8StringEncoding];
    free(json);
    if (!data) return strdup("[]");
    id parsed = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    NSArray *displays = parsed[@"displays"];
    NSData *out = [NSJSONSerialization dataWithJSONObject:displays ?: @[] options:0 error:nil];
    if (!out) return strdup("[]");
    return strdup([[[NSString alloc] initWithData:out encoding:NSUTF8StringEncoding] UTF8String]);
}
