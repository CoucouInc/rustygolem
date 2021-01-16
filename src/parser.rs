use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::flat_map,
    sequence::{preceded, tuple},
};
use nom::{bytes::complete::is_a, combinator::opt, sequence::delimited};
use nom::{
    bytes::complete::is_not,
    combinator::{all_consuming, map, rest},
    error::Error,
    IResult,
};
use nom::Finish;
use nom::{
    character::complete::{alphanumeric1, char, multispace0, multispace1},
    combinator::recognize,
    multi::many1,
};
use nom::{error::ParseError, sequence::pair};

#[derive(Debug, PartialEq)]
pub enum CTCP<'input> {
    VERSION,
    TIME,
    PING(Option<&'input str>),
}

#[derive(Debug, PartialEq)]
pub enum CoucouCmd<'input> {
    CTCP(CTCP<'input>),
    Date(Option<&'input str>),
    Other(&'input str),
}

pub fn parse_command<'input>(
    input: &'input str,
) -> std::result::Result<CoucouCmd<'input>, Error<&str>> {
    all_consuming(alt((ctcp, date, other)))(input)
        .finish()
        .map(|x| x.1)
}

fn ctcp_cmd(input: &str) -> IResult<&str, CTCP> {
    alt((
        map(tag("VERSION"), |_| CTCP::VERSION),
        map(tag("TIME"), |_| CTCP::TIME),
        map(
            pair(
                tag("PING"),
                opt(preceded(multispace1, recognize(is_not("\x01")))),
            ),
            |(_, arg)| CTCP::PING(arg),
        ),
    ))(input)
}

fn ctcp(input: &str) -> IResult<&str, CoucouCmd> {
    let c = '\u{0001}';

    let raw_parse = delimited(char(c), is_not("\x01"), char(c));
    map(
        // sketchy flat_map there, there is likely a better combinator.
        flat_map(raw_parse, move |i| move |_| ctcp_cmd(i)),
        CoucouCmd::CTCP,
    )(input)
}

fn other(input: &str) -> IResult<&str, CoucouCmd> {
    map(rest, CoucouCmd::Other)(input)
}

fn date(input: &str) -> IResult<&str, CoucouCmd> {
    preceded(
        command_prefix,
        map(with_target(tag("date")), |(_, t)| CoucouCmd::Date(t)),
    )(input)
}

fn command_prefix(input: &str) -> IResult<&str, &str> {
    is_a("ρ")(input)
}

fn with_target<'a, F: 'a, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, (&'a str, Option<&'a str>), E>
where
    F: Fn(&'a str) -> IResult<&'a str, &'a str, E>,
{
    pair(inner, opt(target))
}

fn target<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    let target_sep = delimited(multispace0, char('>'), multispace1);
    let nick = recognize(many1(alphanumeric1));
    map(tuple((target_sep, nick, multispace0)), |(_, n, _)| n)(input)
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_target() {
        // monomorphised version to help type inference
        fn _target(input: &str) -> IResult<&str, &str> {
            target(input)
        }

        assert_eq!(_target("> coucou"), Ok(("", "coucou")), "simple target");
        assert_eq!(_target("  > coucou"), Ok(("", "coucou")), "prefix spaces");
        assert_eq!(_target(">   coucou"), Ok(("", "coucou")), "spaces after >");
        assert_eq!(_target("> coucou  "), Ok(("", "coucou")), "trailing spaces");
        assert_eq!(
            _target("  >   coucou  "),
            Ok(("", "coucou")),
            "spaces everywhere"
        );
        assert!(_target(">coucou").is_err(), "need a space after the >");
    }

    #[test]
    fn test_ctcp() {
        assert_eq!(
            ctcp("\u{001}VERSION\u{001}"),
            Ok(("", CoucouCmd::CTCP(CTCP::VERSION))),
            "version"
        );

        assert_eq!(
            ctcp("\u{001}TIME\u{001}"),
            Ok(("", CoucouCmd::CTCP(CTCP::TIME))),
            "time"
        );

        assert_eq!(
            ctcp("\u{001}PING\u{001}"),
            Ok(("", CoucouCmd::CTCP(CTCP::PING(None)))),
            "ping without argument"
        );

        assert_eq!(
            ctcp("\u{001}PING 123\u{001}"),
            Ok(("", CoucouCmd::CTCP(CTCP::PING(Some("123"))))),
            "ping with argument"
        );
    }
}
