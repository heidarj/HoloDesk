#import "HBQuicConnectionBridge.h"

#import <CommonCrypto/CommonDigest.h>
#import <Network/Network.h>
#import <Security/SecCertificate.h>
#import <Security/SecProtocolOptions.h>
#import <Security/SecTrust.h>

NS_ASSUME_NONNULL_BEGIN

@interface HBQuicConnectionBridge ()

@property (nonatomic, copy) NSString *host;
@property (nonatomic, assign) uint16_t port;
@property (nonatomic, copy, nullable) NSString *serverName;
@property (nonatomic, copy) NSString *ALPN;
@property (nonatomic, assign) BOOL allowInsecureCertAuth;
@property (nonatomic, copy, nullable) NSString *pinnedCertificateFingerprint;
@property (nonatomic, assign) HBQuicBridgeMode mode;
@property (nonatomic, strong) dispatch_queue_t queue;

@property (nonatomic, strong, nullable) nw_connection_group_t connectionGroup;
@property (nonatomic, strong, nullable) nw_connection_t controlConnection;
@property (nonatomic, strong, nullable) nw_connection_t datagramConnection;
@property (nonatomic, copy, nullable) HBQuicBridgeStartCompletion startCompletion;

@property (nonatomic, assign) BOOL didStart;
@property (nonatomic, assign) BOOL didExtractControlConnection;
@property (nonatomic, assign) BOOL didExtractDatagramConnection;
@property (nonatomic, assign) BOOL didInitiateClose;
@property (nonatomic, assign) BOOL didTerminate;

@end

@implementation HBQuicConnectionBridge

- (instancetype)initWithHost:(NSString *)host
                        port:(uint16_t)port
                  serverName:(nullable NSString *)serverName
                        ALPN:(NSString *)ALPN
       allowInsecureCertAuth:(BOOL)allowInsecureCertAuth
  pinnedCertificateFingerprint:(nullable NSString *)pinnedCertificateFingerprint
                  queueLabel:(NSString *)queueLabel {
    return [self initWithHost:host
                         port:port
                   serverName:serverName
                         ALPN:ALPN
        allowInsecureCertAuth:allowInsecureCertAuth
   pinnedCertificateFingerprint:pinnedCertificateFingerprint
                   queueLabel:queueLabel
                         mode:HBQuicBridgeModeMixed];
}

- (instancetype)initWithHost:(NSString *)host
                        port:(uint16_t)port
                  serverName:(nullable NSString *)serverName
                        ALPN:(NSString *)ALPN
       allowInsecureCertAuth:(BOOL)allowInsecureCertAuth
  pinnedCertificateFingerprint:(nullable NSString *)pinnedCertificateFingerprint
                  queueLabel:(NSString *)queueLabel
                        mode:(HBQuicBridgeMode)mode {
    self = [super init];
    if (self) {
        _host = [host copy];
        _port = port;
        _serverName = [serverName copy];
        _ALPN = [ALPN copy];
        _allowInsecureCertAuth = allowInsecureCertAuth;
        _pinnedCertificateFingerprint = [pinnedCertificateFingerprint copy];
        _mode = mode;
        _queue = dispatch_queue_create(queueLabel.UTF8String, DISPATCH_QUEUE_SERIAL);
    }
    return self;
}

- (void)startWithCompletion:(HBQuicBridgeStartCompletion)completion {
    dispatch_async(self.queue, ^{
        if (self.connectionGroup != nil || self.didStart) {
            completion([NSError errorWithDomain:NSPOSIXErrorDomain code:EALREADY userInfo:@{
                NSLocalizedDescriptionKey: @"QUIC transport is already started",
            }]);
            return;
        }

        self.startCompletion = [completion copy];
        [self emitEvent:HBQuicBridgeEventTypeStartingTransport detail:nil];

        nw_endpoint_t endpoint = nw_endpoint_create_host(self.host.UTF8String, self.portString.UTF8String);
        if (endpoint == nil) {
            [self completeStartWithError:[self invalidConfigurationError:@"failed to create host endpoint"]];
            return;
        }

        nw_group_descriptor_t descriptor = nw_group_descriptor_create_multiplex(endpoint);
        if (descriptor == nil) {
            [self completeStartWithError:[self invalidConfigurationError:@"failed to create multiplex group descriptor"]];
            return;
        }

        nw_parameters_t parameters = [self createGroupParameters];
        if (parameters == nil) {
            [self completeStartWithError:[self invalidConfigurationError:@"failed to create QUIC parameters"]];
            return;
        }

        nw_connection_group_t group = nw_connection_group_create(descriptor, parameters);
        if (group == nil) {
            [self completeStartWithError:[self invalidConfigurationError:@"failed to create QUIC connection group"]];
            return;
        }

        self.connectionGroup = group;
        nw_connection_group_set_queue(group, self.queue);

        __weak typeof(self) weakSelf = self;
        nw_connection_group_set_state_changed_handler(group, ^(nw_connection_group_state_t state, nw_error_t _Nullable error) {
            [weakSelf handleGroupStateChange:state error:error];
        });

        nw_connection_group_set_receive_handler(group, UINT32_MAX, true, ^(dispatch_data_t _Nullable content, nw_content_context_t context, bool is_complete) {
            [weakSelf handleDatagramContent:content isComplete:is_complete];
            (void)context;
        });

        nw_connection_group_start(group);
    });
}

- (void)sendControlPayload:(NSData *)payload completion:(HBQuicBridgeSendCompletion)completion {
    dispatch_async(self.queue, ^{
        if (self.controlConnection == nil) {
            completion([self invalidConfigurationError:@"control stream is not connected"]);
            return;
        }

        dispatch_data_t content = [self copyDispatchDataFromData:payload];
        nw_connection_send(
            self.controlConnection,
            content,
            NW_CONNECTION_DEFAULT_MESSAGE_CONTEXT,
            true,
            ^(nw_error_t _Nullable error) {
                if (error == nil) {
                    [self emitEvent:HBQuicBridgeEventTypeControlPayloadSent detail:[NSString stringWithFormat:@"%lu", (unsigned long)payload.length]];
                }
                completion([self errorFromNWError:error]);
            }
        );
    });
}

- (void)sendDatagramPayload:(NSData *)payload completion:(HBQuicBridgeSendCompletion)completion {
    dispatch_async(self.queue, ^{
        if (self.connectionGroup == nil) {
            completion([self invalidConfigurationError:@"datagram flow is not connected"]);
            return;
        }

        dispatch_data_t content = [self copyDispatchDataFromData:payload];
        nw_connection_group_send_message(
            self.connectionGroup,
            content,
            nil,
            NW_CONNECTION_DEFAULT_MESSAGE_CONTEXT,
            ^(nw_error_t _Nullable error) {
                completion([self errorFromNWError:error]);
            }
        );
    });
}

- (void)closeWithReason:(nullable NSString *)reason {
    dispatch_async(self.queue, ^{
        if (self.didInitiateClose) {
            return;
        }

        self.didInitiateClose = YES;
        [self emitEvent:HBQuicBridgeEventTypeCloseInitiated detail:reason];

        if (self.controlConnection != nil) {
            nw_connection_cancel(self.controlConnection);
        }
        if (self.datagramConnection != nil) {
            nw_connection_cancel(self.datagramConnection);
        }
        if (self.connectionGroup != nil) {
            nw_connection_group_cancel(self.connectionGroup);
        }
    });
}

#pragma mark - State Handling

- (void)handleGroupStateChange:(nw_connection_group_state_t)state error:(nw_error_t _Nullable)error {
    switch (state) {
        case nw_connection_group_state_ready:
            [self emitEvent:HBQuicBridgeEventTypeGroupReady detail:nil];
            if (self.mode == HBQuicBridgeModeDatagramOnly) {
                self.didStart = YES;
                [self completeStartWithError:nil];
            } else {
                [self extractControlConnectionIfNeeded];
            }
            break;
        case nw_connection_group_state_failed: {
            NSError *nsError = [self errorFromNWError:error];
            [self emitEvent:HBQuicBridgeEventTypeGroupFailed detail:nsError.localizedDescription];
            if (!self.didStart) {
                [self completeStartWithError:nsError];
            } else {
                [self finishWithError:nsError];
            }
            break;
        }
        case nw_connection_group_state_cancelled:
            [self emitEvent:HBQuicBridgeEventTypeGroupCancelled detail:nil];
            if (!self.didStart) {
                [self completeStartWithError:[self cancellationError]];
            } else {
                [self finishWithError:nil];
            }
            break;
        default:
            break;
    }
}

- (void)handleControlStateChange:(nw_connection_state_t)state error:(nw_error_t _Nullable)error {
    switch (state) {
        case nw_connection_state_ready:
            [self emitEvent:HBQuicBridgeEventTypeControlStreamReady detail:nil];
            self.didStart = YES;
            [self completeStartWithError:nil];
            [self scheduleNextControlReceive];
            break;
        case nw_connection_state_failed: {
            NSError *nsError = [self errorFromNWError:error];
            [self emitEvent:HBQuicBridgeEventTypeControlStreamFailed detail:nsError.localizedDescription];
            if (!self.didStart) {
                [self completeStartWithError:nsError];
            } else {
                [self finishWithError:nsError];
            }
            break;
        }
        case nw_connection_state_cancelled:
            [self emitEvent:HBQuicBridgeEventTypeControlStreamCancelled detail:nil];
            if (!self.didStart) {
                [self completeStartWithError:[self cancellationError]];
            } else {
                [self finishWithError:nil];
            }
            break;
        default:
            break;
    }
}

- (void)handleDatagramStateChange:(nw_connection_state_t)state error:(nw_error_t _Nullable)error {
    switch (state) {
        case nw_connection_state_ready:
            self.didStart = YES;
            [self completeStartWithError:nil];
            [self scheduleNextDatagramReceive];
            break;
        case nw_connection_state_failed: {
            NSError *nsError = [self errorFromNWError:error];
            if (!self.didStart) {
                [self completeStartWithError:nsError];
            } else {
                [self finishWithError:nsError];
            }
            break;
        }
        case nw_connection_state_cancelled:
            if (!self.didStart) {
                [self completeStartWithError:[self cancellationError]];
            } else {
                [self finishWithError:nil];
            }
            break;
        default:
            break;
    }
}

- (void)extractControlConnectionIfNeeded {
    if (self.didExtractControlConnection || self.connectionGroup == nil) {
        return;
    }

    nw_connection_t connection = nw_connection_group_extract_connection(self.connectionGroup, nil, nil);
    if (connection == nil) {
        [self completeStartWithError:[self invalidConfigurationError:@"failed to extract QUIC control stream"]];
        return;
    }

    self.didExtractControlConnection = YES;
    self.controlConnection = connection;
    nw_connection_set_queue(connection, self.queue);

    __weak typeof(self) weakSelf = self;
    nw_connection_set_state_changed_handler(connection, ^(nw_connection_state_t state, nw_error_t _Nullable error) {
        [weakSelf handleControlStateChange:state error:error];
    });

    [self emitEvent:HBQuicBridgeEventTypeControlStreamExtracted detail:nil];
    nw_connection_start(connection);
}

- (void)extractDatagramConnectionIfNeeded {
    if (self.didExtractDatagramConnection || self.connectionGroup == nil) {
        return;
    }

    if (@available(macOS 13.0, iOS 16.0, watchOS 9.0, tvOS 16.0, *)) {
        nw_protocol_options_t datagramOptions = nw_quic_create_options();
        if (datagramOptions == nil) {
            [self completeStartWithError:[self invalidConfigurationError:@"failed to create QUIC datagram flow options"]];
            return;
        }

        nw_quic_set_stream_is_datagram(datagramOptions, true);
        nw_connection_t connection = nw_connection_group_extract_connection(self.connectionGroup, nil, datagramOptions);
        if (connection == nil) {
            [self completeStartWithError:[self invalidConfigurationError:@"failed to extract QUIC datagram flow"]];
            return;
        }

        self.didExtractDatagramConnection = YES;
        self.datagramConnection = connection;
        nw_connection_set_queue(connection, self.queue);

        __weak typeof(self) weakSelf = self;
        nw_connection_set_state_changed_handler(connection, ^(nw_connection_state_t state, nw_error_t _Nullable error) {
            [weakSelf handleDatagramStateChange:state error:error];
        });

        nw_connection_start(connection);
        return;
    }

    [self completeStartWithError:[self invalidConfigurationError:@"QUIC datagram flow extraction requires macOS 13.0 or newer"]];
}

- (void)scheduleNextControlReceive {
    if (self.controlConnection == nil || self.didTerminate) {
        return;
    }

    __weak typeof(self) weakSelf = self;
    nw_connection_receive(
        self.controlConnection,
        1,
        65536,
        ^(dispatch_data_t _Nullable content, nw_content_context_t context, bool is_complete, nw_error_t _Nullable error) {
            (void)context;
            [weakSelf handleControlReceiveContent:content isComplete:is_complete error:error];
        }
    );
}

- (void)handleControlReceiveContent:(dispatch_data_t _Nullable)content
                          isComplete:(bool)isComplete
                               error:(nw_error_t _Nullable)error {
    if (content != nil) {
        NSData *payload = [self copyDataFromDispatchData:content];
        if (payload.length > 0 && self.controlPayloadHandler != nil) {
            [self emitEvent:HBQuicBridgeEventTypeControlPayloadReceived detail:[NSString stringWithFormat:@"%lu", (unsigned long)payload.length]];
            self.controlPayloadHandler(payload);
        }
    }

    if (error != nil) {
        [self finishWithError:[self errorFromNWError:error]];
        return;
    }

    if (isComplete) {
        [self finishWithError:nil];
        return;
    }

    [self scheduleNextControlReceive];
}

- (void)scheduleNextDatagramReceive {
    if (self.datagramConnection == nil || self.didTerminate) {
        return;
    }

    __weak typeof(self) weakSelf = self;
    nw_connection_receive_message(
        self.datagramConnection,
        ^(dispatch_data_t _Nullable content, nw_content_context_t _Nullable context, bool is_complete, nw_error_t _Nullable error) {
            (void)context;
            [weakSelf handleDatagramReceiveContent:content isComplete:is_complete error:error];
        }
    );
}

- (void)handleDatagramReceiveContent:(dispatch_data_t _Nullable)content
                           isComplete:(bool)isComplete
                                error:(nw_error_t _Nullable)error {
    if (content != nil) {
        NSData *payload = [self copyDataFromDispatchData:content];
        if (payload.length > 0) {
            [self emitEvent:HBQuicBridgeEventTypeDatagramReceived detail:[NSString stringWithFormat:@"%lu", (unsigned long)payload.length]];
            if (self.datagramHandler != nil) {
                self.datagramHandler(payload);
            }
        }
    }

    if (error != nil) {
        [self finishWithError:[self errorFromNWError:error]];
        return;
    }

    if (isComplete && content == nil) {
        [self finishWithError:nil];
        return;
    }

    [self scheduleNextDatagramReceive];
}

- (void)handleDatagramContent:(dispatch_data_t _Nullable)content isComplete:(bool)isComplete {
    if (content == nil) {
        if (isComplete && !self.didInitiateClose) {
            [self finishWithError:nil];
        }
        return;
    }

    NSData *payload = [self copyDataFromDispatchData:content];
    if (payload.length == 0) {
        return;
    }

    [self emitEvent:HBQuicBridgeEventTypeDatagramReceived detail:[NSString stringWithFormat:@"%lu", (unsigned long)payload.length]];
    if (self.datagramHandler != nil) {
        self.datagramHandler(payload);
    }
}

#pragma mark - Helpers

- (nullable nw_parameters_t)createGroupParameters {
    __block sec_protocol_options_t securityOptions = nil;
    nw_parameters_t parameters = nw_parameters_create_quic(^(nw_protocol_options_t quicOptions) {
        nw_quic_add_tls_application_protocol(quicOptions, self.ALPN.UTF8String);
        if (@available(macOS 13.0, iOS 16.0, watchOS 9.0, tvOS 16.0, *)) {
            nw_quic_set_max_datagram_frame_size(quicOptions, UINT16_MAX);
        }
        securityOptions = nw_quic_copy_sec_protocol_options(quicOptions);
    });

    if (parameters == nil || securityOptions == nil) {
        return nil;
    }

    NSString *resolvedServerName = self.serverName.length > 0 ? self.serverName : self.host;
    sec_protocol_options_set_tls_server_name(securityOptions, resolvedServerName.UTF8String);

    if (self.allowInsecureCertAuth || self.pinnedCertificateFingerprint.length > 0) {
        sec_protocol_options_set_verify_block(securityOptions, ^(sec_protocol_metadata_t _Nonnull metadata, sec_trust_t _Nonnull trust, sec_protocol_verify_complete_t  _Nonnull complete) {
            (void)metadata;
            if (self.allowInsecureCertAuth) {
                complete(true);
                return;
            }

            SecTrustRef trustRef = sec_trust_copy_ref(trust);
            BOOL valid = [self trustMatchesPinnedFingerprint:trustRef];
            if (trustRef != NULL) {
                CFRelease(trustRef);
            }
            complete(valid);
        }, self.queue);
    }

    nw_parameters_set_reuse_local_address(parameters, true);
    return parameters;
}

- (NSString *)portString {
    return [NSString stringWithFormat:@"%hu", self.port];
}


- (void)completeStartWithError:(NSError * _Nullable)error {
    HBQuicBridgeStartCompletion completion = self.startCompletion;
    self.startCompletion = nil;
    if (completion != nil) {
        completion(error);
    }
}

- (void)finishWithError:(NSError * _Nullable)error {
    if (self.didTerminate) {
        return;
    }
    self.didTerminate = YES;

    if (self.startCompletion != nil) {
        [self completeStartWithError:error ?: [self cancellationError]];
        return;
    }

    [self emitEvent:HBQuicBridgeEventTypeCloseCompleted detail:error.localizedDescription];
    if (self.terminationHandler != nil) {
        self.terminationHandler(error);
    }
}

- (void)emitEvent:(HBQuicBridgeEventType)eventType detail:(nullable NSString *)detail {
    if (self.eventHandler != nil) {
        self.eventHandler(eventType, detail);
    }
}

- (NSError *)invalidConfigurationError:(NSString *)detail {
    return [NSError errorWithDomain:NSPOSIXErrorDomain code:EINVAL userInfo:@{
        NSLocalizedDescriptionKey: detail,
    }];
}

- (NSError *)cancellationError {
    return [NSError errorWithDomain:NSPOSIXErrorDomain code:ECANCELED userInfo:@{
        NSLocalizedDescriptionKey: @"QUIC transport was cancelled",
    }];
}

- (nullable NSError *)errorFromNWError:(nw_error_t _Nullable)error {
    if (error == nil) {
        return nil;
    }

    CFErrorRef cfError = nw_error_copy_cf_error(error);
    if (cfError == NULL) {
        return [NSError errorWithDomain:NSPOSIXErrorDomain code:EIO userInfo:@{
            NSLocalizedDescriptionKey: @"Network.framework returned an unknown error",
        }];
    }

    return CFBridgingRelease(cfError);
}

- (dispatch_data_t)copyDispatchDataFromData:(NSData *)data {
    void *bytes = malloc(data.length);
    if (bytes == NULL) {
        return dispatch_data_empty;
    }
    memcpy(bytes, data.bytes, data.length);
    return dispatch_data_create(bytes, data.length, self.queue, ^{
        free(bytes);
    });
}

- (NSData *)copyDataFromDispatchData:(dispatch_data_t)data {
    __block NSMutableData *buffer = [NSMutableData data];
    dispatch_data_apply(data, ^bool(dispatch_data_t region, size_t offset, const void *mappedBuffer, size_t size) {
        (void)region;
        (void)offset;
        [buffer appendBytes:mappedBuffer length:size];
        return true;
    });
    return [buffer copy];
}

- (BOOL)trustMatchesPinnedFingerprint:(SecTrustRef)trust {
    if (trust == NULL || self.pinnedCertificateFingerprint.length == 0) {
        return NO;
    }

    CFArrayRef certificateChain = SecTrustCopyCertificateChain(trust);
    if (certificateChain == NULL || CFArrayGetCount(certificateChain) == 0) {
        if (certificateChain != NULL) {
            CFRelease(certificateChain);
        }
        return NO;
    }

    SecCertificateRef certificate = (SecCertificateRef)CFArrayGetValueAtIndex(certificateChain, 0);
    CFRetain(certificate);
    CFRelease(certificateChain);

    NSData *certificateData = CFBridgingRelease(SecCertificateCopyData(certificate));
    CFRelease(certificate);
    if (certificateData.length == 0) {
        return NO;
    }

    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256(certificateData.bytes, (CC_LONG)certificateData.length, digest);

    NSMutableString *fingerprint = [NSMutableString stringWithCapacity:CC_SHA256_DIGEST_LENGTH * 2];
    for (NSUInteger index = 0; index < CC_SHA256_DIGEST_LENGTH; index++) {
        [fingerprint appendFormat:@"%02x", digest[index]];
    }

    NSString *normalizedActual = [self.class normalizeFingerprint:fingerprint];
    NSString *normalizedExpected = [self.class normalizeFingerprint:self.pinnedCertificateFingerprint ?: @""];
    return [normalizedActual isEqualToString:normalizedExpected];
}

+ (NSString *)normalizeFingerprint:(NSString *)value {
    NSString *trimmed = [value stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    return [[trimmed stringByReplacingOccurrencesOfString:@":" withString:@""] lowercaseString];
}

@end

NS_ASSUME_NONNULL_END
