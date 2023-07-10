use crate::error::Error;
use crate::execution::types::execution_event::ExecutionEvent;
use crate::execution::types::execution_result::ExecutionResult;
use crate::execution::types::execution_result::ExecutionResult::{
    ConsensusExecutionError, SuccessfulFreeExecution, SuccessfulPaidExecution,
};
use crate::platform_types::platform::Platform;
use crate::rpc::core::CoreRPCLike;
use dpp::block::block_info::BlockInfo;
use dpp::validation::SimpleConsensusValidationResult;
use drive::grovedb::Transaction;

impl<C> Platform<C>
where
    C: CoreRPCLike,
{
    /// Executes the given `event` based on the `block_info` and `transaction`.
    ///
    /// This function takes an `ExecutionEvent`, `BlockInfo`, and `Transaction` as input and performs
    /// the corresponding operations on the drive. It will validate the fees of the event and apply
    /// drive operations accordingly.
    ///
    /// # Arguments
    ///
    /// * `event` - The execution event to be processed.
    /// * `block_info` - Information about the current block being processed.
    /// * `transaction` - The transaction associated with the execution event.
    ///
    /// # Returns
    ///
    /// * `Result<ExecutionResult, Error>` - If the execution is successful, it returns an `ExecutionResult`
    ///   which can be one of the following variants: `SuccessfulPaidExecution`, `SuccessfulFreeExecution`, or
    ///   `ConsensusExecutionError`. If the execution fails, it returns an `Error`.
    ///
    /// # Errors
    ///
    /// This function may return an `Error` variant if there is a problem with the drive operations or
    /// an internal error occurs.
    pub(in crate::execution) fn execute_event_v0(
        &self,
        event: ExecutionEvent,
        block_info: &BlockInfo,
        transaction: &Transaction,
    ) -> Result<ExecutionResult, Error> {
        //todo: we need to split out errors
        //  between failed execution and internal errors
        let validation_result =
            self.validate_fees_of_event_v0(&event, block_info, Some(transaction))?;
        match event {
            ExecutionEvent::PaidFromAssetLockDriveEvent {
                identity,
                operations,
                ..
            }
            | ExecutionEvent::PaidDriveEvent {
                identity,
                operations,
            } => {
                if validation_result.is_valid_with_data() {
                    //todo: make this into an atomic event with partial batches
                    let individual_fee_result = self
                        .drive
                        .apply_drive_operations(operations, true, block_info, Some(transaction))
                        .map_err(Error::Drive)?;

                    let balance_change =
                        individual_fee_result.into_balance_change(identity.id.to_buffer());

                    let outcome = self.drive.apply_balance_change_from_fee_to_identity(
                        balance_change,
                        Some(transaction),
                    )?;

                    Ok(SuccessfulPaidExecution(
                        validation_result.into_data()?,
                        outcome.actual_fee_paid,
                    ))
                } else {
                    Ok(ConsensusExecutionError(
                        SimpleConsensusValidationResult::new_with_errors(validation_result.errors),
                    ))
                }
            }
            ExecutionEvent::FreeDriveEvent { operations } => {
                self.drive
                    .apply_drive_operations(operations, true, block_info, Some(transaction))
                    .map_err(Error::Drive)?;
                Ok(SuccessfulFreeExecution)
            }
        }
    }
}