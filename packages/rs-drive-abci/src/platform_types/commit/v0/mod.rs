//! Processing of commits generated by Tenderdash

pub mod accessors;

use crate::abci::AbciError;
use crate::platform_types::cleaned_abci_messages::{cleaned_block_id, cleaned_commit_info};
use dashcore_rpc::dashcore_rpc_json::QuorumType;
use dpp::bls_signatures;
use dpp::validation::{SimpleValidationResult, ValidationResult};
use tenderdash_abci::proto;
use tenderdash_abci::proto::abci::CommitInfo;
use tenderdash_abci::proto::types::BlockId;
use tenderdash_abci::signatures::Signable;

/// Represents block commit
#[derive(Clone, Debug)]
pub struct CommitV0 {
    /// Commit in Tenderdash format
    pub inner: proto::types::Commit,
    /// ID of chain used to sign this commit
    pub chain_id: String,
    /// Type of quorum used to sign this commit
    pub quorum_type: QuorumType,
}

impl CommitV0 {
    /// Create new Commit struct based on commit info and block id received from Tenderdash
    pub(super) fn new_from_cleaned(
        ci: cleaned_commit_info::v0::CleanedCommitInfo,
        block_id: cleaned_block_id::v0::CleanedBlockId,
        height: u64,
        quorum_type: QuorumType,
        chain_id: &str,
    ) -> Self {
        Self {
            chain_id: String::from(chain_id),
            quorum_type,

            inner: proto::types::Commit {
                block_id: Some(block_id.try_into().expect("cannot convert block id")),
                height: height as i64,
                round: ci.round as i32,
                // we need to "un-reverse" quorum hash, as it was reversed in [CleanedCommitInfo::try_from]
                quorum_hash: ci.quorum_hash.to_vec(),
                threshold_block_signature: ci.block_signature.to_vec(),
                threshold_vote_extensions: ci.threshold_vote_extensions.to_vec(),
            },
        }
    }

    /// Create new Commit struct based on commit info and block id received from Tenderdash
    pub(super) fn new(
        ci: CommitInfo,
        block_id: BlockId,
        height: u64,
        quorum_type: QuorumType,
        chain_id: &str,
    ) -> Self {
        Self {
            chain_id: String::from(chain_id),
            quorum_type,

            inner: proto::types::Commit {
                block_id: Some(block_id),
                height: height as i64,
                round: ci.round,
                quorum_hash: ci.quorum_hash,
                threshold_block_signature: ci.block_signature,
                threshold_vote_extensions: ci.threshold_vote_extensions,
            },
        }
    }

    /// Verify all signatures using provided public key.
    ///
    /// ## Return value
    ///
    /// * Ok(true) when all signatures are correct
    /// * Ok(false) when at least one signature is invalid
    /// * Err(e) on error
    pub(super) fn verify_signature(
        &self,
        signature: &[u8; 96],
        public_key: &bls_signatures::PublicKey,
    ) -> SimpleValidationResult<AbciError> {
        if signature == &[0; 96] {
            return ValidationResult::new_with_error(AbciError::BadRequest(
                "commit signature not initialized".to_string(),
            ));
        }
        // We could have received a fake commit, so signature validation needs to be returned if error as a simple validation result
        let signature = match bls_signatures::Signature::from_bytes(signature).map_err(|e| {
            AbciError::BlsErrorOfTenderdashThresholdMechanism(
                e,
                "verification of a commit signature".to_string(),
            )
        }) {
            Ok(signature) => signature,
            Err(e) => return ValidationResult::new_with_error(e),
        };

        //todo: maybe cache this to lower the chance of a hashing based attack (forcing the
        // same calculation each time)
        let quorum_hash = &self.inner.quorum_hash[..]
            .try_into()
            .expect("invalid quorum hash length");

        let hash = match self
            .inner
            .sign_digest(
                &self.chain_id,
                self.quorum_type as u8,
                quorum_hash,
                self.inner.height,
                self.inner.round,
            )
            .map_err(AbciError::Tenderdash)
        {
            Ok(hash) => hash,
            Err(e) => return ValidationResult::new_with_error(e),
        };

        match public_key.verify(&signature, &hash) {
            true => ValidationResult::default(),
            false => ValidationResult::new_with_error(AbciError::BadCommitSignature(format!(
                "commit signature {} is wrong",
                hex::encode(signature.to_bytes().as_slice())
            ))),
        }
    }
}

#[cfg(test)]
mod test {

    use super::CommitV0;
    use crate::platform_types::cleaned_abci_messages::cleaned_commit_info::v0::CleanedCommitInfo;
    use dashcore_rpc::{
        dashcore::hashes::sha256, dashcore::hashes::Hash, dashcore_rpc_json::QuorumType,
    };
    use dpp::bls_signatures::PublicKey;

    use tenderdash_abci::proto::types::{BlockId, PartSetHeader, StateId};
    use tenderdash_abci::signatures::{Hashable, Signable};

    /// Given a commit info and a signature, check that the signature is verified correctly
    #[test]
    fn test_commit_verify() {
        const HEIGHT: i64 = 12345;
        const ROUND: u32 = 2;
        const CHAIN_ID: &str = "test_chain_id";

        const QUORUM_HASH: [u8; 32] = [0u8; 32];

        let ci = CleanedCommitInfo {
            round: ROUND,
            quorum_hash: QUORUM_HASH,
            block_signature: [0u8; 96],
            threshold_vote_extensions: Vec::new(),
        };
        let app_hash = [1u8, 2, 3, 4].repeat(8);

        let state_id = StateId {
            height: HEIGHT as u64,
            app_hash,
            app_version: 1,
            core_chain_locked_height: 3,
            time: 0,
        };

        let block_id = BlockId {
            hash: sha256::Hash::hash("blockID_hash".as_bytes())
                .to_byte_array()
                .to_vec(),
            part_set_header: Some(PartSetHeader {
                total: 1000000,
                hash: sha256::Hash::hash("blockID_part_set_header_hash".as_bytes())
                    .to_byte_array()
                    .to_vec(),
            }),
            state_id: state_id
                .calculate_msg_hash(CHAIN_ID, HEIGHT, ROUND as i32)
                .unwrap(),
        };
        let pubkey = hex::decode("8d63d603fe858be4d7c14a8f308936bd3447c1f361148ad508a04df92f48cd3b2f2b374ef5d1ee8a75f5aeda2f6f3418").unwrap();

        let pubkey = PublicKey::from_bytes(pubkey.as_slice()).unwrap();
        let signature = hex::decode("b95efd51c69a0baf09b130871e735b49cb1b9a0d566bc7ba8fd0fa149dbd28539ab3df435e87ed2a83c94ea714bc8e120504b1cba9363b32c3d58499ed85ecf14539e8e99329fa7952420e4ad9da80b3b28388d62be00770988e4aee705da830").unwrap();

        let commit = CommitV0::new_from_cleaned(
            ci,
            block_id.try_into().unwrap(),
            HEIGHT as u64,
            QuorumType::LlmqTest,
            CHAIN_ID,
        );
        let expect_msg = hex::decode(
            "020000003930000000000000020000000000000035117edfe49351da1e81d1b0f2edfa0b984a7508\
            958870337126efb352f1210715e56fe9d267359b4b437a52636174ac64aa9a021671aabc9985023695bc6e3\
            6746573745f636861696e5f6964",
        )
        .unwrap();
        let expect_msg_hash = sha256::Hash::hash(&expect_msg).to_byte_array().to_vec();
        let expect_sign_hash =
            hex::decode("58fb34b03f9028e6ac181418c753f33e471ae223bb66b2bef7b46732a15b7eac")
                .unwrap();
        assert_eq!(
            expect_msg_hash,
            commit
                .inner
                .calculate_msg_hash(CHAIN_ID, HEIGHT, ROUND as i32)
                .unwrap()
        );
        assert_eq!(
            expect_sign_hash,
            commit
                .inner
                .calculate_sign_hash(
                    CHAIN_ID,
                    QuorumType::LlmqTest as u8,
                    &QUORUM_HASH,
                    HEIGHT,
                    ROUND as i32
                )
                .unwrap()
        );
        assert!(commit
            .verify_signature(
                &signature.clone().try_into().expect("expected 96 bytes"),
                &pubkey
            )
            .is_valid());

        // mutate data and ensure it is invalid
        let mut commit = commit;
        commit.chain_id = "invalid".to_string();
        assert!(!commit
            .verify_signature(&signature.try_into().expect("expected 96 bytes"), &pubkey)
            .is_valid());
    }
}
