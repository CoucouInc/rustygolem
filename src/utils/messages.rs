use anyhow::Result;

pub fn with_target(msg: &str, mb_target: &Option<&str>) -> String {
    let target = mb_target
        .map(|t| format!("{}: ", t))
        .unwrap_or("".to_string());
    format!("{}{}", target, msg)
}

pub fn handle_errors(res: Result<String>) -> String {
    match res {
        Ok(s) => s,
        Err(err) => format!("{}", err),
    }
}
