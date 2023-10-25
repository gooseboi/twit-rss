use color_eyre::eyre::{Context, Result};
use fantoccini::{wd::Capabilities, Client, ClientBuilder};
use serde_json::json;
use std::{
    mem::ManuallyDrop,
    ops::Deref,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::sync::Mutex;
use tracing::debug;

use crate::client::set_auth_cookie;
use crate::config::{DriverConfig, TwitterConfig};

struct PoolValue {
    driver: Child,
    port: usize,
}

pub struct DriverPool {
    pool: Mutex<Vec<PoolValue>>,
}

impl DriverPool {
    pub fn new(config: &DriverConfig) -> Result<Self> {
        let mut pool = vec![];
        for n in 0..config.driver_count {
            let port = config.base_port + n;
            let driver = Command::new("geckodriver")
                .arg("-p")
                .arg(format!("{port}"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .wrap_err("Failed spawning geckodriver process")?;
            pool.push(PoolValue { driver, port });
        }
        let pool = Mutex::new(pool);
        // Give it some time to warm up
        std::thread::sleep(Duration::from_secs(1));
        Ok(DriverPool { pool })
    }

    pub async fn get_client(&self, config: &TwitterConfig) -> Result<Option<WrappedClient>> {
        let mut caps = Capabilities::new();
        caps.insert(
            "moz:firefoxOptions".into(),
            json!({
                "prefs": {
                    "javascript.enabled": true
                },
            }),
        );

        let mut lock = self.pool.lock().await;
        let val = lock.pop();
        drop(lock);

        match val {
            Some(val) => {
                let port = val.port;
                debug!("Returning client using port {port}");
                let client = ClientBuilder::rustls()
                    .capabilities(caps)
                    .connect(&format!("http://localhost:{port}"))
                    .await
                    .wrap_err("failed to connect to WebDriver")?;
                if let Err(e) = set_auth_cookie(&client, config).await {
                    client.close().await?;
                    let mut lock = self.pool.lock().await;
                    lock.push(val);
                    drop(lock);
                    return Err(e);
                }
                Ok(Some(WrappedClient {
                    client,
                    val: ManuallyDrop::new(val),
                    pool: &self.pool,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn close(&self) -> Result<()> {
        let mut v = self.pool.lock().await;
        for PoolValue { port: _, driver } in v.iter_mut() {
            driver.kill()?;
            driver.wait()?;
        }
        Ok(())
    }
}

pub struct WrappedClient<'a> {
    client: Client,
    val: ManuallyDrop<PoolValue>,
    pool: &'a Mutex<Vec<PoolValue>>,
}

impl<'a> WrappedClient<'a> {
    pub async fn close(mut self) -> Result<()> {
        let res = self.client.close().await.map_err(|e| e.into());
        let mut lock = self.pool.lock().await;
        let val = unsafe { ManuallyDrop::take(&mut self.val) };
        lock.push(val);
        res
    }
}

impl<'a> Deref for WrappedClient<'a> {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}
