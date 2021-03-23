use anyhow::{Context, Result};
use libretranslate::Language;
use crate::utils::messages::{handle_errors, with_target};

pub(crate) async fn handle_command(query: Vec<&str>, mb_target: Option<&str>) -> Option<String> {
    Some(handle_errors(async {
        let def = get_definition(&query).await?;
        let translation = translate(def).await?;
        let result = format!("{}: {}", query.join(" "), translation);
        Ok(with_target(&result, &mb_target))
    }.await))
}

async fn get_definition(query: &Vec<&str>) -> Result<String> {
    let client = reqwest::Client::new();
    let html_body = client
        .get("https://www.urbandictionary.com/define.php")
        .query(&[("term", query.join(" "))])
        .send()
        .await?
        .text()
        .await?;

    use scraper::{Html, Selector};
    let parsed_body = Html::parse_document(&html_body);
    let sel = Selector::parse(".meaning").unwrap();

    let mut meanings = parsed_body.select(&sel);
    let first_meaning = meanings.next().with_context(|| "No meaning found")?;
    let def = first_meaning.text().collect::<Vec<_>>().join("");
    Ok(def)
}

// get ownership of the input text. While not ideal, it solves the problem of
// having only 'static tasks. Tokio doesn't have (yet?) the ability
// to spawn scoped tasks.
async fn translate(text: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let source = Language::English;
        let target = Language::French;
        let data = libretranslate::translate(Some(source), target, &text)?;
        println!(
            "translated into {}: {}",
            data.target.as_pretty(),
            data.output
        );
        Ok(data.output)
    }).await?
}
