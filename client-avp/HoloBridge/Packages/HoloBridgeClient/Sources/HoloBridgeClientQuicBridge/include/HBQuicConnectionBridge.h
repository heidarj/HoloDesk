#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

typedef NS_ENUM(NSInteger, HBQuicBridgeEventType) {
    HBQuicBridgeEventTypeStartingTransport = 0,
    HBQuicBridgeEventTypeGroupReady = 1,
    HBQuicBridgeEventTypeGroupFailed = 2,
    HBQuicBridgeEventTypeGroupCancelled = 3,
    HBQuicBridgeEventTypeControlStreamExtracted = 4,
    HBQuicBridgeEventTypeControlStreamReady = 5,
    HBQuicBridgeEventTypeControlStreamFailed = 6,
    HBQuicBridgeEventTypeControlStreamCancelled = 7,
    HBQuicBridgeEventTypeControlPayloadSent = 8,
    HBQuicBridgeEventTypeControlPayloadReceived = 9,
    HBQuicBridgeEventTypeDatagramReceived = 10,
    HBQuicBridgeEventTypeCloseInitiated = 11,
    HBQuicBridgeEventTypeCloseCompleted = 12,
};

typedef NS_ENUM(NSInteger, HBQuicBridgeMode) {
    HBQuicBridgeModeMixed = 0,
    HBQuicBridgeModeDatagramOnly = 1,
};

typedef void (^HBQuicBridgeStartCompletion)(NSError * _Nullable error);
typedef void (^HBQuicBridgeSendCompletion)(NSError * _Nullable error);
typedef void (^HBQuicBridgeEventHandler)(HBQuicBridgeEventType eventType, NSString * _Nullable detail);
typedef void (^HBQuicBridgeDataHandler)(NSData *payload);
typedef void (^HBQuicBridgeTerminationHandler)(NSError * _Nullable error);

@interface HBQuicConnectionBridge : NSObject

@property (nonatomic, copy, nullable) HBQuicBridgeEventHandler eventHandler;
@property (nonatomic, copy, nullable) HBQuicBridgeDataHandler controlPayloadHandler;
@property (nonatomic, copy, nullable) HBQuicBridgeDataHandler datagramHandler;
@property (nonatomic, copy, nullable) HBQuicBridgeTerminationHandler terminationHandler;

- (instancetype)init NS_UNAVAILABLE;
+ (instancetype)new NS_UNAVAILABLE;

- (instancetype)initWithHost:(NSString *)host
                        port:(uint16_t)port
                  serverName:(nullable NSString *)serverName
                        ALPN:(NSString *)ALPN
       allowInsecureCertAuth:(BOOL)allowInsecureCertAuth
  pinnedCertificateFingerprint:(nullable NSString *)pinnedCertificateFingerprint
                  queueLabel:(NSString *)queueLabel;

- (instancetype)initWithHost:(NSString *)host
                        port:(uint16_t)port
                  serverName:(nullable NSString *)serverName
                        ALPN:(NSString *)ALPN
       allowInsecureCertAuth:(BOOL)allowInsecureCertAuth
  pinnedCertificateFingerprint:(nullable NSString *)pinnedCertificateFingerprint
                  queueLabel:(NSString *)queueLabel
                        mode:(HBQuicBridgeMode)mode NS_DESIGNATED_INITIALIZER;

- (void)startWithCompletion:(HBQuicBridgeStartCompletion)completion;
- (void)sendControlPayload:(NSData *)payload completion:(HBQuicBridgeSendCompletion)completion;
- (void)sendDatagramPayload:(NSData *)payload completion:(HBQuicBridgeSendCompletion)completion;
- (void)closeWithReason:(nullable NSString *)reason;

@end

NS_ASSUME_NONNULL_END
