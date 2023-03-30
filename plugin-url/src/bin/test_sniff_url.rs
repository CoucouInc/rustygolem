use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let resp = reqwest::get("https://apnews.com/article/greta-thunberg-german-mine-protest-a870ba0ba69c7816cc04f13b8be2cb94")
        .await?;
    let res = plugin_url::sniff_title(resp).await?;
    println!("mb title is: {res}");

    // let url = "mock url";
    // let tmp = include_str!("coucou.tmp");
    // let selector = scraper::Selector::parse("title").unwrap();
    // // there can be a problem since `<title>coucou` is parsed as the
    // // full title. So need to grab enough bytes from the network
    // // to be reasonably sure that we got the full title
    // // Also, ignore any parse error. The parser is very lenient and can
    // // gives us a title even if there are other error in the document
    // let res = if let Some(title) = scraper::Html::parse_document(tmp)
    //     .select(&selector)
    //     .next()
    // {
    //     log::debug!("found title: {title:?}");
    //     let title = title
    //         .text()
    //         .into_iter()
    //         .collect::<String>()
    //         .replace('\n', " ");
    //
    //     // Simply slicing the string like title[..100] will panic if
    //     // it stops across an utf-8 codepoint boundary.
    //     // So need to iterate across real chars to split properly.
    //     let char_len = title.chars().count();
    //     if char_len > 100 {
    //         let f = title.chars().take(100).collect::<String>();
    //         format!("{}[…] [{url}]", f)
    //     } else {
    //         format!("{title} [{url}]")
    //     }
    // } else {
    //     format!("No title found at {url}")
    // };
    //
    // println!("res: {res:?}");

    Ok(())
}
