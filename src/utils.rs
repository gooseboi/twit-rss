use std::time::Duration;
use tokio::time::sleep;

pub fn sleep_secs(n: usize) -> tokio::time::Sleep {
    sleep(Duration::from_secs(n as u64))
}

pub fn get_post_full_link(link: &str) -> String {
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

pub fn has_classes(e: scraper::ElementRef, classes: &[&str]) -> bool {
    classes.iter().all(|class| {
        e.value()
            .has_class(class, scraper::CaseSensitivity::AsciiCaseInsensitive)
    })
}

pub fn get_user_link(username: &str) -> String {
    format!("https://twitter.com/{username}")
}
