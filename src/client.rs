use color_eyre::eyre::{bail, eyre, Result};
use fantoccini::{cookies::Cookie, Client, Locator};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{config::TwitterConfig, utils::sleep_secs};

async fn auth(c: &Client, config: &TwitterConfig) -> Result<Cookie<'static>> {
    let username = config.username();
    let password = config.password();

    c.goto("https://twitter.com/").await?;
    sleep_secs(5).await;
    if c.source().await?.as_str().contains("This page is down") {
        bail!("Twitter is down");
    }

    c.find(Locator::XPath(
        "/html/body/div/div/div/div[2]/main/div/div/div[1]/div/div/div[3]/div[5]/a/div",
    ))
    .await?
    .click()
    .await?;
    println!("Opened the sign in box");
    sleep_secs(3).await;
    c.find(Locator::XPath("/html/body/div[1]/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[5]/label/div/div[2]/div/input")).await?.click().await?;
    println!("Clicked on the username box");
    sleep_secs(3).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[5]/label/div/div[2]/div/input")).await?.send_keys(username).await?;
    println!("Typed in the username box");
    sleep_secs(1).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[6]")).await?.click().await?;
    println!("Clicked on the next button");
    sleep_secs(5).await;

    if c.source()
        .await?
        .as_str()
        .contains("Enter your phone number")
    {
        println!("Got the confirmation dialog");
        c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[1]/div/div[2]/label/div/div[2]/div/input")).await?.send_keys(username).await?;
        println!("  Inputted the username");
        sleep_secs(2).await;
        c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[2]/div/div/div/div/div")).await?.click().await?;
        println!("  Clicked on the button");
        sleep_secs(3).await;
    }

    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[1]/div/div/div[3]/div/label/div/div[2]/div[1]/input")).await?.send_keys(password).await?;
    println!("Typed in the pasword");
    sleep_secs(3).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[2]/div/div[1]/div/div/div/div")).await?.click().await?;
    println!("Clicked on the log in button");
    sleep_secs(7).await;

    Ok(c.get_all_cookies()
        .await?
        .iter()
        .filter(|c| c.name() == "auth_token")
        .last()
        .ok_or(eyre!("Failed to get cookie with name `auth_token`"))?
        .clone())
}

pub async fn set_auth_cookie(c: &Client, config: &TwitterConfig) -> Result<()> {
    println!("Loading auth");
    let cached = tokio::fs::File::open(&config.auth_cache_fname).await;
    if let Ok(mut f) = cached {
        println!("Found cached auth!");
        let mut contents = vec![];
        f.read_to_end(&mut contents).await.unwrap();
        // For some reason, Clients can only add cookies with 'static, so
        // this must be leaked
        let s = String::from_utf8(contents).unwrap().into_boxed_str();
        let s_ptr = Box::into_raw(s);
        // SAFETY: Since we already own this pointer, doubly referencing it so that Cookie::parse
        // can use it. Moreover, having double references doesn't matter, as we do not care about
        // the value and Cookie::parse also does not modify it.
        let s = unsafe { Box::from_raw(s_ptr) };
        let cookie = Cookie::parse(&*Box::leak(s)).unwrap();
        c.goto("https://twitter.com").await?;
        c.delete_all_cookies().await?;
        c.add_cookie(cookie).await?;
        c.refresh().await?;
        // SAFETY: We know that this pointer is valid, and moreover that we own
        // it, and therefore can drop it
        unsafe { drop(Box::from_raw(s_ptr)) };
    } else {
        println!("Reloading auth from site");
        let cookie = auth(c, config).await?;
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&config.auth_cache_fname)
            .await
            .unwrap();
        f.write_all(cookie.to_string().as_str().as_bytes())
            .await
            .unwrap();
    }
    Ok(())
}
