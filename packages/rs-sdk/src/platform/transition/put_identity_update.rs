use crate::platform::transition::broadcast::BroadcastStateTransition;
use crate::platform::transition::put_settings::PutSettings;
use crate::platform::transition::waitable::Waitable;
use crate::{Error, Sdk};
use dpp::identity::accessors::IdentityGettersV0;
use dpp::identity::signer::Signer;
use dpp::identity::{Identity, IdentityPublicKey, KeyID};
use dpp::state_transition::identity_update_transition::methods::IdentityUpdateTransitionMethodsV0;
use dpp::state_transition::identity_update_transition::IdentityUpdateTransition;
use dpp::state_transition::StateTransition;

/// A trait for updating an identity on platform using an [`IdentityUpdateTransition`].
///
/// This trait allows adding new public keys or disabling existing public keys on an identity.
/// The transition must be signed with the identity's master key.
#[async_trait::async_trait]
pub trait PutIdentityUpdate<S: Signer<IdentityPublicKey>>: Sized {
    /// Builds and broadcasts an [`IdentityUpdateTransition`] to platform.
    ///
    /// Returns the broadcast [`StateTransition`] without waiting for confirmation.
    ///
    /// # Arguments
    ///
    /// * `sdk` - The SDK instance to use for broadcasting.
    /// * `master_public_key_id` - The ID of the identity's master key used to sign the transition.
    /// * `add_public_keys` - Public keys to add to the identity.
    /// * `disable_public_keys` - IDs of existing public keys to disable on the identity.
    /// * `signer` - The signer that holds the master key (and any new unique keys to sign).
    /// * `settings` - Optional broadcast settings (fee increase, request settings, etc.).
    async fn put_identity_update_to_platform(
        &self,
        sdk: &Sdk,
        master_public_key_id: &KeyID,
        add_public_keys: Vec<IdentityPublicKey>,
        disable_public_keys: Vec<KeyID>,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error>;

    /// Builds and broadcasts an [`IdentityUpdateTransition`] to platform, then waits for
    /// the confirmation proof.
    ///
    /// Returns the full [`Identity`] after the update is confirmed on platform.
    ///
    /// Drive proves an identity update with a [`PartialIdentity`] (not a full [`Identity`]),
    /// so this method fetches the complete identity after proof confirmation.
    ///
    /// # Arguments
    ///
    /// * `sdk` - The SDK instance to use for broadcasting.
    /// * `master_public_key_id` - The ID of the identity's master key used to sign the transition.
    /// * `add_public_keys` - Public keys to add to the identity.
    /// * `disable_public_keys` - IDs of existing public keys to disable on the identity.
    /// * `signer` - The signer that holds the master key (and any new unique keys to sign).
    /// * `settings` - Optional broadcast settings (fee increase, request settings, timeout, etc.).
    async fn put_identity_update_to_platform_and_wait_for_response(
        &self,
        sdk: &Sdk,
        master_public_key_id: &KeyID,
        add_public_keys: Vec<IdentityPublicKey>,
        disable_public_keys: Vec<KeyID>,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<Identity, Error>;
}

#[async_trait::async_trait]
impl<S: Signer<IdentityPublicKey> + Send + Sync> PutIdentityUpdate<S> for Identity {
    async fn put_identity_update_to_platform(
        &self,
        sdk: &Sdk,
        master_public_key_id: &KeyID,
        add_public_keys: Vec<IdentityPublicKey>,
        disable_public_keys: Vec<KeyID>,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<StateTransition, Error> {
        let nonce = sdk.get_identity_nonce(self.id(), true, settings).await?;
        let user_fee_increase = settings
            .and_then(|s| s.user_fee_increase)
            .unwrap_or_default();

        let state_transition = IdentityUpdateTransition::try_from_identity_with_signer(
            self,
            master_public_key_id,
            add_public_keys,
            disable_public_keys,
            nonce,
            user_fee_increase,
            signer,
            sdk.version(),
            None,
        ).await?;

        state_transition.broadcast(sdk, settings).await?;
        Ok(state_transition)
    }

    async fn put_identity_update_to_platform_and_wait_for_response(
        &self,
        sdk: &Sdk,
        master_public_key_id: &KeyID,
        add_public_keys: Vec<IdentityPublicKey>,
        disable_public_keys: Vec<KeyID>,
        signer: &S,
        settings: Option<PutSettings>,
    ) -> Result<Identity, Error> {
        let state_transition = self
            .put_identity_update_to_platform(
                sdk,
                master_public_key_id,
                add_public_keys,
                disable_public_keys,
                signer,
                settings,
            )
            .await?;

        Identity::wait_for_response(sdk, state_transition, settings).await
    }
}
