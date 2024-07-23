// Code generated by gRPC proto compiler.  DO NOT EDIT!
// source: platform.proto

#if !defined(GPB_GRPC_PROTOCOL_ONLY) || !GPB_GRPC_PROTOCOL_ONLY
#import "Platform.pbrpc.h"
#import "Platform.pbobjc.h"
#import <ProtoRPC/ProtoRPCLegacy.h>
#import <RxLibrary/GRXWriter+Immediate.h>

#if defined(GPB_USE_PROTOBUF_FRAMEWORK_IMPORTS) && GPB_USE_PROTOBUF_FRAMEWORK_IMPORTS
#import <Protobuf/GPBWrappers.pbobjc.h>
#else
#import "GPBWrappers.pbobjc.h"
#endif
#if defined(GPB_USE_PROTOBUF_FRAMEWORK_IMPORTS) && GPB_USE_PROTOBUF_FRAMEWORK_IMPORTS
#import <Protobuf/GPBStruct.pbobjc.h>
#else
#import "GPBStruct.pbobjc.h"
#endif
#if defined(GPB_USE_PROTOBUF_FRAMEWORK_IMPORTS) && GPB_USE_PROTOBUF_FRAMEWORK_IMPORTS
#import <Protobuf/GPBTimestamp.pbobjc.h>
#else
#import "GPBTimestamp.pbobjc.h"
#endif

@implementation Platform

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wobjc-designated-initializers"

// Designated initializer
- (instancetype)initWithHost:(NSString *)host callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [super initWithHost:host
                 packageName:@"org.dash.platform.dapi.v0"
                 serviceName:@"Platform"
                 callOptions:callOptions];
}

- (instancetype)initWithHost:(NSString *)host {
  return [super initWithHost:host
                 packageName:@"org.dash.platform.dapi.v0"
                 serviceName:@"Platform"];
}

#pragma clang diagnostic pop

// Override superclass initializer to disallow different package and service names.
- (instancetype)initWithHost:(NSString *)host
                 packageName:(NSString *)packageName
                 serviceName:(NSString *)serviceName {
  return [self initWithHost:host];
}

- (instancetype)initWithHost:(NSString *)host
                 packageName:(NSString *)packageName
                 serviceName:(NSString *)serviceName
                 callOptions:(GRPCCallOptions *)callOptions {
  return [self initWithHost:host callOptions:callOptions];
}

#pragma mark - Class Methods

+ (instancetype)serviceWithHost:(NSString *)host {
  return [[self alloc] initWithHost:host];
}

+ (instancetype)serviceWithHost:(NSString *)host callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [[self alloc] initWithHost:host callOptions:callOptions];
}

#pragma mark - Method Implementations

#pragma mark broadcastStateTransition(BroadcastStateTransitionRequest) returns (BroadcastStateTransitionResponse)

- (void)broadcastStateTransitionWithRequest:(BroadcastStateTransitionRequest *)request handler:(void(^)(BroadcastStateTransitionResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTobroadcastStateTransitionWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTobroadcastStateTransitionWithRequest:(BroadcastStateTransitionRequest *)request handler:(void(^)(BroadcastStateTransitionResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"broadcastStateTransition"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[BroadcastStateTransitionResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)broadcastStateTransitionWithMessage:(BroadcastStateTransitionRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"broadcastStateTransition"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[BroadcastStateTransitionResponse class]];
}

#pragma mark getIdentity(GetIdentityRequest) returns (GetIdentityResponse)

- (void)getIdentityWithRequest:(GetIdentityRequest *)request handler:(void(^)(GetIdentityResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityWithRequest:(GetIdentityRequest *)request handler:(void(^)(GetIdentityResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentity"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityWithMessage:(GetIdentityRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentity"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityResponse class]];
}

#pragma mark getIdentityKeys(GetIdentityKeysRequest) returns (GetIdentityKeysResponse)

- (void)getIdentityKeysWithRequest:(GetIdentityKeysRequest *)request handler:(void(^)(GetIdentityKeysResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityKeysWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityKeysWithRequest:(GetIdentityKeysRequest *)request handler:(void(^)(GetIdentityKeysResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentityKeys"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityKeysResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityKeysWithMessage:(GetIdentityKeysRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentityKeys"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityKeysResponse class]];
}

#pragma mark getIdentitiesContractKeys(GetIdentitiesContractKeysRequest) returns (GetIdentitiesContractKeysResponse)

- (void)getIdentitiesContractKeysWithRequest:(GetIdentitiesContractKeysRequest *)request handler:(void(^)(GetIdentitiesContractKeysResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentitiesContractKeysWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentitiesContractKeysWithRequest:(GetIdentitiesContractKeysRequest *)request handler:(void(^)(GetIdentitiesContractKeysResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentitiesContractKeys"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentitiesContractKeysResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentitiesContractKeysWithMessage:(GetIdentitiesContractKeysRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentitiesContractKeys"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentitiesContractKeysResponse class]];
}

#pragma mark getIdentityNonce(GetIdentityNonceRequest) returns (GetIdentityNonceResponse)

- (void)getIdentityNonceWithRequest:(GetIdentityNonceRequest *)request handler:(void(^)(GetIdentityNonceResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityNonceWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityNonceWithRequest:(GetIdentityNonceRequest *)request handler:(void(^)(GetIdentityNonceResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentityNonce"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityNonceResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityNonceWithMessage:(GetIdentityNonceRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentityNonce"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityNonceResponse class]];
}

#pragma mark getIdentityContractNonce(GetIdentityContractNonceRequest) returns (GetIdentityContractNonceResponse)

- (void)getIdentityContractNonceWithRequest:(GetIdentityContractNonceRequest *)request handler:(void(^)(GetIdentityContractNonceResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityContractNonceWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityContractNonceWithRequest:(GetIdentityContractNonceRequest *)request handler:(void(^)(GetIdentityContractNonceResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentityContractNonce"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityContractNonceResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityContractNonceWithMessage:(GetIdentityContractNonceRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentityContractNonce"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityContractNonceResponse class]];
}

#pragma mark getIdentityBalance(GetIdentityBalanceRequest) returns (GetIdentityBalanceResponse)

- (void)getIdentityBalanceWithRequest:(GetIdentityBalanceRequest *)request handler:(void(^)(GetIdentityBalanceResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityBalanceWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityBalanceWithRequest:(GetIdentityBalanceRequest *)request handler:(void(^)(GetIdentityBalanceResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentityBalance"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityBalanceResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityBalanceWithMessage:(GetIdentityBalanceRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentityBalance"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityBalanceResponse class]];
}

#pragma mark getIdentityBalanceAndRevision(GetIdentityBalanceAndRevisionRequest) returns (GetIdentityBalanceAndRevisionResponse)

- (void)getIdentityBalanceAndRevisionWithRequest:(GetIdentityBalanceAndRevisionRequest *)request handler:(void(^)(GetIdentityBalanceAndRevisionResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityBalanceAndRevisionWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityBalanceAndRevisionWithRequest:(GetIdentityBalanceAndRevisionRequest *)request handler:(void(^)(GetIdentityBalanceAndRevisionResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentityBalanceAndRevision"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityBalanceAndRevisionResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityBalanceAndRevisionWithMessage:(GetIdentityBalanceAndRevisionRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentityBalanceAndRevision"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityBalanceAndRevisionResponse class]];
}

#pragma mark getProofs(GetProofsRequest) returns (GetProofsResponse)

- (void)getProofsWithRequest:(GetProofsRequest *)request handler:(void(^)(GetProofsResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetProofsWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetProofsWithRequest:(GetProofsRequest *)request handler:(void(^)(GetProofsResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getProofs"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetProofsResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getProofsWithMessage:(GetProofsRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getProofs"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetProofsResponse class]];
}

#pragma mark getDataContract(GetDataContractRequest) returns (GetDataContractResponse)

- (void)getDataContractWithRequest:(GetDataContractRequest *)request handler:(void(^)(GetDataContractResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetDataContractWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetDataContractWithRequest:(GetDataContractRequest *)request handler:(void(^)(GetDataContractResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getDataContract"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetDataContractResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getDataContractWithMessage:(GetDataContractRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getDataContract"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetDataContractResponse class]];
}

#pragma mark getDataContractHistory(GetDataContractHistoryRequest) returns (GetDataContractHistoryResponse)

- (void)getDataContractHistoryWithRequest:(GetDataContractHistoryRequest *)request handler:(void(^)(GetDataContractHistoryResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetDataContractHistoryWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetDataContractHistoryWithRequest:(GetDataContractHistoryRequest *)request handler:(void(^)(GetDataContractHistoryResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getDataContractHistory"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetDataContractHistoryResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getDataContractHistoryWithMessage:(GetDataContractHistoryRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getDataContractHistory"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetDataContractHistoryResponse class]];
}

#pragma mark getDataContracts(GetDataContractsRequest) returns (GetDataContractsResponse)

- (void)getDataContractsWithRequest:(GetDataContractsRequest *)request handler:(void(^)(GetDataContractsResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetDataContractsWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetDataContractsWithRequest:(GetDataContractsRequest *)request handler:(void(^)(GetDataContractsResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getDataContracts"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetDataContractsResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getDataContractsWithMessage:(GetDataContractsRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getDataContracts"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetDataContractsResponse class]];
}

#pragma mark getDocuments(GetDocumentsRequest) returns (GetDocumentsResponse)

- (void)getDocumentsWithRequest:(GetDocumentsRequest *)request handler:(void(^)(GetDocumentsResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetDocumentsWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetDocumentsWithRequest:(GetDocumentsRequest *)request handler:(void(^)(GetDocumentsResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getDocuments"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetDocumentsResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getDocumentsWithMessage:(GetDocumentsRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getDocuments"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetDocumentsResponse class]];
}

#pragma mark getIdentityByPublicKeyHash(GetIdentityByPublicKeyHashRequest) returns (GetIdentityByPublicKeyHashResponse)

- (void)getIdentityByPublicKeyHashWithRequest:(GetIdentityByPublicKeyHashRequest *)request handler:(void(^)(GetIdentityByPublicKeyHashResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetIdentityByPublicKeyHashWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetIdentityByPublicKeyHashWithRequest:(GetIdentityByPublicKeyHashRequest *)request handler:(void(^)(GetIdentityByPublicKeyHashResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getIdentityByPublicKeyHash"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetIdentityByPublicKeyHashResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getIdentityByPublicKeyHashWithMessage:(GetIdentityByPublicKeyHashRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getIdentityByPublicKeyHash"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetIdentityByPublicKeyHashResponse class]];
}

#pragma mark waitForStateTransitionResult(WaitForStateTransitionResultRequest) returns (WaitForStateTransitionResultResponse)

- (void)waitForStateTransitionResultWithRequest:(WaitForStateTransitionResultRequest *)request handler:(void(^)(WaitForStateTransitionResultResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTowaitForStateTransitionResultWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTowaitForStateTransitionResultWithRequest:(WaitForStateTransitionResultRequest *)request handler:(void(^)(WaitForStateTransitionResultResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"waitForStateTransitionResult"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[WaitForStateTransitionResultResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)waitForStateTransitionResultWithMessage:(WaitForStateTransitionResultRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"waitForStateTransitionResult"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[WaitForStateTransitionResultResponse class]];
}

#pragma mark getConsensusParams(GetConsensusParamsRequest) returns (GetConsensusParamsResponse)

- (void)getConsensusParamsWithRequest:(GetConsensusParamsRequest *)request handler:(void(^)(GetConsensusParamsResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetConsensusParamsWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetConsensusParamsWithRequest:(GetConsensusParamsRequest *)request handler:(void(^)(GetConsensusParamsResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getConsensusParams"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetConsensusParamsResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getConsensusParamsWithMessage:(GetConsensusParamsRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getConsensusParams"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetConsensusParamsResponse class]];
}

#pragma mark getProtocolVersionUpgradeState(GetProtocolVersionUpgradeStateRequest) returns (GetProtocolVersionUpgradeStateResponse)

- (void)getProtocolVersionUpgradeStateWithRequest:(GetProtocolVersionUpgradeStateRequest *)request handler:(void(^)(GetProtocolVersionUpgradeStateResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetProtocolVersionUpgradeStateWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetProtocolVersionUpgradeStateWithRequest:(GetProtocolVersionUpgradeStateRequest *)request handler:(void(^)(GetProtocolVersionUpgradeStateResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getProtocolVersionUpgradeState"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetProtocolVersionUpgradeStateResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getProtocolVersionUpgradeStateWithMessage:(GetProtocolVersionUpgradeStateRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getProtocolVersionUpgradeState"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetProtocolVersionUpgradeStateResponse class]];
}

#pragma mark getProtocolVersionUpgradeVoteStatus(GetProtocolVersionUpgradeVoteStatusRequest) returns (GetProtocolVersionUpgradeVoteStatusResponse)

- (void)getProtocolVersionUpgradeVoteStatusWithRequest:(GetProtocolVersionUpgradeVoteStatusRequest *)request handler:(void(^)(GetProtocolVersionUpgradeVoteStatusResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetProtocolVersionUpgradeVoteStatusWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetProtocolVersionUpgradeVoteStatusWithRequest:(GetProtocolVersionUpgradeVoteStatusRequest *)request handler:(void(^)(GetProtocolVersionUpgradeVoteStatusResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getProtocolVersionUpgradeVoteStatus"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetProtocolVersionUpgradeVoteStatusResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getProtocolVersionUpgradeVoteStatusWithMessage:(GetProtocolVersionUpgradeVoteStatusRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getProtocolVersionUpgradeVoteStatus"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetProtocolVersionUpgradeVoteStatusResponse class]];
}

#pragma mark getEpochsInfo(GetEpochsInfoRequest) returns (GetEpochsInfoResponse)

- (void)getEpochsInfoWithRequest:(GetEpochsInfoRequest *)request handler:(void(^)(GetEpochsInfoResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetEpochsInfoWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetEpochsInfoWithRequest:(GetEpochsInfoRequest *)request handler:(void(^)(GetEpochsInfoResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getEpochsInfo"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetEpochsInfoResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getEpochsInfoWithMessage:(GetEpochsInfoRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getEpochsInfo"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetEpochsInfoResponse class]];
}

#pragma mark getContestedResources(GetContestedResourcesRequest) returns (GetContestedResourcesResponse)

/**
 * What votes are currently happening for a specific contested index
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (void)getContestedResourcesWithRequest:(GetContestedResourcesRequest *)request handler:(void(^)(GetContestedResourcesResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetContestedResourcesWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
/**
 * What votes are currently happening for a specific contested index
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (GRPCProtoCall *)RPCTogetContestedResourcesWithRequest:(GetContestedResourcesRequest *)request handler:(void(^)(GetContestedResourcesResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getContestedResources"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetContestedResourcesResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
/**
 * What votes are currently happening for a specific contested index
 */
- (GRPCUnaryProtoCall *)getContestedResourcesWithMessage:(GetContestedResourcesRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getContestedResources"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetContestedResourcesResponse class]];
}

#pragma mark getContestedResourceVoteState(GetContestedResourceVoteStateRequest) returns (GetContestedResourceVoteStateResponse)

/**
 * What's the state of a contested resource vote? (ie who is winning?)
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (void)getContestedResourceVoteStateWithRequest:(GetContestedResourceVoteStateRequest *)request handler:(void(^)(GetContestedResourceVoteStateResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetContestedResourceVoteStateWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
/**
 * What's the state of a contested resource vote? (ie who is winning?)
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (GRPCProtoCall *)RPCTogetContestedResourceVoteStateWithRequest:(GetContestedResourceVoteStateRequest *)request handler:(void(^)(GetContestedResourceVoteStateResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getContestedResourceVoteState"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetContestedResourceVoteStateResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
/**
 * What's the state of a contested resource vote? (ie who is winning?)
 */
- (GRPCUnaryProtoCall *)getContestedResourceVoteStateWithMessage:(GetContestedResourceVoteStateRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getContestedResourceVoteState"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetContestedResourceVoteStateResponse class]];
}

#pragma mark getContestedResourceVotersForIdentity(GetContestedResourceVotersForIdentityRequest) returns (GetContestedResourceVotersForIdentityResponse)

/**
 * Who voted for a contested resource to go to a specific identity?
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (void)getContestedResourceVotersForIdentityWithRequest:(GetContestedResourceVotersForIdentityRequest *)request handler:(void(^)(GetContestedResourceVotersForIdentityResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetContestedResourceVotersForIdentityWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
/**
 * Who voted for a contested resource to go to a specific identity?
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (GRPCProtoCall *)RPCTogetContestedResourceVotersForIdentityWithRequest:(GetContestedResourceVotersForIdentityRequest *)request handler:(void(^)(GetContestedResourceVotersForIdentityResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getContestedResourceVotersForIdentity"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetContestedResourceVotersForIdentityResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
/**
 * Who voted for a contested resource to go to a specific identity?
 */
- (GRPCUnaryProtoCall *)getContestedResourceVotersForIdentityWithMessage:(GetContestedResourceVotersForIdentityRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getContestedResourceVotersForIdentity"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetContestedResourceVotersForIdentityResponse class]];
}

#pragma mark getContestedResourceIdentityVotes(GetContestedResourceIdentityVotesRequest) returns (GetContestedResourceIdentityVotesResponse)

/**
 * How did an identity vote?
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (void)getContestedResourceIdentityVotesWithRequest:(GetContestedResourceIdentityVotesRequest *)request handler:(void(^)(GetContestedResourceIdentityVotesResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetContestedResourceIdentityVotesWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
/**
 * How did an identity vote?
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (GRPCProtoCall *)RPCTogetContestedResourceIdentityVotesWithRequest:(GetContestedResourceIdentityVotesRequest *)request handler:(void(^)(GetContestedResourceIdentityVotesResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getContestedResourceIdentityVotes"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetContestedResourceIdentityVotesResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
/**
 * How did an identity vote?
 */
- (GRPCUnaryProtoCall *)getContestedResourceIdentityVotesWithMessage:(GetContestedResourceIdentityVotesRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getContestedResourceIdentityVotes"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetContestedResourceIdentityVotesResponse class]];
}

#pragma mark getVotePollsByEndDate(GetVotePollsByEndDateRequest) returns (GetVotePollsByEndDateResponse)

/**
 * What vote polls will end soon?
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (void)getVotePollsByEndDateWithRequest:(GetVotePollsByEndDateRequest *)request handler:(void(^)(GetVotePollsByEndDateResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetVotePollsByEndDateWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
/**
 * What vote polls will end soon?
 *
 * This method belongs to a set of APIs that have been deprecated. Using the v2 API is recommended.
 */
- (GRPCProtoCall *)RPCTogetVotePollsByEndDateWithRequest:(GetVotePollsByEndDateRequest *)request handler:(void(^)(GetVotePollsByEndDateResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getVotePollsByEndDate"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetVotePollsByEndDateResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
/**
 * What vote polls will end soon?
 */
- (GRPCUnaryProtoCall *)getVotePollsByEndDateWithMessage:(GetVotePollsByEndDateRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getVotePollsByEndDate"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetVotePollsByEndDateResponse class]];
}

#pragma mark getPrefundedSpecializedBalance(GetPrefundedSpecializedBalanceRequest) returns (GetPrefundedSpecializedBalanceResponse)

- (void)getPrefundedSpecializedBalanceWithRequest:(GetPrefundedSpecializedBalanceRequest *)request handler:(void(^)(GetPrefundedSpecializedBalanceResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetPrefundedSpecializedBalanceWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetPrefundedSpecializedBalanceWithRequest:(GetPrefundedSpecializedBalanceRequest *)request handler:(void(^)(GetPrefundedSpecializedBalanceResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getPrefundedSpecializedBalance"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetPrefundedSpecializedBalanceResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getPrefundedSpecializedBalanceWithMessage:(GetPrefundedSpecializedBalanceRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getPrefundedSpecializedBalance"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetPrefundedSpecializedBalanceResponse class]];
}

#pragma mark getPathElements(GetPathElementsRequest) returns (GetPathElementsResponse)

- (void)getPathElementsWithRequest:(GetPathElementsRequest *)request handler:(void(^)(GetPathElementsResponse *_Nullable response, NSError *_Nullable error))handler{
  [[self RPCTogetPathElementsWithRequest:request handler:handler] start];
}
// Returns a not-yet-started RPC object.
- (GRPCProtoCall *)RPCTogetPathElementsWithRequest:(GetPathElementsRequest *)request handler:(void(^)(GetPathElementsResponse *_Nullable response, NSError *_Nullable error))handler{
  return [self RPCToMethod:@"getPathElements"
            requestsWriter:[GRXWriter writerWithValue:request]
             responseClass:[GetPathElementsResponse class]
        responsesWriteable:[GRXWriteable writeableWithSingleHandler:handler]];
}
- (GRPCUnaryProtoCall *)getPathElementsWithMessage:(GetPathElementsRequest *)message responseHandler:(id<GRPCProtoResponseHandler>)handler callOptions:(GRPCCallOptions *_Nullable)callOptions {
  return [self RPCToMethod:@"getPathElements"
                   message:message
           responseHandler:handler
               callOptions:callOptions
             responseClass:[GetPathElementsResponse class]];
}

@end
#endif
