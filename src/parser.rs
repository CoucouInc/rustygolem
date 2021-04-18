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
use nom::{
    character::complete::{alphanumeric1, char, multispace0, multispace1},
    combinator::recognize,
    multi::many1,
};
use nom::{error::ParseError, sequence::pair};
use nom::{sequence::terminated, Finish};

use crate::crypto::CryptoCoin;

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
    Joke(Option<&'input str>),
    Crypto(Result<CryptoCoin, &'input str>, Option<&'input str>),
    Urbain(Vec<&'input str>, Option<&'input str>),
    Other(&'input str),
}

pub fn parse_command<'input>(
    input: &'input str,
) -> std::result::Result<CoucouCmd<'input>, Error<&str>> {
    all_consuming(terminated(
        alt((ctcp, date, joke, crypto, urbain, other)),
        multispace0,
    ))(input)
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

fn joke(input: &str) -> IResult<&str, CoucouCmd> {
    preceded(
        command_prefix,
        map(with_target(tag("joke")), |(_, t)| CoucouCmd::Joke(t)),
    )(input)
}

fn crypto(input: &str) -> IResult<&str, CoucouCmd> {
    preceded(
        command_prefix,
        map(
            with_target(tuple((tag("crypto"), multispace1, crypto_cmd))),
            |((_, _, c), t)| CoucouCmd::Crypto(c, t),
        ),
    )(input)
}

fn crypto_cmd(input: &str) -> IResult<&str, Result<CryptoCoin, &str>> {
    alt((
        map(tag("xbt"), |_| Ok(CryptoCoin::Bitcoin)),
        map(tag("btc"), |_| Ok(CryptoCoin::Bitcoin)),
        map(tag("eth"), |_| Ok(CryptoCoin::Ethereum)),
        map(tag("doge"), |_| Ok(CryptoCoin::Doge)),
        map(tag("xrp"), |_| Ok(CryptoCoin::Ripple)),
        map(word, |w| Err(w)),
    ))(input)
}

fn urbain(input: &str) -> IResult<&str, CoucouCmd> {
    preceded(
        command_prefix,
        map(
            with_target(tuple((tag("urbain"), many1(preceded(multispace1, word))))),
            |((_, query), t)| CoucouCmd::Urbain(query, t),
        ),
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
    alt((is_a("&"), is_a("λ")))(input)
}

fn with_target<'a, O, F: 'a, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, (O, Option<&'a str>), E>
where
    F: FnMut(&'a str) -> IResult<&'a str, O, E>,
{
    pair(inner, opt(target))
}

fn target<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    let target_sep = delimited(multispace0, char('>'), multispace1);
    map(tuple((target_sep, word, multispace0)), |(_, n, _)| n)(input)
}

fn word<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    recognize(many1(alphanumeric1))(input)
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    async fn test_target() {
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
    async fn test_ctcp() {
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

    #[test]
    async fn test_date() {
        assert_eq!(
            parse_command("λdate"),
            Ok(CoucouCmd::Date(None)),
            "date with no target"
        );
        assert_eq!(
            parse_command("λdate > charlie"),
            Ok(CoucouCmd::Date(Some("charlie"))),
            "date with target"
        );
    }

    #[test]
    async fn test_joke() {
        assert_eq!(
            parse_command("λjoke"),
            Ok(CoucouCmd::Joke(None)),
            "joke with no target"
        );
        assert_eq!(
            parse_command("λjoke > charlie"),
            Ok(CoucouCmd::Joke(Some("charlie"))),
            "joke with target"
        );
    }

    #[test]
    async fn test_crypto() {
        assert_eq!(
            parse_command("λcrypto"),
            Ok(CoucouCmd::Other("λcrypto")),
            "must have something after the command"
        );
        assert_eq!(
            parse_command("λcrypto > charlie"),
            Ok(CoucouCmd::Other("λcrypto > charlie")),
            "must have a currency"
        );
        assert_eq!(
            parse_command("λcrypto lol > charlie"),
            Ok(CoucouCmd::Crypto(Err("lol"), Some("charlie"))),
            "unknown currency"
        );
        assert_eq!(
            parse_command("λcrypto xbt > charlie"),
            Ok(CoucouCmd::Crypto(Ok(CryptoCoin::Bitcoin), Some("charlie"))),
            "known currency with target"
        );
        assert_eq!(
            parse_command("λcrypto xbt "),
            Ok(CoucouCmd::Crypto(Ok(CryptoCoin::Bitcoin), None)),
            "known currency without target"
        );

        assert_eq!(
            parse_command("λurbain coucou"),
            Ok(CoucouCmd::Urbain(vec!["coucou"], None)),
            "urbain with single word query"
        );

        assert_eq!(
            parse_command("λurbain coucou > target"),
            Ok(CoucouCmd::Urbain(vec!["coucou"], Some("target"))),
            "urbain with single word query and target"
        );

        assert_eq!(
            parse_command("λurbain coucou and some"),
            Ok(CoucouCmd::Urbain(vec!["coucou", "and", "some"], None)),
            "urbain with multiple words query"
        );

        assert_eq!(
            parse_command("λurbain coucou and some > target"),
            Ok(CoucouCmd::Urbain(
                vec!["coucou", "and", "some"],
                Some("target")
            )),
            "urbain with multiple words query and a target"
        );
    }
}
