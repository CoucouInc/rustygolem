#![allow(dead_code)]

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ts = "2022-01-29T19:10:29Z";
    let parse_res = time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339);
    println!("{parse_res:?}");
    let d = parse_res?;

    let format = time::macros::format_description!("[hour]:[minute] [period]");
    let final_str = d.format(format);
    println!("{final_str:?}");

    Ok(())
}
