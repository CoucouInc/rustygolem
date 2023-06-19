use nom::{
    bytes::complete::tag,
    character::complete::{alphanumeric1, char, multispace0, multispace1},
    combinator::{all_consuming, map, opt, recognize},
    error::ParseError,
    multi::many1,
    sequence::{delimited, pair, preceded, terminated, tuple},
    Finish, IResult,
};

pub fn with_target<'a, O, F: 'a, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, (O, Option<&'a str>), E>
where
    F: FnMut(&'a str) -> IResult<&'a str, O, E>,
{
    pair(inner, opt(target))
}

pub fn target<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    let target_sep = delimited(multispace0, char('>'), multispace1);
    map(tuple((target_sep, word, multispace0)), |(_, n, _)| n)(input)
}

pub fn word<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    recognize(many1(alphanumeric1))(input)
}

/// Utility to parse common command prefix
pub fn command_prefix(input: &str) -> nom::IResult<&str, &str> {
    nom::branch::alt((
        nom::bytes::complete::is_a("&"),
        nom::bytes::complete::is_a("λ"),
    ))(input)
}

/// Parse a single command with an optional target
/// Returns None if the parser fails
pub fn single_command<'input>(
    cmd_name: &'static str,
    input: &'input str,
) -> Option<Option<&'input str>> {
    let cmd = preceded(
        command_prefix,
        map(with_target(tag(cmd_name)), |(_, t)| Some(t)),
    );

    all_consuming(terminated(cmd, multispace0))(input)
        .finish()
        .map(|x| x.1)
        .unwrap_or_default()
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_single_command() {
        assert_eq!(
            single_command("coucou", "coucou"),
            None,
            "need the command prefix"
        );

        assert_eq!(
            single_command("coucou", "&other"),
            None,
            "only parses given word"
        );

        assert_eq!(
            single_command("coucou", "&coucou"),
            Some(None),
            "can parse single command"
        );

        assert_eq!(
            single_command("coucou", "&other > charlie"),
            None,
            "target doesn't impact given word"
        );

        assert_eq!(
            single_command("coucou", "&coucou > charlie"),
            Some(Some("charlie")),
            "also parses with target"
        );
    }
}
