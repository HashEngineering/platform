mod balance;
mod nonce;
mod state;
mod structure;

use dpp::block::block_info::BlockInfo;
use dpp::state_transition::identity_credit_withdrawal_transition::IdentityCreditWithdrawalTransition;
use dpp::validation::{ConsensusValidationResult, SimpleConsensusValidationResult};
use dpp::version::PlatformVersion;
use drive::state_transition_action::StateTransitionAction;

use drive::grovedb::TransactionArg;

use crate::error::execution::ExecutionError;
use crate::error::Error;
use crate::execution::types::state_transition_execution_context::StateTransitionExecutionContext;
use crate::platform_types::platform::PlatformRef;
use crate::rpc::core::CoreRPCLike;

use crate::execution::validation::state_transition::identity_credit_withdrawal::state::v0::IdentityCreditWithdrawalStateTransitionStateValidationV0;
use crate::execution::validation::state_transition::identity_credit_withdrawal::structure::v0::IdentityCreditWithdrawalStateTransitionStructureValidationV0;

use crate::execution::validation::state_transition::processor::v0::{
    StateTransitionBasicStructureValidationV0, StateTransitionStateValidationV0,
};
use crate::execution::validation::state_transition::transformer::StateTransitionActionTransformerV0;
use crate::execution::validation::state_transition::ValidationMode;

impl StateTransitionActionTransformerV0 for IdentityCreditWithdrawalTransition {
    fn transform_into_action<C: CoreRPCLike>(
        &self,
        platform: &PlatformRef<C>,
        _block_info: &BlockInfo,
        _validation_mode: ValidationMode,
        _execution_context: &mut StateTransitionExecutionContext,
        _tx: TransactionArg,
    ) -> Result<ConsensusValidationResult<StateTransitionAction>, Error> {
        let platform_version = platform.state.current_platform_version()?;

        match platform_version
            .drive_abci
            .validation_and_processing
            .state_transitions
            .identity_credit_withdrawal_state_transition
            .transform_into_action
        {
            0 => self.transform_into_action_v0(platform),
            version => Err(Error::Execution(ExecutionError::UnknownVersionMismatch {
                method: "identity credit withdrawal transition: transform_into_action".to_string(),
                known_versions: vec![0],
                received: version,
            })),
        }
    }
}

impl StateTransitionBasicStructureValidationV0 for IdentityCreditWithdrawalTransition {
    fn validate_basic_structure(
        &self,
        platform_version: &PlatformVersion,
    ) -> Result<SimpleConsensusValidationResult, Error> {
        match platform_version
            .drive_abci
            .validation_and_processing
            .state_transitions
            .identity_credit_withdrawal_state_transition
            .basic_structure
        {
            Some(0) => {
                // There is nothing expensive here
                self.validate_basic_structure_v0()
            }
            Some(version) => Err(Error::Execution(ExecutionError::UnknownVersionMismatch {
                method: "identity credit withdrawal transition: validate_basic_structure"
                    .to_string(),
                known_versions: vec![0],
                received: version,
            })),
            None => Err(Error::Execution(ExecutionError::VersionNotActive {
                method: "identity credit withdrawal transition: validate_basic_structure"
                    .to_string(),
                known_versions: vec![0],
            })),
        }
    }
}

impl StateTransitionStateValidationV0 for IdentityCreditWithdrawalTransition {
    fn validate_state<C: CoreRPCLike>(
        &self,
        _action: Option<StateTransitionAction>,
        platform: &PlatformRef<C>,
        _validation_mode: ValidationMode,
        _execution_context: &mut StateTransitionExecutionContext,
        tx: TransactionArg,
    ) -> Result<ConsensusValidationResult<StateTransitionAction>, Error> {
        let platform_version = platform.state.current_platform_version()?;

        match platform_version
            .drive_abci
            .validation_and_processing
            .state_transitions
            .identity_credit_withdrawal_state_transition
            .state
        {
            0 => self.validate_state_v0(platform, tx),
            version => Err(Error::Execution(ExecutionError::UnknownVersionMismatch {
                method: "identity credit withdrawal transition: validate_state".to_string(),
                known_versions: vec![0],
                received: version,
            })),
        }
    }
}
