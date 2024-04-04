use crate::state_transition_action::identity::identity_create::IdentityCreateTransitionAction;
use crate::state_transition_action::identity::identity_topup::IdentityTopUpTransitionAction;
use crate::state_transition_action::system::partially_use_asset_lock_action::v0::PartiallyUseAssetLockActionV0;
use crate::state_transition_action::system::partially_use_asset_lock_action::PartiallyUseAssetLockAction;
use dpp::consensus::ConsensusError;
use dpp::fee::Credits;
use dpp::state_transition::identity_create_transition::IdentityCreateTransition;
use dpp::state_transition::identity_topup_transition::IdentityTopUpTransition;

impl PartiallyUseAssetLockAction {
    /// try from identity create transition
    pub fn try_from_identity_create_transition(
        value: IdentityCreateTransition,
        asset_lock_initial_balance_amount: Credits,
        asset_lock_output_script: Vec<u8>,
        asset_lock_remaining_balance_amount: Credits,
        used_credits: Credits,
    ) -> Result<Self, ConsensusError> {
        match value {
            IdentityCreateTransition::V0(v0) => Ok(
                PartiallyUseAssetLockActionV0::try_from_identity_create_transition(
                    v0,
                    asset_lock_initial_balance_amount,
                    asset_lock_output_script,
                    asset_lock_remaining_balance_amount,
                    used_credits,
                )?
                .into(),
            ),
        }
    }

    /// try from borrowed identity create transition
    pub fn try_from_borrowed_identity_create_transition(
        value: &IdentityCreateTransition,
        asset_lock_initial_balance_amount: Credits,
        asset_lock_output_script: Vec<u8>,
        asset_lock_remaining_balance_amount: Credits,
        used_credits: Credits,
    ) -> Result<Self, ConsensusError> {
        match value {
            IdentityCreateTransition::V0(v0) => Ok(
                PartiallyUseAssetLockActionV0::try_from_borrowed_identity_create_transition(
                    v0,
                    asset_lock_initial_balance_amount,
                    asset_lock_output_script,
                    asset_lock_remaining_balance_amount,
                    used_credits,
                )?
                .into(),
            ),
        }
    }

    /// from identity create transition action
    pub fn from_identity_create_transition_action(
        value: IdentityCreateTransitionAction,
        used_credits: Credits,
    ) -> Self {
        match value {
            IdentityCreateTransitionAction::V0(v0) => {
                PartiallyUseAssetLockActionV0::from_identity_create_transition_action(
                    v0,
                    used_credits,
                )
                .into()
            }
        }
    }

    /// from borrowed identity create transition action
    pub fn from_borrowed_identity_create_transition_action(
        value: &IdentityCreateTransitionAction,
        used_credits: Credits,
    ) -> Self {
        match value {
            IdentityCreateTransitionAction::V0(v0) => {
                PartiallyUseAssetLockActionV0::from_borrowed_identity_create_transition_action(
                    v0,
                    used_credits,
                )
                .into()
            }
        }
    }

    /// try from identity top up transition
    pub fn try_from_identity_top_up_transition(
        value: IdentityTopUpTransition,
        asset_lock_initial_balance_amount: Credits,
        asset_lock_output_script: Vec<u8>,
        asset_lock_remaining_balance_amount: Credits,
        used_credits: Credits,
    ) -> Result<Self, ConsensusError> {
        match value {
            IdentityTopUpTransition::V0(v0) => Ok(
                PartiallyUseAssetLockActionV0::try_from_identity_top_up_transition(
                    v0,
                    asset_lock_initial_balance_amount,
                    asset_lock_output_script,
                    asset_lock_remaining_balance_amount,
                    used_credits,
                )?
                .into(),
            ),
        }
    }

    /// try from borrowed identity top up transition
    pub fn try_from_borrowed_identity_top_up_transition(
        value: &IdentityTopUpTransition,
        asset_lock_initial_balance_amount: Credits,
        asset_lock_output_script: Vec<u8>,
        asset_lock_remaining_balance_amount: Credits,
        used_credits: Credits,
    ) -> Result<Self, ConsensusError> {
        match value {
            IdentityTopUpTransition::V0(v0) => Ok(
                PartiallyUseAssetLockActionV0::try_from_borrowed_identity_top_up_transition(
                    v0,
                    asset_lock_initial_balance_amount,
                    asset_lock_output_script,
                    asset_lock_remaining_balance_amount,
                    used_credits,
                )?
                .into(),
            ),
        }
    }

    /// from identity top up transition action
    pub fn from_identity_top_up_transition_action(
        value: IdentityTopUpTransitionAction,
        used_credits: Credits,
    ) -> Self {
        match value {
            IdentityTopUpTransitionAction::V0(v0) => {
                PartiallyUseAssetLockActionV0::from_identity_top_up_transition_action(
                    v0,
                    used_credits,
                )
                .into()
            }
        }
    }

    /// from borrowed identity top up transition action
    pub fn from_borrowed_identity_top_up_transition_action(
        value: &IdentityTopUpTransitionAction,
        used_credits: Credits,
    ) -> Self {
        match value {
            IdentityTopUpTransitionAction::V0(v0) => {
                PartiallyUseAssetLockActionV0::from_borrowed_identity_top_up_transition_action(
                    v0,
                    used_credits,
                )
                .into()
            }
        }
    }
}