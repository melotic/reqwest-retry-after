//! # reqwest-retry-after
//!
//! `reqwest-retry-after` is a library that adds support for the `Retry-After` header
//! in [`reqwest`], using [`reqwest_middleware`].
//!
//! ## Usage
//!
//! Pass [`RetryAfterMiddleware`] to the [`ClientWithMiddleware`] builder.
//!
//! ```
//! use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
//! use reqwest_retry_after::RetryAfterMiddleware;
//!
//! let client = ClientBuilder::new(reqwest::Client::new())
//!     .with(RetryAfterMiddleware::new())
//!     .build();
//! ```
#![warn(missing_docs)]
#![warn(rustdoc::missing_doc_code_examples)]

use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use http::{header::RETRY_AFTER, Extensions};
use reqwest::Url;
use reqwest_middleware::{
    reqwest::{Request, Response},
    Middleware, Next, Result,
};
use time::{format_description::well_known::Rfc2822, OffsetDateTime};
use tokio::sync::RwLock;

/// The `RetryAfterMiddleware` is a [`Middleware`] that adds support for the `Retry-After`
/// header in [`reqwest`].
pub struct RetryAfterMiddleware {
    retry_after: RwLock<HashMap<Url, SystemTime>>,
}

impl RetryAfterMiddleware {
    /// Creates a new `RetryAfterMiddleware`.
    pub fn new() -> Self {
        Self {
            retry_after: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for RetryAfterMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_retry_value(val: &str) -> Option<SystemTime> {
    if let Ok(secs) = val.parse::<u64>() {
        return Some(SystemTime::now() + Duration::from_secs(secs));
    }
    if let Ok(date) = OffsetDateTime::parse(val, &Rfc2822) {
        return Some(date.into());
    }
    None
}

#[async_trait::async_trait]
impl Middleware for RetryAfterMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let url = req.url().clone();

        if let Some(timestamp) = self.retry_after.read().await.get(&url) {
            let now = SystemTime::now();

            if let Ok(duration) = timestamp.duration_since(now) {
                tokio::time::sleep(duration).await;
            }
        }

        let res = next.run(req, extensions).await;

        if let Ok(res) = &res {
            match res.headers().get(RETRY_AFTER) {
                Some(retry_after) => {
                    if let Ok(val) = retry_after.to_str() {
                        if let Some(timestamp) = parse_retry_value(val) {
                            self.retry_after
                                .write()
                                .await
                                .insert(url.clone(), timestamp);
                        }
                    }
                }
                _ => {
                    self.retry_after.write().await.remove(&url);
                }
            }
        }
        res
    }
}

#[cfg(test)]
mod test {
    use crate::RetryAfterMiddleware;
    use httpmock::{Method::GET, MockServer};
    use reqwest::Url;
    use reqwest_middleware::ClientBuilder;
    use std::{str::FromStr, sync::Arc, time::SystemTime};

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
            when.method(GET).path("/").header("RA", "true");
            then.status(200)
                .header("Retry-After", ra_test_duration.to_string())
                .body("");
        });
        let no_ra_mock = server.mock(|when, then| {
            when.method(GET).path("/");
            then.status(200).body("");
        });
        let normal_mock = server.mock(|when, then| {
            when.method(GET).path("/normal");
            then.status(200).body("");
        });

        let url = Url::from_str(&server.url("/")).unwrap();

        // hit URL; get RA value and store it
        let now = SystemTime::now();
        client
            .get(url.clone())
            .header("RA", "true")
            .send()
            .await
            .unwrap();
        ra_mock.assert_async().await;
        test_some_retry_after(&middleware, &url).await;
        test_valid_retry_after(&middleware, &url, now, ra_test_duration).await;

        // hit same URL with stored RA; this should (1) sleep and (2) clear the stored RA afterward
        // meanwhile, hit other URL, which should return instantly
        let now = SystemTime::now();
        client.get(server.url("/normal")).send().await.unwrap();
        client.get(url.clone()).send().await.unwrap();
        normal_mock.assert_async().await;
        no_ra_mock.assert_async().await;
        let duration = SystemTime::now().duration_since(now).unwrap();

        // verify that we actually slept for the duration of the retry-after header
        assert!(duration.as_secs_f64() >= ra_test_duration as f64);
        test_empty_retry_after(&middleware).await;
    }

    async fn test_valid_retry_after(
        middleware: &Arc<RetryAfterMiddleware>,
        url: &Url,
        now: SystemTime,
        ra_dur: u32,
    ) {
        let time = middleware.retry_after.read().await.get(url).cloned();
        assert!(time.is_some());
        let time = time.unwrap();
        let duration = time.duration_since(now);
        assert!(duration.is_ok());
        let duration = duration.unwrap();
        assert!(duration.as_secs_f64() >= ra_dur as f64);
    }

    async fn test_some_retry_after(middleware: &Arc<RetryAfterMiddleware>, url: &Url) {
        assert!(middleware.retry_after.read().await.get(url).is_some());
    }

    async fn test_empty_retry_after(middleware: &Arc<RetryAfterMiddleware>) {
        assert!(middleware.retry_after.read().await.is_empty());
    }
}
