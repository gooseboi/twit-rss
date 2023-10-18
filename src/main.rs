use color_eyre::{
    eyre::{bail, eyre, Context, Result},
    Report,
};
use fantoccini::Client;
use indexmap::IndexSet;
use regex::Regex;
use scraper::{Html, Selector};
use std::sync::Arc;

mod client;
mod config;
mod driver_pool;
mod utils;

use config::Config;
use driver_pool::DriverPool;
use utils::{get_post_full_link, sleep_secs};

fn has_classes(e: scraper::ElementRef, classes: &[&str]) -> bool {
    classes.iter().all(|class| {
        e.value()
            .has_class(class, scraper::CaseSensitivity::AsciiCaseInsensitive)
    })
}

struct FetchedUser {
    _name: String,
    _description: String,
    _pfp_url: String,
    _banner_url: String,
}

async fn get_user_info(_c: &Client, _user_id: &str, _config: &Config) -> Result<FetchedUser> {
    bail!("User info not implemented yet");
}

async fn get_user_link(username: &str) -> String {

}

async fn get_recent_posts_from_user(c: &Client, user_id: &str, config: &Config) -> Result<Vec<()>> {
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

    while links.len() < config.fetch_config.max_links_per_fetch
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

async fn get_post(_c: &Client, link: &str) -> Result<()> {
    let _full_link = get_post_full_link(link);
    //c.goto(full_link).await?;
    //sleep_secs(3).await;
    bail!("TODO: Cannot download a post yet");
}

async fn get_users_from_following(c: &Client, config: &Config) -> Result<Vec<String>> {
    c.goto(&format!(
        "https://twitter.com/{user}/following",
        user = config.fetch_config.fetch_username
    ))
    .await?;
    sleep_secs(6).await;
    let anchor_selector = &Selector::parse("a").unwrap();
    let following_users_classes = config.twitter_config.css_class("following_users")?;
    let mut users = IndexSet::new();

    let mut retries = 0;
    let max_retries = config.fetch_config.max_retries;
    while retries < max_retries {
        c.execute("window.scrollBy(0,100);", vec![]).await?;
        sleep_secs(1 * (retries + 1)).await;

        let s = c.source().await?;
        let doc = Html::parse_document(&s);
        let users_iter = doc
            .select(anchor_selector)
            .filter(|a| has_classes(*a, &following_users_classes))
            .filter_map(|a| {
                a.value()
                    .attr("href")
                    .map(|s| s.get(1..).unwrap().to_owned())
            });

        let old_len = users.len();
        users.extend(users_iter);
        let diff = users.len() - old_len;
        if diff == 0 {
            retries += 1;
        } else {
            retries = 0;
        }
        println!("{retries}/{max_retries} retries");
        println!("Got {} users so far", users.len());
        println!("{users:#?}");
    }

    println!("Ended searching with {} users", users.len());

    Ok(users.into_iter().collect())
}

async fn run(pool: Arc<DriverPool>, config: Config) -> Result<()> {
    let client = pool
        .get_client(&config.twitter_config)
        .await
        .wrap_err("Could not get client")?
        .ok_or(eyre!("No clients available!"))?;

    let users = match get_users_from_following(&client, &config)
        .await
        .wrap_err("Failed getting users")
    {
        Ok(users) => users,
        Err(e) => {
            client.close().await?;
            return Err(e);
        }
    };

    client.close().await?;

    let max_concurrent_users = config.fetch_config.max_concurrent_users;
    let (user_tx, user_rx) = async_channel::unbounded();
    let mut rxs = vec![];
    for _ in 0..max_concurrent_users {
        rxs.push(user_rx.clone());
    }

    //let (post_tx, post_rx) = broadcast::channel(config.driver_config.driver_count*2);
    let config = Arc::new(config);
    for user in users {
        user_tx.send(user).await?;
    }
    println!("Sent all users through channel");

    let mut tasks = vec![];
    for i in 0..config.fetch_config.max_concurrent_users {
        let user_rx = rxs.pop().unwrap();
        let pool = Arc::clone(&pool);
        let config = Arc::clone(&config);
        println!("Starting user fetch task {i}");
        let handle = tokio::spawn(async move {
            println!("Started user fetch task {i}");
            let c = pool
                .get_client(&config.twitter_config)
                .await
                .wrap_err("Failed getting a client to download users")?;
            let c = c.ok_or(eyre!("Failed getting a client to download users, even thought there should be some available"))?;
            println!("Successfully got client");
            loop {
                println!("Starting user loop");
                let user = match user_rx.try_recv() {
                    Ok(u) => u,
                    Err(e) => match e {
                        async_channel::TryRecvError::Closed => bail!("Channel closed unexpectedly"),
                        async_channel::TryRecvError::Empty => {
                            println!("Finished processing messages");
                            break;
                        }
                    },
                };
                println!("Received user {user} in queue");
                let user_link = get_user_link(user);
                let _user_info = get_user_info(&c, &user_link, &config).await?;
                let _posts = get_recent_posts_from_user(&c, &user_link, &config).await?;
            }
            c.close().await?;
            Ok::<(), Report>(())
        });
        tasks.push(handle);
    }
    for task in tasks {
        let task = task.await?;
        if let Err(e) = task {
            println!("Task encountered an error: {e}",);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let config = Config::get().wrap_err("Failed getting config")?;

    let pool = DriverPool::new(&config.driver_config).wrap_err("Failed creating pool")?;
    let pool = Arc::new(pool);

    if let e @ Err(_) = run(Arc::clone(&pool), config).await {
        pool.close().await.wrap_err("Failed closing drivers")?;
        return e;
    } else {
        pool.close().await.wrap_err("Failed closing drivers")
    }
}
