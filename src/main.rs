use fantoccini::{wd::Capabilities, ClientBuilder, Locator};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), fantoccini::error::CmdError> {
    let mut caps = Capabilities::new();
    caps.insert(
        "moz:firefoxOptions".into(),
        json!({
            "prefs": {
                "javascript.enabled": true
            },
            "args": ["--headless"]
        }),
    );
    println!("Starting with caps: {caps:?}");

    let c = ClientBuilder::rustls()
        .capabilities(caps)
        .connect("http://localhost:4444")
        .await
        .expect("failed to connect to WebDriver");

    // TODO: Make JS work, somehow...
    c.goto("https://twitter.com/").await?;
    println!("Found source `{}`", c.source().await?);

    c.close().await
}
