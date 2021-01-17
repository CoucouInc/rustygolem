use crate::utils::messages::with_target;

pub(crate) async fn handle_command(mb_target: Option<&str>) -> Option<String> {
    let req = reqwest::Client::new()
        .get("https://icanhazdadjoke.com")
        .header("Accept", "text/plain");
    let resp = match req.send().await {
        Ok(r) => r,
        Err(err) => {
            return Some(format!(
                "Error while querying icanhazdadjoke API: {:?}",
                err
            ))
        }
    };

    let joke = match resp.text().await {
        Ok(t) => t,
        Err(err) => {
            return Some(format!(
                "Error while getting the response from icanhazdadjoke: {:?}",
                err
            ))
        }
    };

    Some(with_target(&format!("{}", joke), &mb_target))
}
