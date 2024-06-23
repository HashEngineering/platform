//! Test VotePollsByEndDateDriveQuery

use crate::fetch::{common::setup_logs, config::Config};
use dash_sdk::platform::FetchMany;
use dpp::{identity::TimestampMillis, voting::vote_polls::VotePoll};
use drive::query::VotePollsByEndDateDriveQuery;
use std::collections::BTreeMap;

/// Test that we can fetch vote polls
///
/// ## Preconditions
///
/// 1. At least one vote poll exists
#[cfg_attr(
    feature = "network-testing",
    ignore = "requires manual DPNS names setup for masternode voting tests; see fn check_mn_voting_prerequisities()"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn vote_polls_by_ts_ok() {
    setup_logs();

    let cfg = Config::new();

    let sdk = cfg.setup_api("vote_polls_by_ts_ok").await;
    super::contested_resource::check_mn_voting_prerequisities(&cfg)
        .await
        .expect("prerequisities");

    let query = VotePollsByEndDateDriveQuery {
        limit: None,
        offset: None,
        order_ascending: true,
        start_time: None,
        end_time: None,
    };

    let rss = VotePoll::fetch_many(&sdk, query)
        .await
        .expect("fetch contested resources");
    tracing::info!("vote polls retrieved: {:?}", rss);
    assert!(!rss.0.is_empty());
}

/// Test that we can fetch vote polls ordered by timestamp, ascending and descending
///
/// ## Preconditions
///
/// 1. At least 2 vote polls exist
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg_attr(
    feature = "network-testing",
    ignore = "requires manual DPNS names setup for masternode voting tests; see fn check_mn_voting_prerequisities()"
)]
// fails due to PLAN-661
async fn vote_polls_by_ts_order() {
    setup_logs();

    let cfg = Config::new();
    let sdk = cfg.setup_api("vote_polls_by_ts_order").await;
    super::contested_resource::check_mn_voting_prerequisities(&cfg)
        .await
        .expect("prerequisities");

    let base_query = VotePollsByEndDateDriveQuery {
        limit: None,
        offset: None,
        order_ascending: true,
        start_time: None,
        end_time: None,
    };

    for order_ascending in [true, false] {
        let query = VotePollsByEndDateDriveQuery {
            order_ascending,
            ..base_query.clone()
        };

        let rss = VotePoll::fetch_many(&sdk, query)
            .await
            .expect("fetch contested resources");
        tracing::debug!(order_ascending, ?rss, "vote polls retrieved");
        assert!(!rss.0.is_empty());
        let enumerated = rss.0.iter().enumerate().collect::<BTreeMap<_, _>>();
        for (i, (ts, _)) in &enumerated {
            if *i > 0 {
                let (prev_ts, _) = &enumerated[&(i - 1)];
                if order_ascending {
                    assert!(
                        ts >= prev_ts,
                        "ascending order: item {} ({}) must be >= than item {} ({})",
                        ts,
                        i,
                        prev_ts,
                        i - 1
                    );
                } else {
                    assert!(
                        ts <= prev_ts,
                        "descending order: item {} ({}) must be >= than item {} ({})",
                        ts,
                        i,
                        prev_ts,
                        i - 1
                    );
                }
            }
        }
    }
}

/// Test that we can fetch vote polls with a limit
///
/// ## Preconditions
///
/// 1. At least 3 vote poll exists
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[cfg_attr(
    feature = "network-testing",
    ignore = "requires manual DPNS names setup for masternode voting tests; see fn check_mn_voting_prerequisities()"
)]

// fails due to PLAN-659
async fn vote_polls_by_ts_limit() {
    setup_logs();

    let cfg = Config::new();
    let sdk = cfg.setup_api("vote_polls_by_ts_limit").await;
    super::contested_resource::check_mn_voting_prerequisities(&cfg)
        .await
        .expect("prerequisities");

    // Given index with more than 2 contested resources
    const LIMIT: usize = 2;
    const LIMIT_ALL: usize = 100;

    let test_start_time: TimestampMillis = chrono::Utc::now().timestamp_millis() as u64;

    let query_all = VotePollsByEndDateDriveQuery {
        limit: Some(LIMIT_ALL as u16),
        offset: None,
        order_ascending: true,
        start_time: None,
        end_time: Some((test_start_time, true)),
    };

    let all = VotePoll::fetch_many(&sdk, query_all.clone())
        .await
        .expect("fetch vote polls");
    // this counts timestamps, not vote polls themselves
    let count_all_timestamps = all.0.len();
    assert_ne!(count_all_timestamps, 0, "at least one vote poll expected");

    let all_values = all.0.into_iter().collect::<Vec<_>>();

    tracing::debug!(count_all_timestamps, "Count all");
    // When we query for 2 contested values at a time, we get all of them
    let mut checked_count: usize = 0;
    let mut start_time = None;

    for inclusive in [true, false] {
        while checked_count < LIMIT_ALL {
            let query = VotePollsByEndDateDriveQuery {
                limit: Some(LIMIT as u16),
                start_time,
                ..query_all.clone()
            };

            let rss = VotePoll::fetch_many(&sdk, query)
                .await
                .expect("fetch vote polls");

            let Some(last) = rss.0.keys().last().copied() else {
                // no more vote polls
                break;
            };

            tracing::debug!(polls=?rss, inclusive, ?start_time, checked_count, "Vote pools");
            let length = rss.0.len();

            for (j, current) in rss.0.iter().enumerate() {
                let all_idx = if inclusive && (j + checked_count > 0) {
                    j + checked_count - 1
                } else {
                    j + checked_count
                };
                let expected = &all_values[all_idx];
                assert_eq!(*current.0, expected.0, "timestamp should match");
                assert_eq!(current.1, &expected.1, "vote polls should match");
            }

            let expected = if checked_count + LIMIT > count_all_timestamps {
                count_all_timestamps - checked_count
            } else {
                LIMIT
            };
            assert_eq!(length, expected as usize);
            tracing::debug!(polls=?rss, checked_count, "Vote polls");

            start_time = Some((last, inclusive));
            checked_count += if inclusive { length - 1 } else { length };
        }
    }
    assert_eq!(
        checked_count,
        count_all_timestamps * 2,
        "all vote polls should be checked twice (inclusive and exclusive)"
    );
}
