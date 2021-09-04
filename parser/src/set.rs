use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::str;

/// Parse a `.set` file.
///
/// # Errors
///
/// Fails with a list of all the errors if the set is not a valid set file.
#[allow(clippy::module_name_repetitions)]
pub fn parse_set(input: &str) -> Result<Set, Vec<ParseError>> {
    let mut errors = Vec::new();
    let mut cx = ParseContext {
        source: input,
        remaining: input,
        errors: &mut errors,
    };

    let set = parse_set_inner(&mut cx);

    if !cx.remaining.is_empty() {
        panic!("Trailing characters: {:?}", cx.remaining);
    }

    if errors.is_empty() {
        Ok(set)
    } else {
        Err(errors)
    }
}

struct ParseContext<'a, 'e> {
    source: &'a str,
    remaining: &'a str,
    errors: &'e mut Vec<ParseError>,
}

impl<'a> ParseContext<'a, '_> {
    fn offset(&self) -> usize {
        let source = <*const str>::cast::<*const ()>(self.source) as usize;
        let s = <*const str>::cast::<*const ()>(self.remaining) as usize;
        let offset = s.checked_sub(source).unwrap();
        assert!(offset <= self.source.len());
        offset
    }
    fn try_parse<R, F>(&mut self, f: F) -> Result<R, NoMatch>
    where
        F: FnOnce(&mut ParseContext<'_, '_>) -> Result<R, NoMatch>,
    {
        let prev_remaining = self.remaining;
        let prev_errors = self.errors.len();
        let res = f(self);
        if res.is_err() {
            self.remaining = prev_remaining;
            self.errors.truncate(prev_errors);
        }
        res
    }
}

struct NoMatch;

fn parse_set_inner(cx: &mut ParseContext<'_, '_>) -> Set {
    while cx
        .try_parse(|cx| {
            parse_blank_line(cx);
            parse_newline(cx)
        })
        .is_ok()
    {}

    let title = parse_title(cx);

    let mut cards = HashMap::new();

    while parse_newline(cx).is_ok() {
        let card_start = cx.offset();
        if let Ok(card) = parse_card(cx) {
            if let Some(original) = cards.get(&card).cloned() {
                cx.errors.push(ParseError::DuplicateCard {
                    original,
                    duplicate: card_start..cx.offset(),
                });
            } else {
                cards.insert(card, card_start..cx.offset());
            }
        } else {
            parse_blank_line(cx);
        }
    }

    if cards.is_empty() {
        cx.errors.push(ParseError::EmptySet);
    }

    Set {
        title,
        cards: cards.into_keys().collect(),
    }
}

#[test]
fn test_parse_set() {
    use maplit::hashset;

    let parse = |input| {
        let (set, remaining, errors) = run_parser(|cx| Ok(parse_set_inner(cx)), input).unwrap();
        assert_eq!(remaining, "");
        (set, errors)
    };

    assert_eq!(
        parse("title\na,b - c\n\na,b - c"),
        (
            Set {
                title: "title".to_owned(),
                cards: hashset!(card!("a", "b" - "c")),
            },
            vec![duplicate_card(6..13, 15..22)]
        )
    );
    assert_eq!(
        parse(" -- \r\n\r\n \" , - , \" - \" , - , \" "),
        (
            Set {
                title: "--".to_owned(),
                cards: hashset!(card!(" , - , " - " , - , ")),
            },
            vec![],
        )
    );
    assert_eq!(
        parse("x\n\r\n\n\n  \n\r\n"),
        (
            Set {
                title: "x".to_owned(),
                cards: hashset!(),
            },
            vec![empty_set()],
        )
    );
}

fn parse_blank_line(cx: &mut ParseContext<'_, '_>) {
    while parse_ws(cx).is_ok() {}
    parse_comment(cx);
}

fn parse_title(cx: &mut ParseContext<'_, '_>) -> String {
    let title_line_start = cx.offset();

    let parse_title_char = |cx: &mut ParseContext<'_, '_>| {
        cx.try_parse(|cx| {
            parse_character(cx)
                .ok()
                .filter(|&c| c != '#')
                .ok_or(NoMatch)
        })
    };

    let mut title = String::new();

    while let Ok(c) = parse_title_char(cx) {
        title.push(c);
    }

    let title = title.trim().to_owned();

    if title.is_empty() {
        cx.errors.push(ParseError::NoTitle {
            line: title_line_start..cx.offset(),
        });
    }

    parse_comment(cx);

    title
}

#[test]
fn test_parse_title() {
    let parse = |input| run_parser(|cx| Ok(parse_title(cx)), input).unwrap();

    assert_eq!(parse(""), ("".into(), "", vec![no_title(0..0)]));
    assert_eq!(parse(" "), ("".into(), "", vec![no_title(0..1)]));
    assert_eq!(parse("  #foo"), ("".into(), "", vec![no_title(0..2)]));
    assert_eq!(parse("  #foo\n"), ("".into(), "\n", vec![no_title(0..2)]));
    assert_eq!(parse("x\r\n"), ("x".into(), "\r\n", vec![]));
    assert_eq!(parse("   title  "), ("title".into(), "", vec![]));
}

fn parse_card(cx: &mut ParseContext<'_, '_>) -> Result<Card, NoMatch> {
    let card_start = cx.offset();

    let (mut space_before_dash, mut space_after_dash) = (false, false);

    let (terms, has_dash) = cx.try_parse(|cx| {
        let options = cx.try_parse(|cx| {
            while parse_ws(cx).is_ok() {}
            parse_options(cx)
        });

        while parse_ws(cx).is_ok() {
            space_before_dash = true;
        }

        let has_dash = parse_exact_char(cx, '-').is_ok();
        if !has_dash && options.is_err() {
            return Err(NoMatch);
        }
        Ok((options.unwrap_or_default(), has_dash))
    })?;

    let definitions = if has_dash {
        let dash_span = cx.offset() - '-'.len_utf8()..cx.offset();

        while parse_ws(cx).is_ok() {
            space_after_dash = true;
        }

        if !space_before_dash || !space_after_dash {
            cx.errors
                .push(ParseError::MissingWhitespaceAroundDash { dash: dash_span });
        }

        let definitions = parse_options(cx);
        if definitions.is_ok() {
            while parse_ws(cx).is_ok() {}
        }

        let third_part_start = cx.offset();
        if parse_exact_char(cx, '-').is_ok() {
            while cx
                .try_parse(|cx| {
                    parse_character(cx)
                        .ok()
                        .filter(|&c| c != '#')
                        .ok_or(NoMatch)
                })
                .is_ok()
            {}

            cx.errors.push(ParseError::ThirdPart {
                before: card_start..third_part_start,
                span: third_part_start..cx.offset(),
            });
        }

        definitions.unwrap_or_default()
    } else {
        BTreeSet::new()
    };

    if terms.is_empty() {
        cx.errors.push(ParseError::NoTerms {
            card: card_start..cx.offset(),
        });
    }
    if definitions.is_empty() {
        cx.errors.push(ParseError::NoDefinitions {
            card: card_start..cx.offset(),
        });
    }

    parse_comment(cx);

    Ok(Card { terms, definitions })
}

#[test]
fn test_parse_card() {
    let parse = |input| run_parser(parse_card, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse("  "), None);
    assert_eq!(parse("\t\t\t\r\n"), None);
    assert_eq!(parse("t - d"), Some((card!("t" - "d"), "", vec![])));
    assert_eq!(
        parse("t1, t2 - d\n"),
        Some((card!("t1", "t2" - "d"), "\n", vec![]))
    );
    assert_eq!(
        parse("t - d1 ,d2#c"),
        Some((card!("t" - "d1", "d2"), "", vec![]))
    );
    assert_eq!(
        parse("t1,t2 - d1 , d2#comment\r\nfoo"),
        Some((card!("t1", "t2" - "d1", "d2"), "\r\nfoo", vec![]))
    );
    assert_eq!(
        parse(" - xyz"),
        Some((card!(-"xyz"), "", vec![no_terms(0..6)]))
    );
    assert_eq!(
        parse("xyz - "),
        Some((card!("xyz" -), "", vec![no_definitions(0..6)]))
    );
    assert_eq!(
        parse("-"),
        Some((
            card!(-),
            "",
            vec![missing_dash_ws(0..1), no_terms(0..1), no_definitions(0..1)]
        ))
    );
    assert_eq!(
        parse("foo-bar   -   baz-quux"),
        Some((card!("foo-bar" - "baz-quux"), "", vec![])),
    );
    assert_eq!(
        parse("a  -b"),
        Some((card!("a" - "b"), "", vec![missing_dash_ws(3..4)])),
    );
    assert_eq!(
        parse("a-  b"),
        Some((card!("a" - "b"), "", vec![missing_dash_ws(1..2)])),
    );
    assert_eq!(
        parse("a - b - c - d"),
        Some((card!("a" - "b"), "", vec![third_part(0..6, 6..13)])),
    );
}

fn parse_options(cx: &mut ParseContext<'_, '_>) -> Result<BTreeSet<String>, NoMatch> {
    let mut options = <BTreeMap<String, Range<usize>>>::new();
    let mut add_option = |cx: &mut ParseContext<'_, '_>, option: String, span| {
        if option.is_empty() {
            cx.errors.push(ParseError::EmptyOption { span });
        } else if let Some(original) = options.get(&option) {
            cx.errors.push(ParseError::DuplicateOption {
                original: original.clone(),
                duplicate: span,
            });
        } else {
            options.insert(option, span);
        }
    };

    let mut option_start = cx.offset();
    let mut already_parsed_comma = false;

    if parse_exact_char(cx, ',').is_ok() {
        cx.errors.push(ParseError::EmptyOption {
            span: option_start..cx.offset(),
        });
        already_parsed_comma = true;
    } else {
        let option = parse_option(cx)?;
        add_option(cx, option, option_start..cx.offset());
    };

    loop {
        if !already_parsed_comma {
            let comma_res = cx.try_parse(|cx| {
                while parse_ws(cx).is_ok() {}
                option_start = cx.offset();
                parse_exact_char(cx, ',')
            });
            if comma_res.is_err() {
                break;
            }
        }
        already_parsed_comma = false;

        let mut real_option_start = usize::MAX;

        let res = cx.try_parse(|cx| {
            while parse_ws(cx).is_ok() {}
            real_option_start = cx.offset();
            let option = parse_option(cx)?;
            add_option(cx, option, real_option_start..cx.offset());
            Ok(())
        });
        if res.is_err() {
            cx.errors.push(ParseError::EmptyOption {
                span: option_start..if cx.source[real_option_start..].starts_with(',') {
                    real_option_start + 1
                } else {
                    real_option_start
                },
            });
        }
    }

    Ok(options.into_keys().collect())
}

#[test]
fn test_parse_options() {
    let parse = |input| run_parser(parse_options, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse(" x"), None);
    assert_eq!(
        parse("the option"),
        Some((options!("the option"), "", Vec::new()))
    );
    assert_eq!(
        parse("a,b,c#"),
        Some((options!("a", "b", "c"), "#", Vec::new()))
    );
    assert_eq!(
        parse("a  ,  b ,, c"),
        Some((options!("a", "b", "c"), "", vec![empty_option(8..10)]))
    );
    assert_eq!(
        parse(",   , "),
        Some((
            options!(),
            " ",
            vec![empty_option(0..1), empty_option(0..5), empty_option(4..6)]
        ))
    );
    assert_eq!(
        parse("a,a"),
        Some((options!("a"), "", vec![duplicate_option(0..1, 2..3)]))
    );
    assert_eq!(
        parse("\"\""),
        Some((options!(), "", vec![empty_option(0..2)]))
    );
}

fn parse_option(cx: &mut ParseContext<'_, '_>) -> Result<String, NoMatch> {
    let (mut value, after_quote) = match parse_quoted(cx) {
        Ok(quoted) => (quoted, Some(cx.offset())),
        Err(NoMatch) => (String::from(parse_option_atom(cx)?), None),
    };

    let mut trailing_atoms = false;

    loop {
        let old_value_len = value.len();

        let res = cx.try_parse(|cx| {
            if cx.remaining.starts_with('-') {
                while parse_exact_char(cx, '-').is_ok() {
                    value.push('-');
                }
            } else {
                while let Ok(c) = parse_option_ws(cx) {
                    value.push(c);
                }
            }

            value.push(parse_option_atom(cx)?);

            Ok(())
        });
        if res.is_err() {
            value.truncate(old_value_len);
            break;
        }
        trailing_atoms = true;
    }

    if let (true, Some(after_quote)) = (trailing_atoms, after_quote) {
        cx.errors.push(ParseError::TrailingOptionChars {
            span: after_quote..cx.offset(),
        });
    }

    Ok(value)
}

#[test]
fn test_parse_option() {
    let parse = |input| run_parser(parse_option, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse("   foo"), None);
    assert_eq!(
        parse("a\u{2028}"),
        Some(("a".into(), "\u{2028}", Vec::new()))
    );
    assert_eq!(
        parse("a \u{1680}b  "),
        Some(("a \u{1680}b".into(), "  ", Vec::new()))
    );
    assert_eq!(parse("\"---\"--"), Some(("---".into(), "--", Vec::new())));
    assert_eq!(parse("a---b---"), Some(("a---b".into(), "---", Vec::new())));
    assert_eq!(
        parse("a\u{85}b"),
        Some((
            "a\u{85}b".into(),
            "",
            vec![unexpected_control_char('\u{85}', 1..3)]
        ))
    );
    assert_eq!(parse("a\"\""), Some(("a\"\"".into(), "", Vec::new())));
    assert_eq!(
        parse("\"a\"bc\n"),
        Some(("abc".into(), "\n", vec![trailing_option_chars(3..5)]))
    );
}

fn parse_option_atom(cx: &mut ParseContext<'_, '_>) -> Result<char, NoMatch> {
    cx.try_parse(|cx| {
        parse_character(cx)
            .ok()
            .filter(|&c| c != ',' && c != '-' && c != '#' && !c.is_whitespace())
            .ok_or(NoMatch)
    })
}

#[test]
fn test_parse_option_atom() {
    let parse = |input| run_parser(parse_option_atom, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse(","), None);
    assert_eq!(parse("-"), None);
    assert_eq!(parse("#"), None);
    assert_eq!(parse(" "), None);
    assert_eq!(parse("^"), Some(('^', "", Vec::new())));
    assert_eq!(parse("qq"), Some(('q', "q", Vec::new())));
}

fn parse_option_ws(cx: &mut ParseContext<'_, '_>) -> Result<char, NoMatch> {
    let ws = cx.try_parse(|cx| {
        parse_any(cx)
            .ok()
            .filter(|&c| c.is_whitespace() && c != '\r' && c != '\n')
            .ok_or(NoMatch)
    })?;
    if ws.is_control() {
        cx.errors.push(ParseError::UnexpectedControlChar {
            character: ws,
            span: cx.offset() - ws.len_utf8()..cx.offset(),
        });
    }
    Ok(ws)
}

#[test]
fn test_parse_option_ws() {
    let parse = |input| run_parser(parse_option_ws, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse("X"), None);
    assert_eq!(parse("\r"), None);
    assert_eq!(parse("\n"), None);
    assert_eq!(parse(" "), Some((' ', "", Vec::new())));
    assert_eq!(parse("\u{2003}abc"), Some(('\u{2003}', "abc", Vec::new())));
    assert_eq!(
        parse("\u{85}abc"),
        Some((
            '\u{85}',
            "abc",
            vec![unexpected_control_char('\u{85}', 0..2)]
        ))
    );
}

fn parse_quoted(cx: &mut ParseContext<'_, '_>) -> Result<String, NoMatch> {
    let mut value = String::new();

    let string_start = cx.offset();

    parse_exact_char(cx, '"')?;

    loop {
        match parse_character(cx) {
            Ok('\\') => {
                let escape_offset = cx.offset();

                match parse_any(cx) {
                    Ok(c @ ('"' | '\\')) => value.push(c),
                    Ok(escape) => {
                        cx.errors.push(ParseError::UnknownEscape {
                            escape,
                            span: escape_offset..cx.offset(),
                        });
                    }
                    Err(NoMatch) => {}
                }
            }
            Ok('"') => break,
            Ok(c) => value.push(c),
            Err(NoMatch) => {
                cx.errors.push(ParseError::UnclosedQuote {
                    span: string_start..cx.offset(),
                });
                break;
            }
        }
    }

    Ok(value)
}

#[test]
fn test_parse_quoted() {
    let s = |s: &str| s.to_owned();
    let parse = |input| run_parser(parse_quoted, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse("'"), None);
    assert_eq!(parse(" \""), None);
    assert_eq!(parse(r#""""#), Some((s(""), "", Vec::new())));
    assert_eq!(parse(r#"""abc"#), Some((s(""), "abc", Vec::new())));
    assert_eq!(parse(r#""ab""cd"#), Some((s("ab"), "\"cd", Vec::new())));
    assert_eq!(parse(r#""'''""#), Some((s("'''"), "", Vec::new())));
    assert_eq!(parse(r#""a\\b""#), Some((s("a\\b"), "", Vec::new())));
    assert_eq!(parse(r#""\"""#), Some((s("\""), "", Vec::new())));
    assert_eq!(parse(r#""\\\"""#), Some((s("\\\""), "", Vec::new())));
    assert_eq!(parse(r#"""#), Some((s(""), "", vec![unclosed_quote(0..1)])));
    assert_eq!(
        parse(r#""abc"#),
        Some((s("abc"), "", vec![unclosed_quote(0..4)]))
    );
    assert_eq!(
        parse(r#""a\"#),
        Some((s("a"), "", vec![unclosed_quote(0..3)]))
    );
    assert_eq!(
        parse(r#""\""#),
        Some((s("\""), "", vec![unclosed_quote(0..3)]))
    );
    assert_eq!(
        parse("\"x\n"),
        Some((s("x"), "\n", vec![unclosed_quote(0..2)]))
    );
    assert_eq!(
        parse("\"\r\n"),
        Some((s(""), "\r\n", vec![unclosed_quote(0..1)]))
    );
    assert_eq!(
        parse(r#""\'\/\\""#),
        Some((
            s("\\"),
            "",
            vec![unknown_escape('\'', 2..3), unknown_escape('/', 4..5)]
        ))
    );
    assert_eq!(
        parse("\"\\\n\""),
        Some((s(""), "", vec![unknown_escape('\n', 2..3)]))
    );
    assert_eq!(
        parse("\"\\\r\n\""),
        Some((
            s(""),
            "\n\"",
            vec![unknown_escape('\r', 2..3), unclosed_quote(0..3)]
        ))
    );
}

fn parse_comment(cx: &mut ParseContext<'_, '_>) {
    if parse_exact_char(cx, '#').is_ok() {
        while parse_character(cx).is_ok() {}
    }
}

#[test]
fn test_parse_comment() {
    let parse = |input| {
        let ((), rest, errors) = run_parser(
            |cx| {
                parse_comment(cx);
                Ok(())
            },
            input,
        )
        .unwrap();
        (rest, errors)
    };

    assert_eq!(parse(""), ("", Vec::new()));
    assert_eq!(parse("abc"), ("abc", Vec::new()));
    assert_eq!(parse("#"), ("", Vec::new()));
    assert_eq!(parse("#abc"), ("", Vec::new()));
    assert_eq!(parse("#abc\r\ndef"), ("\r\ndef", Vec::new()));
    assert_eq!(parse("\n#abc"), ("\n#abc", Vec::new()));
    assert_eq!(
        parse("#a\u{0}b"),
        ("", vec![unexpected_control_char('\u{0}', 2..3)])
    );
}

fn parse_character(cx: &mut ParseContext<'_, '_>) -> Result<char, NoMatch> {
    let character = cx.try_parse(|cx| {
        parse_any(cx)
            .ok()
            .filter(|&c| c != '\r' && c != '\n')
            .ok_or(NoMatch)
    })?;

    if character.is_control() {
        cx.errors.push(ParseError::UnexpectedControlChar {
            character,
            span: cx.offset() - character.len_utf8()..cx.offset(),
        });
    }

    Ok(character)
}

#[test]
fn test_parse_character() {
    let parse = |input| run_parser(parse_character, input);

    assert_eq!(parse(""), None);
    assert_eq!(parse("\r"), None);
    assert_eq!(parse("\n\n"), None);

    assert_eq!(parse("a"), Some(('a', "", Vec::new())));
    assert_eq!(parse("abc"), Some(('a', "bc", Vec::new())));
    assert_eq!(
        parse("\t"),
        Some(('\t', "", vec![unexpected_control_char('\t', 0..1)]))
    );
    assert_eq!(
        parse("\u{85}x"),
        Some(('\u{85}', "x", vec![unexpected_control_char('\u{85}', 0..2)]))
    );
}

fn parse_ws(cx: &mut ParseContext<'_, '_>) -> Result<(), NoMatch> {
    let ws = cx.try_parse(|cx| {
        parse_any(cx)
            .ok()
            .filter(|&c| c != '\r' && c != '\n' && c.is_whitespace())
            .ok_or(NoMatch)
    })?;

    if ws != ' ' {
        cx.errors.push(ParseError::ExpectedSpace {
            character: ws,
            span: cx.offset() - ws.len_utf8()..cx.offset(),
        });
    }

    Ok(())
}

#[test]
fn test_parse_ws() {
    let parse = |input| run_parser(parse_ws, input).map(|((), rest, errors)| (rest, errors));

    assert_eq!(parse(""), None);
    assert_eq!(parse("X"), None);
    assert_eq!(parse("\r"), None);
    assert_eq!(parse("\n"), None);
    assert_eq!(parse(" "), Some(("", Vec::new())));
    assert_eq!(parse(" x"), Some(("x", Vec::new())));
    assert_eq!(parse("\tx"), Some(("x", vec![expected_space('\t', 0..1)])));
    assert_eq!(
        parse("\u{85}"),
        Some(("", vec![expected_space('\u{85}', 0..2)]))
    );
}

fn parse_newline(cx: &mut ParseContext<'_, '_>) -> Result<(), NoMatch> {
    let c = cx.try_parse(|cx| {
        let c = parse_any(cx)?;
        if c == '\r' || c == '\n' {
            Ok(c)
        } else {
            Err(NoMatch)
        }
    })?;

    if c == '\r' && parse_exact_char(cx, '\n').is_err() {
        cx.errors.push(ParseError::MissingLineFeed {
            cr_span: cx.offset() - 1..cx.offset(),
        });
    }

    Ok(())
}

#[test]
fn test_parse_newline() {
    let parse = |input| run_parser(parse_newline, input).map(|((), rest, errors)| (rest, errors));

    assert_eq!(parse(""), None);
    assert_eq!(parse(" "), None);
    assert_eq!(parse("\n"), Some(("", Vec::new())));
    assert_eq!(parse("\n\n"), Some(("\n", Vec::new())));
    assert_eq!(parse("\r\n\r\n"), Some(("\r\n", Vec::new())));
    assert_eq!(parse("\r"), Some(("", vec![missing_line_feed(0..1)])));
    assert_eq!(parse("\r\r"), Some(("\r", vec![missing_line_feed(0..1)])));
}

fn parse_any(cx: &mut ParseContext<'_, '_>) -> Result<char, NoMatch> {
    let mut chars = cx.remaining.chars();
    let c = chars.next().ok_or(NoMatch)?;
    cx.remaining = chars.as_str();
    Ok(c)
}

fn parse_exact_char(cx: &mut ParseContext<'_, '_>, expected: char) -> Result<(), NoMatch> {
    cx.try_parse(|cx| {
        if parse_any(cx)? == expected {
            Ok(())
        } else {
            Err(NoMatch)
        }
    })
}

/// A parsed `.set` file.
#[derive(Debug, PartialEq, Eq)]
pub struct Set {
    /// The title of the set.
    pub title: String,
    /// The cards in the set.
    pub cards: HashSet<Card>,
}

/// A card, consisting of some terms and some definitions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Card {
    /// The terms of the card.
    pub terms: BTreeSet<String>,
    /// Possible definitions of those terms.
    pub definitions: BTreeSet<String>,
}

/// An error parsing a `.set` file.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The set did not have a title.
    NoTitle {
        /// The span of the line that should have contained a title.
        line: Range<usize>,
    },

    /// The set did not contain any terms.
    EmptySet,

    /// A card was duplicated.
    DuplicateCard {
        /// The span of the original card.
        original: Range<usize>,
        /// The span of the duplicated card.
        duplicate: Range<usize>,
    },

    /// There was a third part to the card, caused by 2+ dashes in one line.
    ThirdPart {
        /// The span before the start of the dash.
        before: Range<usize>,
        /// The span from the start of the dash to the end of the line, or start of the comment.
        span: Range<usize>,
    },

    /// There was whitespace missing around the dash on a card.
    MissingWhitespaceAroundDash {
        /// The span of the dash.
        dash: Range<usize>,
    },

    /// No terms were provided on a card.
    NoTerms {
        /// The span of the card lacking terms.
        card: Range<usize>,
    },

    /// No definitions were provided on a card.
    NoDefinitions {
        /// The span of the card lacking definitions.
        card: Range<usize>,
    },

    /// An option was duplicated.
    DuplicateOption {
        /// The span of the original option.
        original: Range<usize>,
        /// The span of the duplicated option.
        duplicate: Range<usize>,
    },

    /// An option was empty.
    EmptyOption {
        /// The empty option's span.
        span: Range<usize>,
    },

    /// There were trailing characters after a closing quote in an option.
    TrailingOptionChars {
        /// The span of the trailing characters.
        span: Range<usize>,
    },

    /// An unknown character escape was used in a quoted section.
    UnknownEscape {
        /// The character after the backslash.
        escape: char,
        /// The span of the character after the backslash.
        span: Range<usize>,
    },

    /// A quoted section was not terminated with a closing quote.
    UnclosedQuote {
        /// The span of the entire string.
        span: Range<usize>,
    },

    /// A control character was unexpectedly found.
    UnexpectedControlChar {
        /// The control character.
        character: char,
        /// The span of the character.
        span: Range<usize>,
    },

    /// A non-space whitespace character was found.
    ExpectedSpace {
        /// The whitespace character.
        character: char,
        /// The span of the character.
        span: Range<usize>,
    },

    /// An CRLF pair was missing its LF.
    MissingLineFeed {
        /// The span of the CR.
        cr_span: Range<usize>,
    },
}

#[cfg(test)]
mod test_utils {
    use super::*;

    #[track_caller]
    pub(super) fn run_parser<'a, P, R>(
        parser: P,
        input: &'a str,
    ) -> Option<(R, &'a str, Vec<ParseError>)>
    where
        P: FnOnce(&mut ParseContext<'a, '_>) -> Result<R, NoMatch>,
    {
        let mut errors = Vec::new();
        let mut cx = ParseContext {
            source: input,
            remaining: input,
            errors: &mut errors,
        };
        if let Ok(res) = parser(&mut cx) {
            Some((res, cx.remaining, errors))
        } else {
            assert_eq!(cx.remaining, input);
            assert_eq!(errors, []);
            None
        }
    }

    macro_rules! parse_error_constructors {
        ($(fn $name:ident($($field_name:ident: $field_type:ty),*) = $variant_name:ident,)*) => {
            $(pub(crate) fn $name($($field_name: $field_type,)*) -> ParseError {
                ParseError::$variant_name { $($field_name,)* }
            })*
        };
    }
    parse_error_constructors! {
        fn no_title(line: Range<usize>) = NoTitle,
        fn empty_set() = EmptySet,
        fn duplicate_card(original: Range<usize>, duplicate: Range<usize>) = DuplicateCard,
        fn third_part(before: Range<usize>, span: Range<usize>) = ThirdPart,
        fn missing_dash_ws(dash: Range<usize>) = MissingWhitespaceAroundDash,
        fn no_terms(card: Range<usize>) = NoTerms,
        fn no_definitions(card: Range<usize>) = NoDefinitions,
        fn duplicate_option(original: Range<usize>, duplicate: Range<usize>) = DuplicateOption,
        fn empty_option(span: Range<usize>) = EmptyOption,
        fn trailing_option_chars(span: Range<usize>) = TrailingOptionChars,
        fn unknown_escape(escape: char, span: Range<usize>) = UnknownEscape,
        fn unclosed_quote(span: Range<usize>) = UnclosedQuote,
        fn unexpected_control_char(character: char, span: Range<usize>) = UnexpectedControlChar,
        fn expected_space(character: char, span: Range<usize>) = ExpectedSpace,
        fn missing_line_feed(cr_span: Range<usize>) = MissingLineFeed,
    }

    macro_rules! options {
        ($($item:literal),* $(,)?) => {
            maplit::btreeset!($($item.to_owned(),)*)
        };
    }
    pub(crate) use options;

    macro_rules! card {
        (- $($definitions:literal)*) => {
            Card { terms: options!(), definitions: options!($($definitions,)*) }
        };
        ($($terms:literal),* - $($definitions:literal),*) => {
            Card { terms: options!($($terms,)*), definitions: options!($($definitions,)*) }
        };
    }
    pub(crate) use card;
}
#[cfg(test)]
#[allow(clippy::wildcard_imports)]
use test_utils::*;
