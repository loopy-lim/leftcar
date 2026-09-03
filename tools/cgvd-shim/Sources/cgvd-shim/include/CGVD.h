//
//  CGVD.h — private CoreGraphics virtual display API 최소 선언.
//  Khaos Tian(VirtualDisplayExp) → DeskPad 계보에서 공개된 시그니처 목록을
//  참고해 이 프로브용으로 직접 작성했다 (파일 텍스트 복사 아님).
//  Private API — macOS 버전에 따라 파손 가능. 본 프로브는 evidence 수집용이다.
//

#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>

NS_ASSUME_NONNULL_BEGIN

@interface CGVirtualDisplayMode : NSObject
@property(readonly, nonatomic) CGFloat refreshRate;
@property(readonly, nonatomic) NSUInteger width;
@property(readonly, nonatomic) NSUInteger height;
- (instancetype)initWithWidth:(NSUInteger)width
                       height:(NSUInteger)height
                  refreshRate:(CGFloat)refreshRate;
@end

@interface CGVirtualDisplaySettings : NSObject
@property(retain, nonatomic) NSArray<CGVirtualDisplayMode *> *modes;
@property(nonatomic) unsigned int hiDPI;
@end

@interface CGVirtualDisplay : NSObject
@property(readonly, nonatomic) CGDirectDisplayID displayID;
@property(readonly, nonatomic) NSArray *modes;
- (instancetype)initWithDescriptor:(id)descriptor;
- (BOOL)applySettings:(CGVirtualDisplaySettings *)settings;
@end

@interface CGVirtualDisplayDescriptor : NSObject
@property(nonatomic) unsigned int maxPixelsWide;
@property(nonatomic) unsigned int maxPixelsHigh;
@property(nonatomic) CGSize sizeInMillimeters;
@property(nonatomic) unsigned int serialNum;
@property(nonatomic) unsigned int productID;
@property(nonatomic) unsigned int vendorID;
@property(retain, nonatomic, nullable) NSString *name;
- (void)setDispatchQueue:(dispatch_queue_t)queue;
@end

NS_ASSUME_NONNULL_END
