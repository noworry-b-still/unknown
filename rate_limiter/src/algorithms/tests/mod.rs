// src/algorithms/tests/mod.rs

/// Tests for Token Bucket algorithm
mod token_bucket_tests;

// Tests for Fixed Window algorithm
// mod fixed_window_tests;

// Tests for Sliding Window algorithm
// mod sliding_window_tests;

// /// Common tests for all algorithms
// #[cfg(test)]
// mod common_tests {
//     use std::time::Duration;

//     use crate::{
//         algorithms::{FixedWindow, RateLimitAlgorithm, SlidingWindow, TokenBucket},
//         config::{FixedWindowConfig, SlidingWindowConfig, TokenBucketConfig},
//         test_utils::create_test_rate_limiter,
//     };

//     /// Test consistent behavior across all algorithm implementations
//     #[tokio::test]
//     async fn test_algorithm_trait_consistency() {
//         // Create configurations with equivalent parameters
//         let tb_config = TokenBucketConfig {
//             capacity: 5,
//             refill_rate: 0.0, // No refill to make it comparable to others
//             initial_tokens: Some(5),
//         };

//         let fw_config = FixedWindowConfig {
//             max_requests: 5,
//             window: Duration::from_secs(60), // Long window to avoid timing issues
//         };

//         let sw_config = SlidingWindowConfig {
//             max_requests: 5,
//             window: Duration::from_secs(60),
//             precision: 1, // Single bucket for fixed window comparison
//         };

//         // Create algorithms
//         let tb_algorithm = TokenBucket::new(tb_config);
//         let fw_algorithm = FixedWindow::new(fw_config);
//         let sw_algorithm = SlidingWindow::new(sw_config);

//         // Create rate limiters
//         let tb_limiter = create_test_rate_limiter(tb_algorithm).await;
//         let fw_limiter = create_test_rate_limiter(fw_algorithm).await;
//         let sw_limiter = create_test_rate_limiter(sw_algorithm).await;

//         // Test basic behaviors across all algorithms
//         for (name, limiter) in [
//             ("token_bucket", &tb_limiter),
//             ("fixed_window", &fw_limiter),
//             ("sliding_window", &sw_limiter),
//         ] {
//             // All should allow exactly 5 requests
//             for i in 0..5 {
//                 let result = limiter.check_and_record(name).await.unwrap();
//                 assert!(result.allowed, "{}: Request {} should be allowed", name, i);
//             }

//             // All should deny further requests
//             let result = limiter.check_and_record(name).await.unwrap();
//             assert!(!result.allowed, "{}: 6th request should be denied", name);

//             // All should respect key isolation
//             let other_result = limiter
//                 .check_and_record(&format!("{}_other", name))
//                 .await
//                 .unwrap();
//             assert!(
//                 other_result.allowed,
//                 "{}: Different key should be allowed",
//                 name
//             );

//             // All should support reset
//             limiter.reset(name).await.unwrap();
//             let reset_result = limiter.check_and_record(name).await.unwrap();
//             assert!(
//                 reset_result.allowed,
//                 "{}: Request after reset should be allowed",
//                 name
//             );
//         }
//     }

//     /// Test correct function of is_allowed() without recording
//     #[tokio::test]
//     async fn test_is_allowed_without_recording() {
//         // Create token bucket for this test
//         let tb_config = TokenBucketConfig {
//             capacity: 3,
//             refill_rate: 0.0, // No refill
//             initial_tokens: Some(3),
//         };

//         let algorithm = TokenBucket::new(tb_config);
//         let limiter = create_test_rate_limiter(algorithm).await;
//         let key = "test_is_allowed";

//         // Check allowed without recording
//         for _ in 0..10 {
//             let allowed = limiter.is_allowed(key).await.unwrap();
//             assert!(
//                 allowed,
//                 "is_allowed() should return true without consuming tokens"
//             );
//         }

//         // Now record requests to consume tokens
//         for i in 0..3 {
//             let result = limiter.check_and_record(key).await.unwrap();
//             assert!(result.allowed, "Request {} should be allowed", i);
//         }

//         // Further requests should be denied
//         let result = limiter.check_and_record(key).await.unwrap();
//         assert!(
//             !result.allowed,
//             "Request after limit exhausted should be denied"
//         );

//         // But is_allowed should still return false now
//         let allowed = limiter.is_allowed(key).await.unwrap();
//         assert!(
//             !allowed,
//             "is_allowed() should return false when limit exhausted"
//         );
//     }
// }
