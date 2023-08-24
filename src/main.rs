use fantoccini::{wd::Capabilities, ClientBuilder, Locator};

#[tokio::main]
async fn main() -> Result<(), fantoccini::error::CmdError> {
    let mut firefox_args = serde_json::Map::new();
    firefox_args.insert(
        "args".into(),
        serde_json::Value::Array(vec!["--headless".into(), "".into()]),
    );
    let mut prefs = serde_json::Map::new();
    prefs.insert("javascript.enabled".into(), serde_json::Value::Bool(true));
    firefox_args.insert("prefs".into(), serde_json::Value::Object(prefs));
    let mut caps = Capabilities::new();
    caps.insert(
        "moz:firefoxOptions".into(),
        serde_json::Value::Object(firefox_args),
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
