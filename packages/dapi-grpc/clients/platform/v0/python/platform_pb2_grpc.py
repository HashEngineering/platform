# Generated by the gRPC Python protocol compiler plugin. DO NOT EDIT!
"""Client and server classes corresponding to protobuf-defined services."""
import grpc

import platform_pb2 as platform__pb2


class PlatformStub(object):
    """Missing associated documentation comment in .proto file."""

    def __init__(self, channel):
        """Constructor.

        Args:
            channel: A grpc.Channel.
        """
        self.broadcastStateTransition = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/broadcastStateTransition',
                request_serializer=platform__pb2.BroadcastStateTransitionRequest.SerializeToString,
                response_deserializer=platform__pb2.BroadcastStateTransitionResponse.FromString,
                )
        self.getIdentity = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentity',
                request_serializer=platform__pb2.GetIdentityRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityResponse.FromString,
                )
        self.getIdentityKeys = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentityKeys',
                request_serializer=platform__pb2.GetIdentityKeysRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityKeysResponse.FromString,
                )
        self.getIdentitiesContractKeys = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentitiesContractKeys',
                request_serializer=platform__pb2.GetIdentitiesContractKeysRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentitiesContractKeysResponse.FromString,
                )
        self.getIdentityNonce = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentityNonce',
                request_serializer=platform__pb2.GetIdentityNonceRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityNonceResponse.FromString,
                )
        self.getIdentityContractNonce = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentityContractNonce',
                request_serializer=platform__pb2.GetIdentityContractNonceRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityContractNonceResponse.FromString,
                )
        self.getIdentityBalance = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentityBalance',
                request_serializer=platform__pb2.GetIdentityBalanceRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityBalanceResponse.FromString,
                )
        self.getIdentityBalanceAndRevision = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentityBalanceAndRevision',
                request_serializer=platform__pb2.GetIdentityBalanceAndRevisionRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityBalanceAndRevisionResponse.FromString,
                )
        self.getProofs = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getProofs',
                request_serializer=platform__pb2.GetProofsRequest.SerializeToString,
                response_deserializer=platform__pb2.GetProofsResponse.FromString,
                )
        self.getDataContract = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getDataContract',
                request_serializer=platform__pb2.GetDataContractRequest.SerializeToString,
                response_deserializer=platform__pb2.GetDataContractResponse.FromString,
                )
        self.getDataContractHistory = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getDataContractHistory',
                request_serializer=platform__pb2.GetDataContractHistoryRequest.SerializeToString,
                response_deserializer=platform__pb2.GetDataContractHistoryResponse.FromString,
                )
        self.getDataContracts = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getDataContracts',
                request_serializer=platform__pb2.GetDataContractsRequest.SerializeToString,
                response_deserializer=platform__pb2.GetDataContractsResponse.FromString,
                )
        self.getDocuments = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getDocuments',
                request_serializer=platform__pb2.GetDocumentsRequest.SerializeToString,
                response_deserializer=platform__pb2.GetDocumentsResponse.FromString,
                )
        self.getIdentityByPublicKeyHash = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getIdentityByPublicKeyHash',
                request_serializer=platform__pb2.GetIdentityByPublicKeyHashRequest.SerializeToString,
                response_deserializer=platform__pb2.GetIdentityByPublicKeyHashResponse.FromString,
                )
        self.waitForStateTransitionResult = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/waitForStateTransitionResult',
                request_serializer=platform__pb2.WaitForStateTransitionResultRequest.SerializeToString,
                response_deserializer=platform__pb2.WaitForStateTransitionResultResponse.FromString,
                )
        self.getConsensusParams = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getConsensusParams',
                request_serializer=platform__pb2.GetConsensusParamsRequest.SerializeToString,
                response_deserializer=platform__pb2.GetConsensusParamsResponse.FromString,
                )
        self.getProtocolVersionUpgradeState = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getProtocolVersionUpgradeState',
                request_serializer=platform__pb2.GetProtocolVersionUpgradeStateRequest.SerializeToString,
                response_deserializer=platform__pb2.GetProtocolVersionUpgradeStateResponse.FromString,
                )
        self.getProtocolVersionUpgradeVoteStatus = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getProtocolVersionUpgradeVoteStatus',
                request_serializer=platform__pb2.GetProtocolVersionUpgradeVoteStatusRequest.SerializeToString,
                response_deserializer=platform__pb2.GetProtocolVersionUpgradeVoteStatusResponse.FromString,
                )
        self.getEpochsInfo = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getEpochsInfo',
                request_serializer=platform__pb2.GetEpochsInfoRequest.SerializeToString,
                response_deserializer=platform__pb2.GetEpochsInfoResponse.FromString,
                )
        self.getPathElements = channel.unary_unary(
                '/org.dash.platform.dapi.v0.Platform/getPathElements',
                request_serializer=platform__pb2.GetPathElementsRequest.SerializeToString,
                response_deserializer=platform__pb2.GetPathElementsResponse.FromString,
                )


class PlatformServicer(object):
    """Missing associated documentation comment in .proto file."""

    def broadcastStateTransition(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentity(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentityKeys(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentitiesContractKeys(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentityNonce(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentityContractNonce(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentityBalance(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentityBalanceAndRevision(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getProofs(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getDataContract(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getDataContractHistory(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getDataContracts(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getDocuments(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getIdentityByPublicKeyHash(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def waitForStateTransitionResult(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getConsensusParams(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getProtocolVersionUpgradeState(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getProtocolVersionUpgradeVoteStatus(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getEpochsInfo(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')

    def getPathElements(self, request, context):
        """Missing associated documentation comment in .proto file."""
        context.set_code(grpc.StatusCode.UNIMPLEMENTED)
        context.set_details('Method not implemented!')
        raise NotImplementedError('Method not implemented!')


def add_PlatformServicer_to_server(servicer, server):
    rpc_method_handlers = {
            'broadcastStateTransition': grpc.unary_unary_rpc_method_handler(
                    servicer.broadcastStateTransition,
                    request_deserializer=platform__pb2.BroadcastStateTransitionRequest.FromString,
                    response_serializer=platform__pb2.BroadcastStateTransitionResponse.SerializeToString,
            ),
            'getIdentity': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentity,
                    request_deserializer=platform__pb2.GetIdentityRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityResponse.SerializeToString,
            ),
            'getIdentityKeys': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentityKeys,
                    request_deserializer=platform__pb2.GetIdentityKeysRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityKeysResponse.SerializeToString,
            ),
            'getIdentitiesContractKeys': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentitiesContractKeys,
                    request_deserializer=platform__pb2.GetIdentitiesContractKeysRequest.FromString,
                    response_serializer=platform__pb2.GetIdentitiesContractKeysResponse.SerializeToString,
            ),
            'getIdentityNonce': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentityNonce,
                    request_deserializer=platform__pb2.GetIdentityNonceRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityNonceResponse.SerializeToString,
            ),
            'getIdentityContractNonce': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentityContractNonce,
                    request_deserializer=platform__pb2.GetIdentityContractNonceRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityContractNonceResponse.SerializeToString,
            ),
            'getIdentityBalance': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentityBalance,
                    request_deserializer=platform__pb2.GetIdentityBalanceRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityBalanceResponse.SerializeToString,
            ),
            'getIdentityBalanceAndRevision': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentityBalanceAndRevision,
                    request_deserializer=platform__pb2.GetIdentityBalanceAndRevisionRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityBalanceAndRevisionResponse.SerializeToString,
            ),
            'getProofs': grpc.unary_unary_rpc_method_handler(
                    servicer.getProofs,
                    request_deserializer=platform__pb2.GetProofsRequest.FromString,
                    response_serializer=platform__pb2.GetProofsResponse.SerializeToString,
            ),
            'getDataContract': grpc.unary_unary_rpc_method_handler(
                    servicer.getDataContract,
                    request_deserializer=platform__pb2.GetDataContractRequest.FromString,
                    response_serializer=platform__pb2.GetDataContractResponse.SerializeToString,
            ),
            'getDataContractHistory': grpc.unary_unary_rpc_method_handler(
                    servicer.getDataContractHistory,
                    request_deserializer=platform__pb2.GetDataContractHistoryRequest.FromString,
                    response_serializer=platform__pb2.GetDataContractHistoryResponse.SerializeToString,
            ),
            'getDataContracts': grpc.unary_unary_rpc_method_handler(
                    servicer.getDataContracts,
                    request_deserializer=platform__pb2.GetDataContractsRequest.FromString,
                    response_serializer=platform__pb2.GetDataContractsResponse.SerializeToString,
            ),
            'getDocuments': grpc.unary_unary_rpc_method_handler(
                    servicer.getDocuments,
                    request_deserializer=platform__pb2.GetDocumentsRequest.FromString,
                    response_serializer=platform__pb2.GetDocumentsResponse.SerializeToString,
            ),
            'getIdentityByPublicKeyHash': grpc.unary_unary_rpc_method_handler(
                    servicer.getIdentityByPublicKeyHash,
                    request_deserializer=platform__pb2.GetIdentityByPublicKeyHashRequest.FromString,
                    response_serializer=platform__pb2.GetIdentityByPublicKeyHashResponse.SerializeToString,
            ),
            'waitForStateTransitionResult': grpc.unary_unary_rpc_method_handler(
                    servicer.waitForStateTransitionResult,
                    request_deserializer=platform__pb2.WaitForStateTransitionResultRequest.FromString,
                    response_serializer=platform__pb2.WaitForStateTransitionResultResponse.SerializeToString,
            ),
            'getConsensusParams': grpc.unary_unary_rpc_method_handler(
                    servicer.getConsensusParams,
                    request_deserializer=platform__pb2.GetConsensusParamsRequest.FromString,
                    response_serializer=platform__pb2.GetConsensusParamsResponse.SerializeToString,
            ),
            'getProtocolVersionUpgradeState': grpc.unary_unary_rpc_method_handler(
                    servicer.getProtocolVersionUpgradeState,
                    request_deserializer=platform__pb2.GetProtocolVersionUpgradeStateRequest.FromString,
                    response_serializer=platform__pb2.GetProtocolVersionUpgradeStateResponse.SerializeToString,
            ),
            'getProtocolVersionUpgradeVoteStatus': grpc.unary_unary_rpc_method_handler(
                    servicer.getProtocolVersionUpgradeVoteStatus,
                    request_deserializer=platform__pb2.GetProtocolVersionUpgradeVoteStatusRequest.FromString,
                    response_serializer=platform__pb2.GetProtocolVersionUpgradeVoteStatusResponse.SerializeToString,
            ),
            'getEpochsInfo': grpc.unary_unary_rpc_method_handler(
                    servicer.getEpochsInfo,
                    request_deserializer=platform__pb2.GetEpochsInfoRequest.FromString,
                    response_serializer=platform__pb2.GetEpochsInfoResponse.SerializeToString,
            ),
            'getPathElements': grpc.unary_unary_rpc_method_handler(
                    servicer.getPathElements,
                    request_deserializer=platform__pb2.GetPathElementsRequest.FromString,
                    response_serializer=platform__pb2.GetPathElementsResponse.SerializeToString,
            ),
    }
    generic_handler = grpc.method_handlers_generic_handler(
            'org.dash.platform.dapi.v0.Platform', rpc_method_handlers)
    server.add_generic_rpc_handlers((generic_handler,))


 # This class is part of an EXPERIMENTAL API.
class Platform(object):
    """Missing associated documentation comment in .proto file."""

    @staticmethod
    def broadcastStateTransition(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/broadcastStateTransition',
            platform__pb2.BroadcastStateTransitionRequest.SerializeToString,
            platform__pb2.BroadcastStateTransitionResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentity(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentity',
            platform__pb2.GetIdentityRequest.SerializeToString,
            platform__pb2.GetIdentityResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentityKeys(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentityKeys',
            platform__pb2.GetIdentityKeysRequest.SerializeToString,
            platform__pb2.GetIdentityKeysResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentitiesContractKeys(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentitiesContractKeys',
            platform__pb2.GetIdentitiesContractKeysRequest.SerializeToString,
            platform__pb2.GetIdentitiesContractKeysResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentityNonce(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentityNonce',
            platform__pb2.GetIdentityNonceRequest.SerializeToString,
            platform__pb2.GetIdentityNonceResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentityContractNonce(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentityContractNonce',
            platform__pb2.GetIdentityContractNonceRequest.SerializeToString,
            platform__pb2.GetIdentityContractNonceResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentityBalance(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentityBalance',
            platform__pb2.GetIdentityBalanceRequest.SerializeToString,
            platform__pb2.GetIdentityBalanceResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentityBalanceAndRevision(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentityBalanceAndRevision',
            platform__pb2.GetIdentityBalanceAndRevisionRequest.SerializeToString,
            platform__pb2.GetIdentityBalanceAndRevisionResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getProofs(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getProofs',
            platform__pb2.GetProofsRequest.SerializeToString,
            platform__pb2.GetProofsResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getDataContract(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getDataContract',
            platform__pb2.GetDataContractRequest.SerializeToString,
            platform__pb2.GetDataContractResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getDataContractHistory(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getDataContractHistory',
            platform__pb2.GetDataContractHistoryRequest.SerializeToString,
            platform__pb2.GetDataContractHistoryResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getDataContracts(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getDataContracts',
            platform__pb2.GetDataContractsRequest.SerializeToString,
            platform__pb2.GetDataContractsResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getDocuments(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getDocuments',
            platform__pb2.GetDocumentsRequest.SerializeToString,
            platform__pb2.GetDocumentsResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getIdentityByPublicKeyHash(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getIdentityByPublicKeyHash',
            platform__pb2.GetIdentityByPublicKeyHashRequest.SerializeToString,
            platform__pb2.GetIdentityByPublicKeyHashResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def waitForStateTransitionResult(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/waitForStateTransitionResult',
            platform__pb2.WaitForStateTransitionResultRequest.SerializeToString,
            platform__pb2.WaitForStateTransitionResultResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getConsensusParams(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getConsensusParams',
            platform__pb2.GetConsensusParamsRequest.SerializeToString,
            platform__pb2.GetConsensusParamsResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getProtocolVersionUpgradeState(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getProtocolVersionUpgradeState',
            platform__pb2.GetProtocolVersionUpgradeStateRequest.SerializeToString,
            platform__pb2.GetProtocolVersionUpgradeStateResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getProtocolVersionUpgradeVoteStatus(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getProtocolVersionUpgradeVoteStatus',
            platform__pb2.GetProtocolVersionUpgradeVoteStatusRequest.SerializeToString,
            platform__pb2.GetProtocolVersionUpgradeVoteStatusResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getEpochsInfo(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getEpochsInfo',
            platform__pb2.GetEpochsInfoRequest.SerializeToString,
            platform__pb2.GetEpochsInfoResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)

    @staticmethod
    def getPathElements(request,
            target,
            options=(),
            channel_credentials=None,
            call_credentials=None,
            insecure=False,
            compression=None,
            wait_for_ready=None,
            timeout=None,
            metadata=None):
        return grpc.experimental.unary_unary(request, target, '/org.dash.platform.dapi.v0.Platform/getPathElements',
            platform__pb2.GetPathElementsRequest.SerializeToString,
            platform__pb2.GetPathElementsResponse.FromString,
            options, channel_credentials,
            insecure, call_credentials, compression, wait_for_ready, timeout, metadata)
