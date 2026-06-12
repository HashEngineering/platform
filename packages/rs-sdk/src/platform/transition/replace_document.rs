use std::sync::Arc;
use dpp::data_contract::DataContract;
use dpp::data_contract::document_type::accessors::DocumentTypeV0Getters;
use dpp::data_contract::document_type::DocumentType;
use dpp::document::{Document, DocumentV0Getters};
use dpp::identity::IdentityPublicKey;
use dpp::identity::signer::Signer;
use dpp::ProtocolError;
use dpp::state_transition::batch_transition::BatchTransition;
use dpp::state_transition::batch_transition::methods::v0::DocumentsBatchTransitionMethodsV0;
use dpp::state_transition::proof_result::StateTransitionProofResult;
use dpp::state_transition::StateTransition;
use drive::drive::Drive;
use rs_dapi_client::{DapiRequest, RequestSettings};
use crate::platform::transition::put_settings::PutSettings;
use crate::{Error, Sdk};
use crate::platform::block_info_from_metadata::block_info_from_metadata;
use crate::platform::transition::broadcast_request::BroadcastRequestForStateTransition;
use crate::platform::transition::put_document::PutDocument;
use dapi_grpc::platform::VersionedGrpcResponse;
use crate::platform::transition::broadcast::BroadcastStateTransition;
use crate::platform::transition::waitable::Waitable;

#[async_trait::async_trait]
/// A trait for replacing a document on platform
pub trait ReplaceDocument<S: Signer<IdentityPublicKey>>: Waitable {
    /// Replaces a document on platform
    /// setting settings to `None` sets default connection behavior
    async fn replace_on_platform(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        identity_public_key: IdentityPublicKey,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error>;


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

#[async_trait::async_trait]
impl<S: Signer<IdentityPublicKey>> ReplaceDocument<S> for Document {
    async fn replace_on_platform(
        &self,
        sdk: &Sdk,
        document_type: DocumentType,
        identity_public_key: IdentityPublicKey,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error> {
        let new_identity_contract_nonce = sdk
            .get_identity_contract_nonce(
                self.owner_id(),
                document_type.data_contract_id(),
                true,
                settings,
            )
            .await?;
        tracing::trace!("ReplaceDocument::put_to_platform, nonce: {:?}", new_identity_contract_nonce);
        let settings = settings.unwrap_or_default();

        let transition = BatchTransition::new_document_replacement_transition_from_document(
            self.clone(),
            document_type.as_ref(),
            &identity_public_key,
            new_identity_contract_nonce,
            settings.user_fee_increase.unwrap_or_default(),
            None,
            signer,
            sdk.version(),
            None
        ).await?;

        // response is empty for a broadcast, result comes from the stream wait for state transition result
        transition.broadcast(sdk, Some(settings)).await?;

        tracing::trace!("ReplaceDocument::put_to_platform, returning: {:?}", transition);
        Ok(transition)
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

        Self::wait_for_response(sdk, state_transition, settings).await
    }
}