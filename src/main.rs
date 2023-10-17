use color_eyre::eyre::{bail, eyre, Context, Result};
use fantoccini::Client;
use regex::Regex;
use scraper::{Html, Selector};

mod client;
mod config;
mod driver_pool;
mod utils;

use config::Config;
use driver_pool::DriverPool;
use utils::sleep_secs;

#[derive(Debug)]
struct Post {
    link: String,
    date: u64,
    text: String,
    repost_link: Option<String>,
    repost_date: u64,
}

async fn get_recent_posts_for_user(
    c: &Client,
    user_id: &str,
    config: &Config,
) -> Result<Vec<Post>> {
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

fn get_full_link(link: &str) -> String {
    if link.starts_with("https://twitter.com") {
        // "https://twitter.com/..."
        link.to_owned()
    } else if link.starts_with("twitter.com") {
        // "twitter.com/..."
        format!("https://{link}")
    } else {
        // "/..."
        format!("https://twitter.com{link}")
    }
}

async fn get_post(c: &Client, link: &str) -> Result<Post> {
    let full_link = get_full_link(link);
    //c.goto(full_link).await?;
    //sleep_secs(3).await;
    bail!("TODO: Cannot download a post yet");
}

async fn get_users_from_following(c: &Client, user: &str) -> Result<Vec<String>> {
    bail!("TODO: Cannot get users from a subscription yet");
}

async fn run(pool: &DriverPool, config: &Config) -> Result<()> {
    let client = pool
        .get_client(&config.twitter_config)
        .await
        .wrap_err("Could not get client")?
        .ok_or(eyre!("No clients available!"))?;

    let users = get_users_from_following(&client, "<username>")
        .await
        .wrap_err("Failed getting users")?;

    for user in users {
        println!("{user}");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let config = Config::get().wrap_err("Failed getting config")?;

    let pool = DriverPool::new(&config.driver_config).wrap_err("Failed creating pool")?;

    if let e @ Err(_) = run(&pool, &config).await {
        pool.close().wrap_err("Failed closing drivers")?;
        return e;
    } else {
        pool.close().wrap_err("Failed closing drivers")
    }
}
