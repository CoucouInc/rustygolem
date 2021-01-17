pub fn with_target(msg: &str, mb_target: &Option<&str>) -> String {
    let target = mb_target.map(|t| format!("{}: ", t)).unwrap_or("".to_string());
    format!("{}{}", target, msg)
}
