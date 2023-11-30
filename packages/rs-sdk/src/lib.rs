//! # Dash Platform Rust SDK
//!
//! This is the official Rust SDK for the Dash Platform. Dash Platform is a Layer 2 cryptocurrency technology that
//! builds upon the Dash layer 1 network. This SDK provides an abstraction layer to simplify usage of the Dash
//! Platform along with data models based on the Dash Platform Protocol (DPP), a CRUD interface, and bindings
//! for other technologies such as C.
//!
//!
//! ## Dash Platform Protocol Data Model
//!
//! SDK data model uses types defined in [Dash Platform Protocol (DPP)](crate::platform::dpp). At this point, the following
//! types are supported:
//!
//! 1. [`Identity`](crate::platform::Identity)
//! 2. [`Data Contract`](crate::platform::DataContract)
//! 3. [`Document`](crate::platform::Document)
//!
//! To define document search conditions, you can use [`DriveQuery`](crate::platform::DriveQuery) and convert it
//! to [`DocumentQuery`](crate::platform::DocumentQuery) with the [`From`] trait.
//!
//! Basic DPP objects are re-exported in the [`platform`] module.
//!
//! ## CRUD Interface
//!
//! Operations on data model objects can be executing using traits following CRUD (Create, Read, Update, and Delete)
//! approach. The following traits are already implemented:
//!
//! 1. [`Fetch`](crate::platform::Fetch)
//! 2. [`FetchMany`](crate::platform::FetchMany)
//!
//! Fetch and FetchMany traits return objects based on provided queries. Some example queries include:
//!
//! 1. [`Identifier`](crate::platform::Identifier) - fetches an object by its identifier
//! 2. [`DocumentQuery`](crate::platform::DocumentQuery) - fetches documents based on search conditions; see
//! [query syntax documentation](https://docs.dash.org/projects/platform/en/stable/docs/reference/query-syntax.html)
//! for more details.
//! 3. [`DriveQuery`](crate::platform::DriveQuery) - can be used to build more complex queries
//!
//! ## Testability
//!
//! SDK operations can be mocked using [Sdk::new_mock()].
//!
//! Examples can be found in `tests/mock_*.rs`.
//!
//! ## Error handling
//!
//! Errors of type [Error] are returned by the rs-sdk. Note that missing objects ("not found") are not
//! treated as errors; `Ok(None)` is returned instead.
//!
//! Mocking functions often panic instead of returning an error.
//!
//! ## Logging
//!
//! This project uses the `tracing` crate for instrumentation and logging. The `tracing` ecosystem provides a powerful,
//! flexible framework for adding structured, context-aware logs to your program.
//!
//! To enable logging, you can use the `tracing_subscriber` crate which allows applications to customize how events are processed and recorded.
//! An example can be found in `tests/common.rs:setup_logs()`.
//!
#![warn(missing_docs)]
#![allow(rustdoc::private_intra_doc_links)]

pub mod core;
pub mod error;
pub mod mock;
pub mod platform;
pub mod sdk;

use std::path::PathBuf;
use std::str::FromStr;
pub use error::Error;
pub use sdk::{Sdk, SdkBuilder};
use crate::core::CoreClient;
use crate::platform::{Fetch, Identifier};
use dpp::prelude::Identity;
use rs_dapi_client::AddressList;
use serde::Deserialize;
use tokio::runtime;

/// Version of the SDK
pub const VERSION: &str = env!("CARGO_PKG_VERSION");


pub const PLATFORM_IP: &str = "10.56.229.104";
pub const CORE_PORT: u16 = 30002;
pub const CORE_USER: &str = "PdXjj4HC";
pub const CORE_PASSWORD: &str = "POv4lqSbzO7m";
pub const PLATFORM_PORT: u16 = 2443;

#[derive(Debug, Deserialize)]
/// Configuration for rs-sdk.
///
/// Content of this configuration is loaded from environment variables or `${CARGO_MANIFEST_DIR}/.env` file
/// when the [Config::new()] is called.
/// Variable names in the enviroment and `.env` file must be prefixed with [RS_SDK_](Config::CONFIG_PREFIX)
/// and written as SCREAMING_SNAKE_CASE (e.g. `RS_SDK_PLATFORM_HOST`).
pub struct Config {
    /// Hostname of the Dash Platform node to connect to
    pub platform_host: String,
    /// Port of the Dash Platform node grpc interface
    pub platform_port: u16,
    /// Port of the Dash Core RPC interface running on the Dash Platform node
    pub core_port: u16,
    /// Username for Dash Core RPC interface
    pub core_user: String,
    /// Password for Dash Core RPC interface
    pub core_password: String,

    /// Directory where all generated test vectors will be saved.
    ///
    /// See [SdkBuilder::with_dump_dir()](crate::SdkBuilder::with_dump_dir()) for more details.
    pub dump_dir: PathBuf,

    // IDs of some objects generated by the testnet
    /// ID of existing identity.
    ///
    /// Format: Base58
    pub existing_identity_id: Identifier,
    /// ID of existing data contract.
    ///
    /// Format: Base58
    pub existing_data_contract_id: Identifier,
    /// Name of document type defined for [`existing_data_contract_id`](Config::existing_data_contract_id).
    pub existing_document_type_name: String,
    /// ID of document of the type [`existing_document_type_name`](Config::existing_document_type_name)
    /// in [`existing_data_contract_id`](Config::existing_data_contract_id).
    pub existing_document_id: Identifier,
}

impl Config {
    /// Prefix of configuration options in the environment variables and `.env` file.
    pub const CONFIG_PREFIX: &str = "RS_SDK_";
    /// Load configuration from operating system environment variables and `.env` file.
    ///
    /// Create new [Config] with data from environment variables and `${CARGO_MANIFEST_DIR}/tests/.env` file.
    /// Variable names in the environment and `.env` file must be converted to SCREAMING_SNAKE_CASE and
    /// prefixed with [RS_SDK_](Config::CONFIG_PREFIX).
    pub fn new() -> Self {
        // load config from .env file, ignore errors

        let path: String = env!("CARGO_MANIFEST_DIR").to_owned() + "/tests/.env";
        if let Err(err) = dotenvy::from_path(&path) {
            tracing::warn!(path, ?err, "failed to load config file");
        }

        let config: Self = envy::prefixed(Self::CONFIG_PREFIX)
            .from_env()
            .expect("configuration error");

        if config.is_empty() {
            tracing::warn!(path, ?config, "some config fields are empty");
            #[cfg(not(feature = "offline-testing"))]
            panic!("invalid configuration")
        }

        config
    }

    /// Check if credentials of the config are empty.
    ///
    /// Checks if fields [platform_host](Config::platform_host), [platform_port](Config::platform_port),
    /// [core_port](Config::core_port), [core_user](Config::core_user) and [core_password](Config::core_password)
    /// are not empty.
    ///
    /// Other fields are ignored.
    pub fn is_empty(&self) -> bool {
        self.core_user.is_empty()
            || self.core_password.is_empty()
            || self.platform_host.is_empty()
            || self.platform_port == 0
            || self.core_port == 0
    }

    #[allow(unused)]
    /// Create list of Platform addresses from the configuration
    pub fn address_list(&self) -> AddressList {
        let address: String = format!("http://{}:{}", self.platform_host, self.platform_port);

        AddressList::from_iter(vec![http::Uri::from_str(&address).expect("valid uri")])
    }

    /// Create new SDK instance
    ///
    /// Depending on the feature flags, it will connect to the configured platform node or mock API.
    ///
    /// ## Feature flags
    ///
    /// * `offline-testing` is not set - connect to the platform and generate
    /// new test vectors during execution
    /// * `offline-testing` is set - use mock implementation and
    /// load existing test vectors from disk
    pub async fn setup_api(&self) -> Sdk {
        // offline testing takes precedence over network testing
        #[cfg(all(feature = "network-testing", not(feature = "offline-testing")))]
            let sdk = {
            // Dump all traffic to disk
            let builder = rs_sdk::SdkBuilder::new(self.address_list()).with_core(
                &self.platform_host,
                self.core_port,
                &self.core_user,
                &self.core_password,
            );

            #[cfg(feature = "generate-test-vectors")]
                let builder = builder.with_dump_dir(&self.dump_dir);

            builder.build().expect("cannot initialize api")
        };

        // offline testing takes precedence over network testing
        #[cfg(feature = "offline-testing")]
            let sdk = {
            let mut mock_sdk = SdkBuilder::new_mock()
                .build()
                .expect("initialize api");

            mock_sdk
                .mock()
                .quorum_info_dir(&self.dump_dir)
                .load_expectations(&self.dump_dir)
                .await
                .expect("load expectations");

            mock_sdk
        };

        sdk
    }

    // fn default_identity_id() -> Identifier {
    //     SystemDataContract::DPNS
    //         .source()
    //         .expect("data contract source")
    //         .owner_id_bytes
    //         .into()
    // }

    // fn default_data_contract_id() -> Identifier {
    //     data_contracts::SystemDataContract::DPNS.id()
    // }
    //
    // fn default_document_type_name() -> String {
    //     "domain".to_string()
    // }
    // fn default_document_id() -> Identifier {
    //     DPNS_DASH_TLD_DOCUMENT_ID.into()
    // }
    //
    // fn default_dump_dir() -> PathBuf {
    //     PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    //         .join("tests")
    //         .join("vectors")
    // }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

#[ferment_macro::export]
pub fn get_identity() -> Identity {
    pub const IDENTITY_ID_BYTES: [u8; 32] = [
        65, 63, 57, 243, 204, 9, 106, 71, 187, 2, 94, 221, 190, 127, 141, 114, 137, 209, 243, 50,
        60, 215, 90, 101, 229, 15, 115, 5, 44, 117, 182, 217,
    ];
    let id = Identifier::from_bytes(&IDENTITY_ID_BYTES).expect("parse identity id");
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut api = runtime
        .block_on(Config::new().setup_api());
        //.unwrap()
        //.expect("api should exist");

    runtime
        .block_on(dpp::prelude::Identity::fetch(&mut api, id))
        .unwrap()
        .expect("identity should exist")
}
