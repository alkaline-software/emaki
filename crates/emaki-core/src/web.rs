//! A file fetched off the web for the window: a picture a markdown file
//! shows from an address (`![](https://…)`, `<img src="https://…">`),
//! which gpui asks for through the app's HTTP client (`fetch.rs`). Only a
//! GET, and nothing is sent with it.

use std::io::Read;
use std::time::Duration;

/// The most of a body that is read: past it a file is no picture to show
/// in a pane.
pub const MOST: u64 = 64 << 20;

/// What a GET answered.
pub struct Fetched {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// GET `url`, following its redirects. An answer of any status is
/// returned as it is; an error is the network's.
pub fn get(url: &str) -> Result<Fetched, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(8)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&format!("emaki/{}", crate::update::current_version()))
        .build();
    let response = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(e.to_string()),
    };
    let status = response.status();
    let headers = response.headers_names().into_iter().filter_map(|name| response.header(&name).map(|value| (name.clone(), value.to_string()))).collect();
    let mut body = Vec::new();
    response.into_reader().take(MOST).read_to_end(&mut body).map_err(|e| e.to_string())?;
    Ok(Fetched { status, headers, body })
}
