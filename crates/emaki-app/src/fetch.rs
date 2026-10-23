//! gpui's HTTP client. gpui has none of its own, and an `img` of an
//! address (a picture a markdown file shows from `https://…`) asks it for
//! the bytes; without one the picture was drawn as nothing. A GET through
//! the core's `web::get`, on a thread of its own.

use futures::FutureExt as _;
use http_client::{AsyncBody, HttpClient, Url, http};

pub(crate) struct Fetcher;

impl HttpClient for Fetcher {
    fn user_agent(&self) -> Option<&http::HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(&self, req: http::Request<AsyncBody>) -> futures::future::BoxFuture<'static, anyhow::Result<http::Response<AsyncBody>>> {
        let url = req.uri().to_string();
        let get = req.method() == http::Method::GET;
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(if get { emaki_core::web::get(&url) } else { Err("only a GET is made".into()) });
        });
        async move {
            let fetched = rx.await.map_err(|_| anyhow::anyhow!("the fetch ended with no answer"))?.map_err(|e| anyhow::anyhow!(e))?;
            let mut response = http::Response::builder().status(fetched.status);
            for (name, value) in fetched.headers {
                response = response.header(name, value);
            }
            Ok(response.body(AsyncBody::from(fetched.body))?)
        }
        .boxed()
    }
}
