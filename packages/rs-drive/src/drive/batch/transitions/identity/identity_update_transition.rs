use crate::drive::batch::transitions::DriveHighLevelOperationConverter;
use crate::drive::batch::DriveOperation::IdentityOperation;
use crate::drive::batch::{DriveOperation, IdentityOperationType};

use crate::error::Error;
use dpp::block::epoch::Epoch;

use dpp::identity::state_transition::identity_update_transition::IdentityUpdateTransitionAction;

impl DriveHighLevelOperationConverter for IdentityUpdateTransitionAction {
    fn into_high_level_drive_operations<'a>(
        self,
        _epoch: &Epoch,
    ) -> Result<Vec<DriveOperation<'a>>, Error> {
        let IdentityUpdateTransitionAction {
            add_public_keys,
            disable_public_keys,
            public_keys_disabled_at,
            identity_id,
            revision,
            ..
        } = self;

        let mut drive_operations = vec![];

        drive_operations.push(IdentityOperation(
            IdentityOperationType::UpdateIdentityRevision {
                identity_id: identity_id.to_buffer(),
                revision,
            },
        ));

        if !add_public_keys.is_empty() {
            drive_operations.push(IdentityOperation(
                IdentityOperationType::AddNewKeysToIdentity {
                    identity_id: identity_id.to_buffer(),
                    keys_to_add: add_public_keys,
                },
            ));
        }
        if let Some(public_keys_disabled_at) = public_keys_disabled_at {
            if !disable_public_keys.is_empty() {
                drive_operations.push(IdentityOperation(
                    IdentityOperationType::DisableIdentityKeys {
                        identity_id: identity_id.to_buffer(),
                        keys_ids: disable_public_keys,
                        disable_at: public_keys_disabled_at,
                    },
                ));
            }
        }

        Ok(drive_operations)
    }
}