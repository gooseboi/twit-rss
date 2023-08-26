use fantoccini::{cookies::Cookie, wd::Capabilities, Client, ClientBuilder, Locator};
use serde_json::json;
use std::env;
use tokio::time::{sleep, Duration};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use sqlx::{Connection, SqliteConnection};


async fn auth(c: &Client) -> Result<Vec<Cookie<'static>>, fantoccini::error::CmdError> {
    let username =
        env::var("TWITTER_USERNAME").expect("Could not load twitter username from environment!");
    let password =
        env::var("TWITTER_PASSWORD").expect("Could not load twitter password from environment!");

    c.goto("https://twitter.com/").await?;
    sleep(Duration::from_secs(5)).await;
    if c.source().await?.as_str().contains("This page is down") {
        panic!("Twitter is down");
    }

    c.find(Locator::XPath(
        "/html/body/div/div/div/div[2]/main/div/div/div[1]/div/div/div[3]/div[5]/a/div",
    ))
    .await?
    .click()
    .await?;
    println!("Opened the sign in box");
    sleep(Duration::from_secs(3)).await;
    c.find(Locator::XPath("/html/body/div[1]/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[5]/label/div/div[2]/div/input")).await?.click().await?;
    println!("Clicked on the username box");
    sleep(Duration::from_secs(3)).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[5]/label/div/div[2]/div/input")).await?.send_keys(username.as_str()).await?;
    println!("Typed in the username box");
    sleep(Duration::from_secs(1)).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[6]")).await?.click().await?;
    println!("Clicked on the next button");
    sleep(Duration::from_secs(5)).await;

    if c.source()
        .await?
        .as_str()
        .contains("Enter your phone number")
    {
        println!("Got the confirmation dialog");
        c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[1]/div/div[2]/label/div/div[2]/div/input")).await?.send_keys(username.as_str()).await?;
        println!("  Inputted the username");
        sleep(Duration::from_secs(2)).await;
        c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[2]/div/div/div/div/div")).await?.click().await?;
        println!("  Clicked on the button");
        sleep(Duration::from_secs(3)).await;
    }

    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[1]/div/div/div[3]/div/label/div/div[2]/div[1]/input")).await?.send_keys(password.as_str()).await?;
    println!("Typed in the pasword");
    sleep(Duration::from_secs(3)).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[2]/div/div[1]/div/div/div/div")).await?.click().await?;
    println!("Clicked on the log in button");
    sleep(Duration::from_secs(7)).await;

    c.get_all_cookies().await
}

#[tokio::main]
async fn main() -> Result<(), fantoccini::error::CmdError> {
    let mut caps = Capabilities::new();
    caps.insert(
        "moz:firefoxOptions".into(),
        json!({
            "prefs": {
                "javascript.enabled": true
            },
        }),
    );
    println!("Starting with caps: {caps:?}");

    let c = ClientBuilder::rustls()
        .capabilities(caps)
        .connect("http://localhost:4444")
        .await
        .expect("failed to connect to WebDriver");

    println!("Attempting to load cached_auth");
    let cached = tokio::fs::File::open("cached_auth").await;
    if let Ok(mut f) = cached {
        println!("Found cached auth!");
        let mut contents = vec![];
        f.read_to_end(&mut contents).await.unwrap();
        // For some reason, Clients can only add cookies with 'static, so
        // this must be leaked
        let s = String::from_utf8(contents).unwrap().leak();
        let cookie = Cookie::parse(&*s).unwrap();
        c.goto("https://twitter.com").await?;
        c.add_cookie(cookie).await?;
        c.refresh().await?;
    } else {
        println!("Reloading cached_auth");
        let res = auth(&c).await;
        if let Err(e) = res {
            c.close().await?;
            return Err(e);
        }

        let cookie=res.unwrap().iter().filter(|c| c.name() == "auth_token").last().unwrap().clone();
        let mut f = tokio::fs::OpenOptions::new().write(true).create(true).open("cached_auth").await.unwrap();
        f.write_all(cookie.to_string().as_str().as_bytes()).await.unwrap();
    }

    sleep(Duration::from_secs(5)).await;



    c.close().await
}
