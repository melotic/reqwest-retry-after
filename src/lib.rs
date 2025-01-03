//! # reqwest-retry-after
//!
//! `reqwest-retry-after` is a library that adds support for the `Retry-After` header
//! in [`reqwest`], using [`reqwest_middleware`].
//!
//! ## Usage
//!
//! Simply pass [`RetryAfterMiddleware`] to the [`ClientWithMiddleware`] builder.
//!
//! ```
//! use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
//! use reqwest_retry_after::RetryAfterMiddleware;
//!
//! let client = ClientBuilder::new(reqwest::Client::new())
//!     .with(RetryAfterMiddleware::new())
//!     .build();
//! ```
//!
//! ## Notes
//!
//! A client constructed with [`RetryAfterMiddleware`] will apply the `Retry-After` header
//! to all future requests, regardless of domain or URL. This means that if you query one URL
//! which sets a `Retry-After`, and then query a different URL that has no ratelimiting,
//! the `Retry-After` will be applied to the new URL.
//!
//! If you need this functionality, consider creating a seperate client for each endpoint.
#![warn(missing_docs)]
#![warn(rustdoc::missing_doc_code_examples)]

use std::time::{Duration, SystemTime};

use http::{header::RETRY_AFTER, Extensions};
use reqwest_middleware::{
    reqwest::{Request, Response},
    Middleware, Next, Result,
};
use tokio::sync::RwLock;

/// The `RetryAfterMiddleware` is a [`Middleware`] that adds support for the `Retry-After`
/// header in [`reqwest`].
pub struct RetryAfterMiddleware {
    retry_after: RwLock<Option<SystemTime>>,
}

impl RetryAfterMiddleware {
    /// Creates a new `RetryAfterMiddleware`.
    pub fn new() -> Self {
        Self {
            retry_after: RwLock::new(None),
        }
    }
}

impl Default for RetryAfterMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Middleware for RetryAfterMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let lock = *self.retry_after.read().await;

        if let Some(it) = lock {
            let now = SystemTime::now();

            if let Ok(duration) = it.duration_since(now) {
                tokio::time::sleep(duration).await
            }
        }

        let res = next.run(req, extensions).await;

        if let Ok(res) = &res {
            match res.headers().get(RETRY_AFTER) {
                Some(retry_after) => {
                    // parse secs from header, return res if invalid header
                    if let Ok(secs) = retry_after.to_str() {
                        if let Ok(secs) = secs.parse::<u64>() {
                            let retry_after = Some(SystemTime::now() + Duration::from_secs(secs));
                            *self.retry_after.write().await = retry_after;
                        }
                    }
                }
                _ => *self.retry_after.write().await = None,
            }
        }
        res
    }
}

#[cfg(test)]
mod test {
    use crate::RetryAfterMiddleware;
    use httpmock::{Method::GET, MockServer};
    use reqwest_middleware::ClientBuilder;
    use std::{sync::Arc, time::SystemTime};

    #[tokio::test]
    async fn test() {
        // create
        let ra_test_duration = 2;
        let middleware = Arc::new(RetryAfterMiddleware::new());

        // build client with middleware
        let client = ClientBuilder::new(reqwest::Client::new())
            .with_arc(middleware.clone())
            .build();

        test_empty_retry_after(&middleware).await;

        // create mock server
        let server = MockServer::start();
        let ra_mock = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200)
                .header("Retry-After", ra_test_duration.to_string())
                .body("");
        });
        let no_ra_mock = server.mock(|when, then| {
            when.method(GET).path("/normal");
            then.status(200).body("");
        });

        // create request
        let now = SystemTime::now();
        client.get(server.url("/")).send().await.unwrap();
        ra_mock.assert_async().await;

        test_some_retry_after(&middleware).await;
        test_valid_retry_after(&middleware, now, ra_test_duration).await;

        // verify that we actually sleep for the duration of the retry-after header
        let now = SystemTime::now();
        client.get(server.url("/normal")).send().await.unwrap();
        no_ra_mock.assert_async().await;
        let duration = SystemTime::now().duration_since(now).unwrap();

        assert!(duration.as_secs_f64() >= ra_test_duration as f64);
        test_empty_retry_after(&middleware).await;
    }

    async fn test_valid_retry_after(
        middleware: &Arc<RetryAfterMiddleware>,
        now: SystemTime,
        ra_dur: u32,
    ) {
        let time = *middleware.retry_after.read().await;
        assert!(time.is_some());
        let time = time.unwrap();
        let duration = time.duration_since(now);
        assert!(duration.is_ok());
        let duration = duration.unwrap();
        assert!(duration.as_secs_f64() >= ra_dur as f64);
    }

    async fn test_some_retry_after(middleware: &Arc<RetryAfterMiddleware>) {
        assert!(middleware.retry_after.read().await.is_some());
    }

    async fn test_empty_retry_after(middleware: &Arc<RetryAfterMiddleware>) {
        assert!(middleware.retry_after.read().await.is_none());
    }
}
