#![allow(dead_code)]

use std::pin::Pin;
use twitch_api2::HelixClient;
// use twitch_api2::twitch_oauth2::client::Client;
use twitch_api2::{twitch_oauth2::AppAccessToken, types::UserId};

trait ATrait {
    fn run<'async_trait>(
        &'async_trait self,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'async_trait>>
    where
        Self: Sync;
}

#[derive(Default)]
struct Coucou {
    // client: TwitchClient<'static, reqwest::Client>,
    // client: C,
    // client: HelixClient<'a, reqwest::Client>,
    auth_client: reqwest::Client,
    helix_client: HelixClient<'static, reqwest::Client>,
}


impl ATrait for Coucou
where
    // for<'a> 'c: 'a,
    // for<'c> OC: twitch_api2::twitch_oauth2::client::Client<'c>,
    // C: twitch_api2::
{
    fn run<'async_trait>(
        &'async_trait self,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'async_trait>>
    where
        Self: Sync,
    {
        async fn _run(_self: &Coucou)
        where
            // for<'a> Cl: Client<'a> + twitch_api2::twitch_oauth2::client::Client<'a>,
            // for<'a> OC: twitch_api2::twitch_oauth2::client::Client<'a>,
        {
            // for _ in 0..5 {
            //     tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            //     println!("running!");
            // }

            let token = AppAccessToken::get_app_access_token(
                &_self.auth_client,
                "clientid".into(),
                "secret".into(),
                vec![],
            )
            .await
            .unwrap();

            let uid: UserId = "uid".into();
            _self.helix_client.get_user_from_id(uid, &token).await.unwrap();
            println!("done");
        }

        Box::pin(_run(self))
    }
}

// impl<C> twitch_api2

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let req_client = reqwest::Client::default();
    let helix_client = HelixClient::with_client(req_client.clone());

    let c = Coucou {
        auth_client: req_client,
        helix_client,
    };
    c.run().await;

    Ok(())
}
