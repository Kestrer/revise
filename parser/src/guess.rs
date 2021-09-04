use std::collections::BTreeSet;

/// Parse a guess for the definitions of a term.
#[allow(clippy::module_name_repetitions)]
#[must_use]
pub fn parse_guess(input: &str) -> BTreeSet<String> {
    let mut cx = ParseContext { remaining: input };

    let guess = parse_guess_inner(&mut cx);

    if !cx.remaining.is_empty() {
        panic!("Trailing characters: {:?}", cx.remaining);
    }

    guess
}

struct ParseContext<'a> {
    remaining: &'a str,
}

impl ParseContext<'_> {
    fn try_parse<R, F>(&mut self, f: F) -> Result<R, NoMatch>
    where
        F: FnOnce(&mut ParseContext<'_>) -> Result<R, NoMatch>,
    {
        let prev_remaining = self.remaining;
        let res = f(self);
        if res.is_err() {
            self.remaining = prev_remaining;
        }
        res
    }
}

struct NoMatch;

fn parse_guess_inner(cx: &mut ParseContext<'_>) -> BTreeSet<String> {
    let mut options = BTreeSet::new();

    loop {
        while parse_whitespace(cx).is_ok() {}

        if let Ok(option) = parse_option(cx) {
            if !option.is_empty() {
                options.insert(option);
            }
            while parse_whitespace(cx).is_ok() {}
        }

        if parse_exact_char(cx, ',').is_err() {
            break;
        }
    }

    options
}

fn parse_option(cx: &mut ParseContext<'_>) -> Result<String, NoMatch> {
    parse_quoted(cx).or_else(|NoMatch| {
        let mut value = String::new();

        value.push(parse_option_atom(cx)?);

        loop {
            let old_value_len = value.len();

            let res = cx.try_parse(|cx| {
                while let Ok(c) = parse_whitespace(cx) {
                    value.push(c);
                }
                value.push(parse_option_atom(cx)?);
                Ok(())
            });
            if res.is_err() {
                value.truncate(old_value_len);
                break;
            }
        }

        Ok(value)
    })
}

fn parse_option_atom(cx: &mut ParseContext<'_>) -> Result<char, NoMatch> {
    cx.try_parse(|cx| {
        parse_any(cx)
            .ok()
            .filter(|&c| c != ',' && !c.is_whitespace())
            .ok_or(NoMatch)
    })
}

fn parse_quoted(cx: &mut ParseContext<'_>) -> Result<String, NoMatch> {
    parse_exact_char(cx, '"')?;

    let mut value = String::new();

    loop {
        match parse_any(cx) {
            Ok('\\') => {
                if let Ok(c) = parse_any(cx) {
                    value.push(c);
                }
            }
            Ok('"') | Err(NoMatch) => break,
            Ok(c) => value.push(c),
        }
    }

    Ok(value)
}

fn parse_whitespace(cx: &mut ParseContext<'_>) -> Result<char, NoMatch> {
    cx.try_parse(|cx| {
        parse_any(cx)
            .ok()
            .filter(|&c| c.is_whitespace())
            .ok_or(NoMatch)
    })
}

fn parse_any(cx: &mut ParseContext<'_>) -> Result<char, NoMatch> {
    let mut chars = cx.remaining.chars();
    let c = chars.next().ok_or(NoMatch)?;
    cx.remaining = chars.as_str();
    Ok(c)
}

fn parse_exact_char(cx: &mut ParseContext<'_>, expected: char) -> Result<(), NoMatch> {
    cx.try_parse(|cx| {
        if parse_any(cx)? == expected {
            Ok(())
        } else {
            Err(NoMatch)
        }
    })
}

#[test]
fn test() {
    macro_rules! guess {
        ($($option:literal),* $(,)?) => {
            maplit::btreeset!($($option.to_owned(),)*)
        }
    }

    assert_eq!(parse_guess(""), guess!());
    assert_eq!(parse_guess(" "), guess!());
    assert_eq!(parse_guess(",,,,,,"), guess!());
    assert_eq!(parse_guess("a, \"\" ,,b,,,"), guess!("a", "b"));
    assert_eq!(parse_guess(" foo "), guess!("foo"));
    assert_eq!(parse_guess("a,b"), guess!("a", "b"));
    assert_eq!(parse_guess("\"a,b\""), guess!("a,b"));
    assert_eq!(parse_guess("\"\\\"\\\\\""), guess!("\"\\"));
    assert_eq!(parse_guess(" - - , -- -- "), guess!("- -", "-- --"));
    assert_eq!(parse_guess("a\",b\"\""), guess!("a\"", "b\"\""));
}
