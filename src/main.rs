use color_eyre::{
    eyre::{bail, eyre, Context, Result},
    Report,
};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod client;
mod config;
mod driver_pool;
mod fetch;
mod utils;

use config::Config;
use driver_pool::DriverPool;

use crate::fetch::users::{get_user_info, get_users_from_following};
use crate::utils::get_user_link;

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
                        warn!("Encountered error while fetching user info for {user}: {e:#}");
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
            error!("Task encountered an error: {e:#}",);
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
