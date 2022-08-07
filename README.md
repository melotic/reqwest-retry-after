# reqwest-retry-after

![Crates.io](https://img.shields.io/crates/v/reqwest-retry-after)
![docs.rs](https://img.shields.io/docsrs/reqwest-retry-after/latest)
![Crates.io](https://img.shields.io/crates/l/reqwest-retry-after)

`reqwest-retry-after` is a library that adds support for the `Retry-After` header in [reqwest](https://github.com/seanmonstar/reqwest), using [reqwest_middleware](https://github.com/TrueLayer/reqwest-middleware).

## Usage

Simply pass `RetryAfterMiddleware` to the `ClientWithMiddleware` builder.

```rust
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry_after::RetryAfterMiddleware;

let client = ClientBuilder::new(reqwest::Client::new())
    .with(RetryAfterMiddleware::new())
    .build();
```
