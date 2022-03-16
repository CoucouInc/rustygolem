use nom::{
    bytes::complete::take_till,
    character::complete::{multispace0, multispace1},
    combinator::{map, opt},
    error::ParseError,
    sequence::{delimited, pair, tuple},
    IResult,
};

pub(crate) fn with_target<'a, O, F: 'a, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, (O, Option<&'a str>), E>
where
    F: FnMut(&'a str) -> IResult<&'a str, O, E>,
{
    pair(inner, opt(target))
}

pub fn target<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    let target_sep = delimited(
        multispace1,
        nom::character::complete::char('>'),
        multispace1,
    );
    map(tuple((target_sep, word, multispace0)), |(_, n, _)| n)(input)
}

pub fn word<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    take_till(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r')(input)
}

/// Utility to parse common command prefix
pub(crate) fn command_prefix(input: &str) -> nom::IResult<&str, &str> {
    nom::branch::alt((
        nom::bytes::complete::is_a("&"),
        nom::bytes::complete::is_a("λ"),
    ))(input)
}

#[cfg(test)]
mod test {
    use super::*;
    use nom::Finish;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_word() {
        let r: Result<_, nom::error::VerboseError<_>> = word("coucou").finish();
        assert_eq!(r, Ok(("", "coucou")));
    }
}
