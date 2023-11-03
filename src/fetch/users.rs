use color_eyre::{
    eyre::{bail, eyre, Context},
    Result,
};
use fantoccini::{error::CmdError, Client, Locator};
use indexmap::IndexSet;
use scraper::{Html, Selector};
use tracing::{debug, info, span, warn, Level, Span};

use crate::config::Config;
use crate::utils::{has_classes, sleep_secs};

#[derive(Debug, Clone)]
pub struct FetchedUser {
    display_name: String,
    username: String,
    description: String,
    date_created: String,
    related_link: Option<String>,
    location: Option<String>,
    following: usize,
    followers: usize,
    pfp_url: String,
    banner_url: String,
}

mod json {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct JsonFetchedUser {
        pub display_name: String,
        pub username: String,
        pub description: String,
        pub date_created: String,
        pub related_link: Option<String>,
        pub location: Option<String>,
        pub following: usize,
        pub followers: usize,
        pub pfp_url: String,
    }

    pub fn try_get_info_from_json(span: Span, src: &str) -> Option<JsonFetchedUser> {
        let doc = Html::parse_document(src);
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

        let user = JsonFetchedUser {
            pfp_url,
            display_name,
            username,
            description,
            date_created,
            related_link,
            location,
            following,
            followers,
        };

        Some(user)
    }
}

mod page {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct PageUserInfo {
        pub display_name: String,
        pub username: String,
        pub description: String,
        pub date_created: String,
        pub related_link: Option<String>,
        pub location: Option<String>,
        pub following: Option<usize>,
        pub followers: Option<usize>,
    }

    pub fn try_get_info_from_page(_user: &str, src: &str) -> Result<PageUserInfo> {
        let doc = Html::parse_document(src);
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

fn get_banner_url_impl(src: &str) -> Result<String> {
    let doc = Html::parse_document(src);
    let img_selector = &Selector::parse("img").unwrap();
    doc.select(img_selector)
        .find(|i| i.value().attr("alt").map(|s| s == "Image").unwrap_or(false))
        .ok_or(eyre!("Failed to find banner in page"))
        .map(|i| i.value().attr("src").ok_or(eyre!("Image had no src attr")))
        .and_then(|v| v)
        .map(|s| s.to_owned())
}

async fn get_banner_url(c: &Client, user_link: &str, config: &Config) -> Result<String> {
    // Click on the banner
    c.find(Locator::XPath(config.twitter_config.xpath("banner_img")?))
        .await?
        .click()
        .await?;
    sleep_secs(5).await;

    let src = c.source().await?;
    let res = get_banner_url_impl(&src);
    // Exit out
    match c
        .find(Locator::XPath(config.twitter_config.xpath("banner_exit")?))
        .await
    {
        Ok(e) => e.click().await?,
        Err(CmdError::NoSuchElement(_)) => goto_user_profile(c, user_link).await?,
        Err(e) => return Err(e.into()),
    }
    res
}

pub async fn get_user_info(
    c: &Client,
    user: &str,
    user_link: &str,
    config: &Config,
) -> Result<FetchedUser> {
    // TODO: Retry maybe?
    goto_user_profile(c, user_link).await?;

    let span = span!(Level::INFO, "info_from_json");
    let src = c.source().await?;
    if let Some(u) = json::try_get_info_from_json(span, &src) {
        info!("Got user info for {user} from json");
        let banner_url = get_banner_url(c, user_link, config)
            .await
            .wrap_err("Failed getting banner_url")?;
        info!("Got banner_url for {user} after json");
        return Ok(FetchedUser {
            display_name: u.display_name,
            username: u.username,
            description: u.description,
            date_created: u.date_created,
            related_link: u.related_link,
            location: u.location,
            following: u.following,
            followers: u.followers,
            pfp_url: u.pfp_url,
            banner_url,
        });
    } else {
        warn!("Failed getting user info for {user} from json");
    }
    let _ = span;

    // This is a workaround for an issue that occurs when the divs are in the same scope as the
    // below await call.
    // Since they use `Cell`s, they are not Send, and the compiler complains execution may stop
    // while they are still in scope. However, we know that after this point they are out of
    // scope. Despite this, the compiler doesn't realise, and this is the workaround.
    let src = c.source().await?;
    let _page_user_info = page::try_get_info_from_page(user, &src)?;

    c.find(Locator::XPath("/html/body/div[1]/div/div/div[2]/main/div/div/div/div/div/div[3]/div/div/div/div/div[1]/div[1]")).await?;

    bail!("TODO: Getting user info not implemented yet")
}

pub async fn get_users_from_following(c: &Client, config: &Config) -> Result<Vec<String>> {
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
        sleep_secs(config.fetch_config.users_from_following_retry_delay * (retries + 1)).await;
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
