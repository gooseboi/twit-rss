use color_eyre::eyre::{bail, eyre, Context, Result};
use fantoccini::{cookies::Cookie, wd::Capabilities, Client, ClientBuilder, Locator};
use nix::sys::signal;
use nix::unistd::Pid;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::json;
use std::env;
use std::process::Stdio;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::time::{sleep, Duration};

const MAX_LINKS_PER_FETCH: usize = 5;
const AUTH_CACHE_FNAME: &str = "cached_auth";

const DRIVER_COUNT: usize = 5;
const BASE_PORT: usize = 8444;

static DRIVER_ARRAY: Lazy<Mutex<Vec<(Child, usize, bool)>>> = Lazy::new(|| {
    let mut v = vec![];

    for n in (0..).take(DRIVER_COUNT) {
        let port = BASE_PORT + n;
        println!("Starting driver with port {port}");
        let driver = Command::new("geckodriver")
            .arg("-p")
            .arg(format!("{port}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        v.push((driver, port, true));
    }

    Mutex::new(v)
});

fn sleep_secs(n: usize) -> tokio::time::Sleep {
    sleep(Duration::from_secs(n as u64))
}

async fn auth(c: &Client) -> Result<Vec<Cookie<'static>>> {
    let username = env::var("TWITTER_USERNAME")
        .map_err(|_| eyre!("Could not load twitter username from environment!"))?;
    let password = env::var("TWITTER_PASSWORD")
        .map_err(|_| eyre!("Could not load twitter password from environment!"))?;

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
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div/div/div/div[5]/label/div/div[2]/div/input")).await?.send_keys(username.as_str()).await?;
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
        c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[1]/div/div[2]/label/div/div[2]/div/input")).await?.send_keys(username.as_str()).await?;
        println!("  Inputted the username");
        sleep_secs(2).await;
        c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[2]/div/div/div/div/div")).await?.click().await?;
        println!("  Clicked on the button");
        sleep_secs(3).await;
    }

    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[1]/div/div/div[3]/div/label/div/div[2]/div[1]/input")).await?.send_keys(password.as_str()).await?;
    println!("Typed in the pasword");
    sleep_secs(3).await;
    c.find(Locator::XPath("/html/body/div/div/div/div[1]/div[2]/div/div/div/div/div/div[2]/div[2]/div/div/div[2]/div[2]/div[2]/div/div[1]/div/div/div/div")).await?.click().await?;
    println!("Clicked on the log in button");
    sleep_secs(7).await;

    c.get_all_cookies().await.map_err(|e| e.into())
}

async fn set_auth_cookie(c: &Client) -> Result<()> {
    println!("Loading auth");
    let cached = tokio::fs::File::open(AUTH_CACHE_FNAME).await;
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
        let s = unsafe { Box::from_raw(s_ptr.clone()) };
        let cookie = Cookie::parse(&*Box::leak(s)).unwrap();
        c.goto("https://twitter.com").await?;
        c.add_cookie(cookie).await?;
        c.refresh().await?;
        // SAFETY: We know that this pointer is valid, and moreover that we own
        // it, and therefore can drop it
        unsafe { drop(Box::from_raw(s_ptr)) };
    } else {
        println!("Reloading auth from site");
        let cookies = auth(c).await?;

        let cookie = cookies
            .iter()
            .filter(|c| c.name() == "auth_token")
            .last()
            .unwrap()
            .clone();
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(AUTH_CACHE_FNAME)
            .await
            .unwrap();
        f.write_all(cookie.to_string().as_str().as_bytes())
            .await
            .unwrap();
    }
    Ok(())
}

#[derive(Debug)]
struct Post {
    link: String,
    date: u64,
    text: String,
    repost_link: Option<String>,
    repost_date: u64,
}

async fn get_recent_posts_for_user(c: &Client, user_id: &str) -> Result<Vec<Post>> {
    c.goto(&format!("https://twitter.com/{user_id}")).await?;
    sleep_secs(4).await;
    let username = {
        let doc = Html::parse_document(&c.source().await?);
        let div_selector = &Selector::parse("div").unwrap();
        doc.select(div_selector)
            .filter(|e| {
                e.value().attrs().any(|(k, v)| {
                    k.eq_ignore_ascii_case("data-testid") && v.eq_ignore_ascii_case("UserName")
                })
            })
            .map(|e| e.text().collect::<Vec<_>>().join(" "))
            .next()
            .ok_or(eyre!("Could not find username element"))?
            .split(" @") // <username> @<user_id>
            .map(|s| {
                println!("{s}");
                s
            })
            .next()
            .ok_or(eyre!("Username was not in '<username> @<user_id>'"))?
            .trim()
            .to_owned()
    };
    println!("Downloading data for {username}");

    let re = Regex::new(&format!("^/\\w+/status/\\d+$")).unwrap();
    let anchor_selector = &Selector::parse("a").unwrap();
    let article_selector = &Selector::parse("article").unwrap();
    let user_status_format = &format!("/{user_id}");
    let mut links = indexmap::IndexSet::new();

    while links.len() < MAX_LINKS_PER_FETCH
        || !links
            .last()
            .map(|l: &String| l.starts_with(user_status_format))
            .unwrap_or(true)
    {
        c.execute("window.scrollBy(0,300);", vec![]).await?;
        sleep_secs(1).await;

        let s = c.source().await?;
        let doc = Html::parse_document(&s);
        let link_iter = doc
            .select(article_selector)
            .flat_map(|article| article.select(anchor_selector))
            .filter_map(|e| e.value().attr("href"))
            .filter(|l| re.is_match(l))
            .map(|l| l.to_owned());

        links.extend(link_iter);
        println!("Got {} posts so far", links.len());
        println!("{links:#?}");
    }

    println!("Ended searching with {} posts", links.len());

    let mut posts = vec![];
    for i in 0..links.len() {
        let link = links.get_index(i).unwrap();
        if link.starts_with(user_status_format) {
            let post = get_post(c, link).await?;
            posts.push(post);
            continue;
        } else {
            bail!("Getting retweets not supported!")
        }
    }

    Ok(posts)
}

async fn get_post(c: &Client, link: &str) -> Result<Post> {
    let full_link = &format!("https://twitter.com{link}");
    //c.goto(full_link).await?;
    //sleep_secs(3).await;
    bail!("TODO: Cannot download a post yet");
}

async fn run(c: &Client) -> Result<()> {
    set_auth_cookie(c).await?;

    let posts = get_recent_posts_for_user(c, "jonhoo")
        .await
        .wrap_err("Failed getting posts")?;
    for post in posts {
        println!("{post:?}");
    }

    Ok(())
}

struct ClientWithPort {
    client: Client,
    port: usize,
}

impl ClientWithPort {
    async fn close(self) -> Result<()> {
        self.client.close().await?;
        let mut guard = DRIVER_ARRAY.lock().unwrap();
        let Some(item) = guard.iter_mut().filter(|(_, p, _)| self.port == *p).next() else {
            bail!("Unable to find driver in array with port {}", self.port);
        };
        item.2 = true;
        drop(guard);

        Ok(())
    }

    async fn new() -> Result<Option<Self>> {
        let mut guard = DRIVER_ARRAY.lock().unwrap();
        let Some(item) = guard.iter_mut().filter(|(_, _, b)| *b).next() else {
            return Ok(None);
        };
        item.2 = false;
        let port = item.1;
        drop(guard);

        let mut caps = Capabilities::new();
        caps.insert(
            "moz:firefoxOptions".into(),
            json!({
                "prefs": {
                    "javascript.enabled": true
                },
            }),
        );

        let client = ClientBuilder::rustls()
            .capabilities(caps)
            .connect(&format!("http://localhost:{port}"))
            .await
            .wrap_err("failed to connect to WebDriver")?;

        Ok(Some(ClientWithPort { client, port: 4444 }))
    }
}

fn shutdown() -> Result<()> {
    let mut guard = DRIVER_ARRAY.lock().unwrap();
    for (child, p, _) in guard.iter_mut() {
        // TODO: Clean up children
        let pid = Pid::from_raw(child.id().unwrap() as i32);
        signal::kill(pid, signal::SIGTERM)
            .wrap_err(format!("Failed running kill on process with port {p}"))?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let c = ClientWithPort::new()
        .await
        .wrap_err("Failed getting a client")?
        .ok_or(eyre!("No clients available"))?;

    if let Err(e) = run(&c.client).await {
        shutdown().wrap_err("Failed shutting down")?;
        return Err(e);
    } else {
        shutdown()
    }
}
