use color_eyre::{
    eyre::{bail, eyre, Context, Result},
    Report,
};
use fantoccini::{error::CmdError, Client, Locator};
use indexmap::IndexSet;
use regex::Regex;
use scraper::{Html, Selector};
use std::sync::Arc;
use tracing::{debug, error, info, span, warn, Level, Span};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

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

#[derive(Debug, Clone)]
struct UserImgs {
    pfp_url: String,
    banner_url: String,
}

#[derive(Debug, Clone)]
struct UserInfo {
    display_name: String,
    username: String,
    description: String,
    date_created: String,
    related_link: Option<String>,
    location: Option<String>,
    following: usize,
    followers: usize,
}

#[derive(Debug, Clone)]
struct FetchedUser {
    imgs: UserImgs,
    info: UserInfo,
}

async fn goto_user_profile(c: &Client, user_link: &str) -> Result<()> {
    c.goto(user_link).await?;
    sleep_secs(4).await;
    // Find "Yes, view profile" button for NSFW profiles
    match c.find(Locator::XPath("/html/body/div[1]/div/div/div[2]/main/div/div/div/div/div/div[3]/div/div/div[2]/div/div[3]/div")).await {
        Ok(e) => e.click().await?,
        Err(CmdError::NoSuchElement(_)) => {}
        Err(e) => return Err(e.into()),
    };
    sleep_secs(4).await;

    Ok(())
}

fn try_get_info_from_json(span: Span, doc: Html) -> Option<FetchedUser> {
    let _enter = span.enter();
    let script_selector = &Selector::parse("script").unwrap();

    let s = doc.select(script_selector).find(|s| {
        s.value()
            .attr("type")
            .map(|t| t == "application/ld+json")
            .unwrap_or(false)
            && s.value()
                .attr("data-testid")
                .map(|d| d == "UserProfileSchema-test")
                .unwrap_or(false)
    })?;

    let json = s.text().collect::<String>();
    let parsed_json: serde_json::Value = serde_json::from_str(&json).ok()?;
    let map = parsed_json.as_object()?;

    let date_created = map.get("dateCreated")?.as_str()?.to_owned();
    debug!(date_created);

    let related_link = map
        .get("relatedLink")
        .and_then(|v| v.as_array())
        .and_then(|v| {
            let e = v.get(1);
            if e.is_none() {
                warn!("Failed getting related link from array {v:#?}");
            };
            e
        })
        .and_then(|s| {
            if s.as_str().is_none() {
                warn!("relatedLink value was not a string (What?): {s}");
            };
            s.as_str()
        })
        .map(|s| s.to_owned());
    debug!(?related_link);

    let author = map.get("author").and_then(|m| m.as_object())?;

    let display_name = author
        .get("givenName")
        .and_then(|s| s.as_str())
        .map(|s| s.to_owned())?;
    debug!(display_name);

    let username = author
        .get("additionalName")
        .and_then(|s| s.as_str())
        .map(|s| s.to_owned())?;
    debug!(username);

    let description = author
        .get("description")
        .and_then(|s| s.as_str())
        .map(|s| s.to_owned())?;
    debug!(description);

    let location = author
        .get("homeLocation")
        .and_then(|h| h.as_object())
        .and_then(|m| {
            if m.get("@type").map(|ty| ty != "Place").unwrap_or(false) {
                warn!(
                    "homeLocation.type is not `Place`: `{ty}`",
                    ty = m.get("@type").unwrap()
                );
                None
            } else {
                m.get("name")
            }
        })
        .and_then(|p| p.as_str().map(|s| s.to_owned()))
        .and_then(|s| if s.is_empty() { None } else { Some(s) });
    debug!(?location);

    let Some(interactions) = author
        .get("interactionStatistic")
        .and_then(|m| m.as_array())
    else {
        warn!("No interactions");
        return None;
    };

    let followers = interactions
        .iter()
        .map(|item| item.as_object())
        .find(|map| {
            map.and_then(|map| map.get("name").map(|s| s == "Follows"))
                .unwrap_or(false)
        })
        .flatten()
        .and_then(|m| m.get("userInteractionCount"))
        .and_then(|v| v.as_u64())? as usize;
    debug!(followers);

    let following = interactions
        .iter()
        .map(|item| item.as_object())
        .find(|map| {
            map.and_then(|map| map.get("name").map(|s| s == "Friends"))
                .unwrap_or(false)
        })
        .flatten()
        .and_then(|m| m.get("userInteractionCount"))
        .and_then(|v| v.as_u64())? as usize;
    debug!(following);

    let image = author.get("image").and_then(|m| m.as_object())?;

    let pfp_url = image
        .get("contentUrl")
        .and_then(|s| s.as_str())
        .map(|s| s.to_owned())?;
    debug!(pfp_url);

    warn!("TODO: Fetching banner url");
    return None;

    let user = FetchedUser {
        imgs: UserImgs {
            pfp_url,
            banner_url: String::new(),
        },
        info: UserInfo {
            display_name,
            username,
            description,
            date_created,
            related_link,
            location,
            following,
            followers,
        },
    };

    Some(user)
}

fn try_get_info_from_page(_user: &str, doc: Html) -> Result<UserInfo> {
    let div_selector = &Selector::parse("div").unwrap();
    let anchor_selector = &Selector::parse("a").unwrap();
    let username_div = doc
        .select(div_selector)
        .find(|d| {
            d.value()
                .attr("data-testid")
                .map(|s| s == "UserName")
                .unwrap_or(false)
        })
        .ok_or(eyre!("Failed to find username"))?;

    let text = username_div.text().collect::<String>();
    let mut iter = text.split('@').map(|s| s.trim().to_owned());
    let _display_name = iter
        .next()
        .ok_or(eyre!("Failed to find user display name"))?;
    let _username = iter.next().ok_or(eyre!("Failed to find username"))?;

    let description_div = doc
        .select(div_selector)
        .find(|d| {
            d.value()
                .attr("data-testid")
                .map(|s| s == "UserDescription")
                .unwrap_or(false)
        })
        .ok_or(eyre!("Failed to find user description"))?;

    let _description = description_div.text().collect::<String>();

    let mut following_anchor = doc.select(anchor_selector).filter(|a| {
        a.value()
            .attr("href")
            .map(|s| s.contains("following"))
            .unwrap_or(false)
    });
    let a = following_anchor.next().ok_or(eyre!(
        "No element with link `following` to extract following count"
    ))?;
    let text = a.text().collect::<String>();
    let following = text
        .split_whitespace()
        .next()
        .ok_or(eyre!("Failed to find following count"))?;
    // If the count is big enough, it truncates the count and displays it abbreviated.
    // i.e. 200_000 = 200K
    let _following: Option<usize> = if following.contains(|c| c == 'K' || c == 'M') {
        None
    } else {
        let n = following
            .parse()
            .wrap_err("Failed parsing following count")?;
        Some(n)
    };

    let mut followers_anchor = doc.select(anchor_selector).filter(|a| {
        a.value()
            .attr("href")
            .map(|s| s.contains("followers"))
            .unwrap_or(false)
    });
    let a = followers_anchor.next().ok_or(eyre!(
        "No element with link `follower` to extract followers count"
    ))?;
    let text = a.text().collect::<String>();
    let followers = text
        .split_whitespace()
        .next()
        .ok_or(eyre!("Failed to find followers count"))?;
    // Same here
    let _followers: Option<usize> = if followers.contains(|c| c == 'K' || c == 'M') {
        None
    } else {
        let n = followers
            .parse()
            .wrap_err("Failed parsing followers count")?;
        Some(n)
    };
    debug!(_following);
    debug!(_followers);

    bail!("TODO: Unimplemented")
}

async fn get_user_info(
    c: &Client,
    user: &str,
    user_link: &str,
    _config: &Config,
) -> Result<FetchedUser> {
    // TODO: Retry maybe?
    goto_user_profile(c, user_link).await?;

    {
        let span = span!(Level::INFO, "info_from_json");
        let doc = Html::parse_document(&c.source().await?);
        if let Some(t) = try_get_info_from_json(span, doc) {
            info!("Got user info for {user} from json");
            return Ok(t);
        } else {
            warn!("Failed getting user info for {user} from json");
        }
    }
    // This is a workaround for an issue that occurs when the divs are in the same scope as the
    // below await call.
    // Since they use `Cell`s, they are not Send, and the compiler complains execution may stop
    // while they are still in scope. However, we know that after this point they are out of
    // scope. Despite this, the compiler doesn't realise, and this is the workaround.
    let doc = Html::parse_document(&c.source().await?);
    let UserInfo {
        display_name: _,
        username: _,
        description: _,
        following: _,
        followers: _,
        date_created: _,
        related_link: _,
        location: _,
    } = try_get_info_from_page(user, doc)?;

    c.find(Locator::XPath("/html/body/div[1]/div/div/div[2]/main/div/div/div/div/div/div[3]/div/div/div/div/div[1]/div[1]")).await?;

    bail!("TODO: Getting user info not implemented yet")
}

fn get_user_link(username: &str) -> String {
    format!("https://twitter.com/{username}")
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
    debug!("Downloading data for {username}");

    let re = Regex::new("^/\\w+/status/\\d+$").unwrap();
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
        debug!("Got {} posts so far", links.len());
    }

    info!("Ended searching with {} posts", links.len());

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
        if retries != 0 {
            info!("{retries}/{max_retries} retries at fetching users from following");
        }

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
        debug!("Got {} users so far", users.len());
    }

    info!("Ended searching with {} users", users.len());

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

    // TODO: Maybe do the user list fetch at the same time as the users
    // list is starting to be fetched
    client.close().await?;

    let max_concurrent_users = config.fetch_config.max_concurrent_users;
    let (user_tx, user_rx) = async_channel::unbounded();
    let mut rxs = vec![];
    for _ in 0..max_concurrent_users {
        rxs.push(user_rx.clone());
    }

    let config = Arc::new(config);
    for user in users {
        user_tx.send(user).await?;
    }

    let mut tasks = vec![];
    info!(
        "Starting {} tasks to fetch users",
        config.fetch_config.max_concurrent_users
    );
    for i in 0..config.fetch_config.max_concurrent_users {
        let user_rx = rxs.pop().unwrap();
        let pool = Arc::clone(&pool);
        let config = Arc::clone(&config);
        let handle = tokio::spawn(async move {
            let id = i;
            debug!("Started user fetch task {id}");
            let c = pool
                .get_client(&config.twitter_config)
                .await
                .wrap_err("Failed getting a client to download users")?;
            let c = c.ok_or(eyre!("Failed getting a client to download users, even thought there should be some available"))?;
            debug!("Successfully got client in task {id}");
            loop {
                let user = match user_rx.try_recv() {
                    Ok(u) => u,
                    Err(e) => match e {
                        async_channel::TryRecvError::Closed => bail!("Channel closed unexpectedly"),
                        async_channel::TryRecvError::Empty => {
                            info!("Finished processing users");
                            break;
                        }
                    },
                };
                debug!("Received user {user} in task {id}");
                let user_link = get_user_link(&user);
                let user_info = match get_user_info(&c, &user, &user_link, &config).await {
                    Ok(u) => u,
                    Err(e) => {
                        warn!("Encountered error while fetching user info for {user}: {e}");
                        continue;
                    }
                };
                info!("{user_info:#?}");
                //let _posts = get_recent_posts_from_user(&c, &user_link, &config).await?;
            }
            c.close().await?;
            Ok::<(), Report>(())
        });
        tasks.push(handle);
    }
    for task in tasks {
        let task = task.await?;
        if let Err(e) = task {
            error!("Task encountered an error: {e}",);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

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
