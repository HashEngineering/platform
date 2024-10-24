// MIT LICENSE
//
// Copyright (c) 2021 Dash Core Group
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.
//

//! Epoch Start Blocks
//!
//! This modules implements functions in Drive relevant to Epoch start blocks.
//!

mod get_epoch_start_block_core_height;
mod get_epoch_start_block_height;
mod get_first_epoch_start_block_info_between_epochs;

use dpp::block::epoch::EpochIndex;

/// `StartBlockInfo` contains information about the starting block of an epoch.
#[derive(Debug, PartialEq, Eq)]
pub struct StartBlockInfo {
    /// The index of the epoch.
    pub epoch_index: EpochIndex,

    /// The height of the starting block within the epoch.
    pub start_block_height: u64,

    /// The core height of the starting block.
    pub start_block_core_height: u32,
}

#[cfg(feature = "full")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::helpers::setup::setup_drive_with_initial_state_structure;

    mod get_epoch_start_block_height {
        use super::*;
        use crate::error::drive::DriveError;
        use crate::error::Error;
        use crate::fee_pools::epochs::epoch_key_constants::{
            KEY_START_BLOCK_CORE_HEIGHT, KEY_START_BLOCK_HEIGHT,
        };
        use crate::fee_pools::epochs::paths::EpochProposers;
        use dpp::block::epoch::Epoch;
        use dpp::version::PlatformVersion;
        use grovedb::Element;

        #[test]
        fn test_error_if_epoch_tree_is_not_initiated() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let non_initiated_epoch = Epoch::new(7000).unwrap();

            let result = drive.get_epoch_start_block_height(
                &non_initiated_epoch,
                Some(&transaction),
                platform_version,
            );

            assert!(matches!(
                result,
                Err(Error::GroveDB(grovedb::Error::PathParentLayerNotFound(_)))
            ));
        }

        #[test]
        fn test_error_if_value_is_not_set() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let epoch = Epoch::new(0).unwrap();

            let result =
                drive.get_epoch_start_block_height(&epoch, Some(&transaction), platform_version);

            assert!(matches!(result, Err(Error::GroveDB(_))));
        }

        #[test]
        fn test_error_if_value_has_invalid_length() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let epoch = Epoch::new(0).unwrap();

            drive
                .grove
                .insert(
                    &epoch.get_path(),
                    KEY_START_BLOCK_HEIGHT.as_slice(),
                    Element::Item(u128::MAX.to_be_bytes().to_vec(), None),
                    None,
                    Some(&transaction),
                )
                .unwrap()
                .expect("should insert invalid data");

            let result =
                drive.get_epoch_start_block_height(&epoch, Some(&transaction), platform_version);

            assert!(matches!(
                result,
                Err(Error::Drive(DriveError::CorruptedSerialization(_)))
            ));
        }

        #[test]
        fn test_error_if_value_has_invalid_length_core_height() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let epoch = Epoch::new(0).unwrap();

            drive
                .grove
                .insert(
                    &epoch.get_path(),
                    KEY_START_BLOCK_CORE_HEIGHT.as_slice(),
                    Element::Item(u64::MAX.to_be_bytes().to_vec(), None),
                    None,
                    Some(&transaction),
                )
                .unwrap()
                .expect("should insert invalid data");

            let result = drive.get_epoch_start_block_core_height(
                &epoch,
                Some(&transaction),
                platform_version,
            );

            assert!(matches!(
                result,
                Err(Error::Drive(DriveError::CorruptedSerialization(_)))
            ));
        }

        #[test]
        fn test_error_if_element_has_invalid_type() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let epoch = Epoch::new(0).unwrap();

            drive
                .grove
                .insert(
                    &epoch.get_path(),
                    KEY_START_BLOCK_HEIGHT.as_slice(),
                    Element::empty_tree(),
                    None,
                    Some(&transaction),
                )
                .unwrap()
                .expect("should insert invalid data");

            let result =
                drive.get_epoch_start_block_height(&epoch, Some(&transaction), platform_version);

            assert!(matches!(
                result,
                Err(Error::Drive(DriveError::UnexpectedElementType(_)))
            ));
        }

        #[test]
        fn test_error_if_element_has_invalid_type_core_height() {
            let drive = setup_drive_with_initial_state_structure();
            let platform_version = PlatformVersion::latest();

            let transaction = drive.grove.start_transaction();

            let epoch = Epoch::new(0).unwrap();

            drive
                .grove
                .insert(
                    &epoch.get_path(),
                    KEY_START_BLOCK_CORE_HEIGHT.as_slice(),
                    Element::empty_tree(),
                    None,
                    Some(&transaction),
                )
                .unwrap()
                .expect("should insert invalid data");

            let result = drive.get_epoch_start_block_core_height(
                &epoch,
                Some(&transaction),
                platform_version,
            );

            assert!(matches!(
                result,
                Err(Error::Drive(DriveError::UnexpectedElementType(_)))
            ));
        }
    }

    mod get_first_epoch_start_block_height_between_epochs {
        use super::*;
        use crate::drive::batch::grovedb_op_batch::GroveDbOpBatchV0Methods;
        use crate::drive::batch::GroveDbOpBatch;
        use crate::fee_pools::epochs::operations_factory::EpochOperations;
        use dpp::block::epoch::Epoch;
        use dpp::version::PlatformVersion;

        #[test]
        fn test_next_block_height() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let epoch_tree_0 = Epoch::new(0).unwrap();
            let epoch_tree_1 = Epoch::new(1).unwrap();

            let mut batch = GroveDbOpBatch::new();

            batch.push(epoch_tree_0.update_start_block_height_operation(1));
            batch.push(epoch_tree_0.update_start_block_core_height_operation(1));
            batch.push(epoch_tree_1.update_start_block_height_operation(2));
            batch.push(epoch_tree_1.update_start_block_core_height_operation(2));

            drive
                .grove_apply_batch(batch, false, Some(&transaction), &platform_version.drive)
                .expect("should apply batch");

            let next_epoch_start_block_height_option = drive
                .get_first_epoch_start_block_info_between_epochs(
                    0,
                    2,
                    Some(&transaction),
                    platform_version,
                )
                .expect("should find next start_block_height");

            assert_eq!(
                next_epoch_start_block_height_option,
                Some(StartBlockInfo {
                    epoch_index: 1,
                    start_block_height: 2,
                    start_block_core_height: 2,
                })
            );
        }

        #[test]
        fn test_none_if_there_are_no_start_block_heights() {
            let drive = setup_drive_with_initial_state_structure();
            let transaction = drive.grove.start_transaction();

            let platform_version = PlatformVersion::latest();

            let next_epoch_start_block_height = drive
                .get_first_epoch_start_block_info_between_epochs(
                    0,
                    4,
                    Some(&transaction),
                    platform_version,
                )
                .expect("should find next start_block_height");

            assert!(next_epoch_start_block_height.is_none());
        }

        #[test]
        fn test_none_if_start_block_height_is_outside_of_specified_epoch_range() {
            let drive = setup_drive_with_initial_state_structure();

            let platform_version = PlatformVersion::latest();

            let transaction = drive.grove.start_transaction();

            let epoch_tree_0 = Epoch::new(0).unwrap();
            let epoch_tree_3 = Epoch::new(3).unwrap();

            let mut batch = GroveDbOpBatch::new();

            batch.push(epoch_tree_0.update_start_block_height_operation(1));
            batch.push(epoch_tree_3.update_start_block_height_operation(3));

            drive
                .grove_apply_batch(batch, false, Some(&transaction), &platform_version.drive)
                .expect("should apply batch");

            let next_epoch_start_block_height = drive
                .get_first_epoch_start_block_info_between_epochs(
                    0,
                    2,
                    Some(&transaction),
                    platform_version,
                )
                .expect("should find next start_block_height");

            assert!(next_epoch_start_block_height.is_none());
        }

        #[test]
        fn test_start_block_height_in_two_epoch_in_case_of_gaps() {
            let drive = setup_drive_with_initial_state_structure();

            let platform_version = PlatformVersion::latest();

            let transaction = drive.grove.start_transaction();

            let epoch_tree_0 = Epoch::new(0).unwrap();
            let epoch_tree_3 = Epoch::new(3).unwrap();

            let mut batch = GroveDbOpBatch::new();

            batch.push(epoch_tree_0.update_start_block_height_operation(1));
            batch.push(epoch_tree_0.update_start_block_core_height_operation(1));
            batch.push(epoch_tree_3.update_start_block_height_operation(2));
            batch.push(epoch_tree_3.update_start_block_core_height_operation(5));

            drive
                .grove_apply_batch(batch, false, Some(&transaction), &platform_version.drive)
                .expect("should apply batch");

            let next_epoch_start_block_height = drive
                .get_first_epoch_start_block_info_between_epochs(
                    0,
                    4,
                    Some(&transaction),
                    platform_version,
                )
                .expect("should find next start_block_height doesn't error")
                .expect("should find next start_block_height");

            assert_eq!(
                next_epoch_start_block_height,
                StartBlockInfo {
                    epoch_index: 3,
                    start_block_height: 2,
                    start_block_core_height: 5,
                }
            );
        }
    }
}
