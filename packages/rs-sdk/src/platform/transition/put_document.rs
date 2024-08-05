use crate::platform::transition::broadcast_request::BroadcastRequestForStateTransition;
use std::sync::Arc;
use dapi_grpc::platform::v0::WaitForStateTransitionResultResponse;

use crate::{Error, Sdk};

use crate::platform::block_info_from_metadata::block_info_from_metadata;
use crate::platform::transition::put_settings::PutSettings;
use dapi_grpc::platform::VersionedGrpcResponse;
use dpp::data_contract::document_type::accessors::DocumentTypeV0Getters;
use dpp::data_contract::document_type::DocumentType;
use dpp::data_contract::DataContract;
use dpp::document::{Document, DocumentV0Getters};
use dpp::identity::signer::Signer;
use dpp::identity::IdentityPublicKey;
use dpp::ProtocolError;
use dpp::state_transition::documents_batch_transition::methods::v0::DocumentsBatchTransitionMethodsV0;
use dpp::state_transition::documents_batch_transition::DocumentsBatchTransition;
use dpp::state_transition::proof_result::StateTransitionProofResult;
use dpp::state_transition::StateTransition;
use drive::drive::Drive;
use rs_dapi_client::{DapiRequest, RequestSettings};

#[async_trait::async_trait]
/// A trait for putting a document to platform
pub trait PutDocument<S: Signer> {
    /// Puts a document on platform
    /// setting settings to `None` sets default connection behavior
    async fn put_to_platform(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        document_state_transition_entropy: [u8; 32],
        identity_public_key: IdentityPublicKey,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error>;

    async fn replace_on_platform(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        identity_public_key: IdentityPublicKey,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error>;

    /// Waits for the response of a state transition after it has been broadcast
    async fn wait_for_response(
        &self,
        sdk: &Sdk,
        state_transition: StateTransition,
        data_contract: Arc<DataContract>,
        settings: Option<PutSettings>
    ) -> Result<Document, Error>;

    /// Puts an identity on platform and waits for the confirmation proof
    async fn put_to_platform_and_wait_for_response(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        document_state_transition_entropy: [u8; 32],
        identity_public_key: IdentityPublicKey,
        data_contract: Arc<DataContract>,
        signer: &S,
        settings: Option<PutSettings>
    ) -> Result<Document, Error>;

    async fn replace_on_platform_and_wait_for_response(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        identity_public_key: IdentityPublicKey,
        data_contract: Arc<DataContract>,
        signer: &S,
        settings: Option<PutSettings>
    ) -> Result<Document, Error>;
}

use dapi_grpc::platform::v0::StateTransitionBroadcastError;
use dapi_grpc::platform::v0::wait_for_state_transition_result_response::wait_for_state_transition_result_response_v0;
use dapi_grpc::platform::v0::wait_for_state_transition_result_response::Version::V0;
fn get_error(response: &WaitForStateTransitionResultResponse) -> Option<&StateTransitionBroadcastError> {
    match &response.version {
        Some(V0(responseV0)) => {
            return match &responseV0.result {
                Some(wait_for_state_transition_result_response_v0::Result::Error(error)) => Some(&error),
                _ => None
            }
        }
        _ => {}
    }
    None
}

#[async_trait::async_trait]
impl<S: Signer> PutDocument<S> for Document {
    async fn put_to_platform(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        document_state_transition_entropy: [u8; 32],
        identity_public_key: IdentityPublicKey,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error> {
        tracing::trace!("PutDocument::put_to_platform");
        let new_identity_contract_nonce = sdk
            .get_identity_contract_nonce(
                self.owner_id(),
                document_type.data_contract_id(),
                true,
                settings,
            )
            .await?;
        tracing::trace!("PutDocument::put_to_platform, nonce: {:?}", new_identity_contract_nonce);
        let settings = settings.unwrap_or_default();

        let transition = DocumentsBatchTransition::new_document_creation_transition_from_document(
            self.clone(),
            document_type.as_ref(),
            document_state_transition_entropy,
            &identity_public_key,
            new_identity_contract_nonce,
            settings.user_fee_increase.unwrap_or_default(),
            signer,
            sdk.version(),
            None,
            None,
            None,
        )?;

        tracing::trace!("PutDocument::put_to_platform, transition: {:?}", transition);
        let request = transition.broadcast_request_for_state_transition()?;
        tracing::trace!("PutDocument::put_to_platform, request: {:?}", request);

        let response = request
            .clone()
            .execute(sdk, settings.request_settings)
            .await;

        match response {
            Ok(r) => tracing::trace!("PutDocument::put_to_platform, response: {:?}", r),
            Err(e) => {
                tracing::trace!("PutDocument::put_to_platform, response error: {:?}", e);
                return Err(Error::from(e));
            }
        }

        // response is empty for a broadcast, result comes from the stream wait for state transition result
        tracing::trace!("PutDocument::put_to_platform, returning: {:?}", transition);
        Ok(transition)
    }

    async fn replace_on_platform(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        identity_public_key: IdentityPublicKey,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error> {
        tracing::trace!("PutDocument::put_to_platform");
        let new_identity_contract_nonce = sdk
            .get_identity_contract_nonce(
                self.owner_id(),
                document_type.data_contract_id(),
                true,
                settings,
            )
            .await?;
        tracing::trace!("PutDocument::put_to_platform, nonce: {:?}", new_identity_contract_nonce);
        let settings = settings.unwrap_or_default();

        let transition = DocumentsBatchTransition::new_document_replacement_transition_from_document(
            self.clone(),
            document_type.as_ref(),
            &identity_public_key,
            new_identity_contract_nonce,
            settings.user_fee_increase.unwrap_or_default(),
            signer,
            sdk.version(),
            None,
            None,
            None,
        )?;

        tracing::trace!("PutDocument::put_to_platform, transition: {:?}", transition);
        let request = transition.broadcast_request_for_state_transition()?;
        tracing::trace!("PutDocument::put_to_platform, request: {:?}", request);

        let response = request
            .clone()
            .execute(sdk, settings.request_settings)
            .await;

        match response {
            Ok(r) => tracing::trace!("PutDocument::put_to_platform, response: {:?}", r),
            Err(e) => {
                tracing::trace!("PutDocument::put_to_platform, response error: {:?}", e);
                return Err(Error::from(e));
            }
        }

        // response is empty for a broadcast, result comes from the stream wait for state transition result
        tracing::trace!("PutDocument::put_to_platform, returning: {:?}", transition);
        Ok(transition)
    }



    async fn wait_for_response(
        &self,
        sdk: &Sdk,
        state_transition: StateTransition,
        data_contract: Arc<DataContract>,
        settings: Option<PutSettings>
    ) -> Result<Document, Error> {
        tracing::trace!("PutDocument::wait_for_response: {:?}", state_transition);
        let request = state_transition.wait_for_state_transition_result_request()?;
        tracing::trace!("PutDocument::wait_for_response, request: {:?}", request);

        let request_settings = match settings {
            Some(put_settings) => put_settings.request_settings,
            None => RequestSettings::default()
        };

        let response = request.execute(sdk, request_settings).await?;
        tracing::trace!("PutDocument::wait_for_response, response: {:?}", response);

        // look at error here
        match get_error(&response) {
            Some(e) => {
                return Err(Error::Protocol(ProtocolError::Generic(e.message.to_string())))
            },
            None => {}
        }

        let block_info = block_info_from_metadata(response.metadata()?)?;

        let proof = response.proof_owned()?;

        let (_, result) = Drive::verify_state_transition_was_executed_with_proof(
            &state_transition,
            &block_info,
            proof.grovedb_proof.as_slice(),
            &|_| Ok(Some(data_contract.clone())),
            sdk.version(),
        )?;

        tracing::trace!("PutDocument::wait_for_response, result: {:?}", result);
        //todo verify

        match result {
            StateTransitionProofResult::VerifiedDocuments(mut documents) => {
                let document = documents
                    .remove(self.id_ref())
                    .ok_or(Error::InvalidProvedResponse(
                        "did not prove the sent document".to_string(),
                    ))?
                    .ok_or(Error::InvalidProvedResponse(
                        "expected there to actually be a document".to_string(),
                    ))?;
                Ok(document)
            }
            _ => Err(Error::DapiClientError("proved a non document".to_string())),
        }
    }

    async fn put_to_platform_and_wait_for_response(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        document_state_transition_entropy: [u8; 32],
        identity_public_key: IdentityPublicKey,
        data_contract: Arc<DataContract>,
        signer: &S,
        settings: Option<PutSettings>
    ) -> Result<Document, Error> {
        tracing::trace!("preparing put document to platform: {:?}", self);
        let state_transition = self
            .put_to_platform(
                sdk,
                document_type,
                document_state_transition_entropy,
                identity_public_key,
                signer,
                settings,
            )
            .await?;
        tracing::trace!("put document to platform complete: {} {:?}", hex::encode(state_transition.transaction_id().unwrap()), state_transition);

        // TODO: Why do we need full type annotation?
        let document =
            <Self as PutDocument<S>>::wait_for_response(self, sdk, state_transition, data_contract, settings)
                .await?;
        tracing::trace!("waiting for document  complete: {:?}", document);
        Ok(document)
    }

    async fn replace_on_platform_and_wait_for_response(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        identity_public_key: IdentityPublicKey,
        data_contract: Arc<DataContract>,
        signer: &S,
        settings: Option<PutSettings>
    ) -> Result<Document, Error> {
        tracing::trace!("preparing replace document on platform: {:?}", self);
        let state_transition = self
            .replace_on_platform(
                sdk,
                document_type,
                identity_public_key,
                signer,
                settings,
            )
            .await?;
        tracing::trace!("replace document to platform complete: {} {:?}", hex::encode(state_transition.transaction_id().unwrap()), state_transition);

        // TODO: Why do we need full type annotation?
        let document =
            <Self as PutDocument<S>>::wait_for_response(self, sdk, state_transition, data_contract, settings)
                .await?;
        tracing::trace!("waiting for replace document  complete: {:?}", document);
        Ok(document)
    }
}
