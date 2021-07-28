//! Parser for the `.set` files read by `revise`.
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::missing_panics_doc, clippy::range_plus_one)]
#![warn(missing_docs)]

use std::cmp;
use std::collections::{BTreeSet, HashSet};
use std::fmt::{self, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::str;

/// Parse a `.set` file.
///
/// # Errors
///
/// Fails with a list of all the errors if the set is not a valid set file.
pub fn parse(input: &str) -> Result<Set<'_>, Vec<ParseError>> {
    let mut errors = Vec::new();

    let mut lines = input
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'));

    let title = lines.next();
    if title.map_or(true, str::is_empty) {
        errors.push(ParseError::NoTitle(
            title.map(|title| range_of(title, input)),
        ));
    }

    if let Some(second_line) = lines.next() {
        if !second_line.is_empty() {
            errors.push(ParseError::SecondLineNotEmpty(range_of(second_line, input)));
        }
    }

    let mut cards = <HashSet<Card<'_>>>::new();
    let mut attempted_cards = false;

    for line in lines {
        if line.is_empty() {
            continue;
        }

        let card_span = range_of(line, input);

        let mut line_parts = line.splitn(3, " - ");

        let terms = parse_options(
            line_parts.next().unwrap(),
            input,
            CardSide::Term,
            &mut errors,
        );

        let definitions = if let Some(definitions) = line_parts.next() {
            definitions
        } else {
            errors.push(ParseError::NoDefinitions(card_span));
            continue;
        };

        let definitions = parse_options(definitions, input, CardSide::Definition, &mut errors);

        if let Some(third_part) = line_parts.next() {
            let offset = offset_of(third_part, input) - " - ".len();
            errors.push(ParseError::ThirdPart {
                before: offset_of(line, input)..offset,
                span: offset..offset + " - ".len() + third_part.len(),
            });
        }

        assert_eq!(line_parts.next(), None);

        if terms.is_empty() || definitions.is_empty() {
            attempted_cards = true;
            continue;
        }

        let card = Card {
            terms,
            definitions,
            span: card_span.clone(),
        };
        if let Some(original_card) = cards.get(&card) {
            errors.push(ParseError::DuplicateCard {
                duplicate: card_span,
                original: original_card.span.clone(),
            });
        } else {
            cards.insert(card);
        }
    }

    if cards.is_empty() && !attempted_cards {
        errors.push(ParseError::EmptySet);
    }

    if errors.is_empty() {
        Ok(Set {
            title: title.unwrap(),
            cards,
        })
    } else {
        Err(errors)
    }
}

fn parse_options<'a>(
    input: &'a str,
    source: &str,
    side: CardSide,
    errors: &mut Vec<ParseError>,
) -> BTreeSet<&'a str> {
    let mut options = BTreeSet::new();

    for option_untrimmed in input.split(',') {
        let option = option_untrimmed.trim();

        if option.is_empty() {
            errors.push(ParseError::EmptyOption {
                side,
                span: if option_untrimmed.is_empty() {
                    let input_offset = offset_of(input, source);
                    let option_offset = offset_of(option_untrimmed, input);
                    option_offset.saturating_sub(1) + input_offset
                        ..cmp::min(option_offset + 1, input.len()) + input_offset
                } else {
                    range_of(option_untrimmed, source)
                },
            });
            continue;
        }

        if !options.insert(option) {
            errors.push(ParseError::DuplicateOption {
                side,
                duplicate: range_of(option, source),
                original: range_of(options.get(option).unwrap(), source),
            });
        }
    }

    options
}

/// A parsed `.set` file.
#[derive(Debug, PartialEq, Eq)]
pub struct Set<'a> {
    /// The title of the set.
    pub title: &'a str,
    /// The cards in the set.
    pub cards: HashSet<Card<'a>>,
}

/// A card, consisting of some terms and some definitions.
#[derive(Debug)]
pub struct Card<'a> {
    /// The terms of the card.
    pub terms: BTreeSet<&'a str>,
    /// Possible definitions of those terms.
    pub definitions: BTreeSet<&'a str>,
    /// The source span of this card.
    pub span: Range<usize>,
}

impl PartialEq for Card<'_> {
    fn eq(&self, other: &Card<'_>) -> bool {
        self.terms == other.terms && self.definitions == other.definitions
    }
}

impl Eq for Card<'_> {}

impl Hash for Card<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.terms.hash(state);
        self.definitions.hash(state);
    }
}

/// An error parsing a `.set` file.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The set did not have a title.
    ///
    /// Contains the span of the line that should have contained a title, if there was one.
    NoTitle(Option<Range<usize>>),

    /// The second line of the set was not empty.
    ///
    /// Contains the span of the second line.
    SecondLineNotEmpty(Range<usize>),

    /// The set did not contain any terms.
    EmptySet,

    /// An option was empty.
    EmptyOption {
        /// The side the option was on.
        side: CardSide,
        /// The empty option's span.
        span: Range<usize>,
    },

    /// An option was duplicated.
    DuplicateOption {
        /// The side the duplicated option was on.
        side: CardSide,
        /// The span of the original term.
        original: Range<usize>,
        /// The span of the duplicated term.
        duplicate: Range<usize>,
    },

    /// A card lacks any definitions.
    ///
    /// Contains the range of the line the card is on.
    NoDefinitions(Range<usize>),

    /// There was a third part to the card (two ` - `s in one line).
    ///
    /// Contains the range from the start of the ` - ` to the end of the line.
    ThirdPart {
        /// The span before the start of the ` - `.
        before: Range<usize>,
        /// The span from the start of the ` - ` to the end of the line.
        span: Range<usize>,
    },

    /// A card was duplicated.
    DuplicateCard {
        /// The span of the original card.
        original: Range<usize>,
        /// The span of the duplicated card.
        duplicate: Range<usize>,
    },
}

/// The side of a card: term or definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardSide {
    /// The term side (left of ` - `).
    Term,
    /// The definition side (right of ` - `).
    Definition,
}

impl Display for CardSide {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Term => "term",
            Self::Definition => "definition",
        })
    }
}

fn offset_of(needle: &str, source: &str) -> usize {
    let offset = (<*const str>::cast::<*const ()>(needle) as usize)
        .checked_sub(<*const str>::cast::<*const ()>(source) as usize)
        .unwrap();
    assert!(offset <= source.len());
    offset
}

fn range_of(needle: &str, source: &str) -> Range<usize> {
    let offset = offset_of(needle, source);
    offset..offset + needle.len()
}

#[cfg(test)]
mod tests {
    mod output {
        use maplit::{btreeset, hashset};

        use crate::{parse, Card, Set};

        #[test]
        fn simple() {
            let set = "title\n\nterm -  definition1 ,  definition2  \n\nterm1,term2,  term3   - definition ";
            assert_eq!(
                parse(set).unwrap(),
                Set {
                    title: "title",
                    cards: hashset! {
                        Card {
                            terms: btreeset! { "term" },
                            definitions: btreeset! { "definition1", "definition2" },
                            span: 0..0,
                        },
                        Card {
                            terms: btreeset! { "term1", "term2", "term3" },
                            definitions: btreeset! { "definition" },
                            span: 0..0
                        },
                    },
                }
            );
        }
    }

    mod pass {
        use crate::parse;

        #[test]
        fn any_line_break() {
            assert_eq!(
                parse("title\r\n\nx - y\r\n\r\n\n\r\nz - w\r\n\n").unwrap(),
                parse("title\n\nx - y\nz - w").unwrap()
            );
        }

        #[test]
        fn ignore_empty_lines() {
            assert_eq!(
                parse("title\n\na - b\n\n\nc - d\n\ne - f\n\n\n\n\n").unwrap(),
                parse("title\n\na - b\nc - d\ne - f").unwrap(),
            );
        }

        #[test]
        fn comments() {
            assert_eq!(
                parse("\t# x\n# x\ntitle\n# xyz\n\na - b\n# comment\nc - d\n   #comment"),
                parse("title\n\na - b\nc - d"),
            );
        }
    }

    mod error {
        use crate::{parse, CardSide, ParseError};

        #[test]
        fn no_title() {
            assert_eq!(
                parse("").unwrap_err(),
                [ParseError::NoTitle(None), ParseError::EmptySet]
            );
            assert_eq!(
                parse("#x\n").unwrap_err(),
                [ParseError::NoTitle(None), ParseError::EmptySet]
            );
            assert_eq!(
                parse("\n\nt - d").unwrap_err(),
                [ParseError::NoTitle(Some(0..0))]
            );
            assert_eq!(
                parse("#c\n\n\nt - d").unwrap_err(),
                [ParseError::NoTitle(Some(3..3))]
            );
        }

        #[test]
        fn empty_set() {
            assert_eq!(parse("title").unwrap_err(), [ParseError::EmptySet]);
            assert_eq!(parse("title\n").unwrap_err(), [ParseError::EmptySet]);
            assert_eq!(parse("title\n\n").unwrap_err(), [ParseError::EmptySet]);
            assert_eq!(
                parse("title\n\n\n\n\n\n\n\n\n").unwrap_err(),
                [ParseError::EmptySet]
            );
            assert_eq!(
                parse("title\n\n#a\n#b\n#c").unwrap_err(),
                [ParseError::EmptySet]
            );
        }

        #[test]
        fn nonempty_second_line() {
            assert_eq!(
                parse("title\ntest").unwrap_err(),
                [ParseError::SecondLineNotEmpty(6..10), ParseError::EmptySet]
            );
            assert_eq!(
                parse("title\nabc\nt - d").unwrap_err(),
                [ParseError::SecondLineNotEmpty(6..9)]
            );
            assert_eq!(
                parse("title\n# c\nabc\nt - d").unwrap_err(),
                [ParseError::SecondLineNotEmpty(10..13)],
            );
            assert_eq!(
                parse("\ntest\nt - d").unwrap_err(),
                [
                    ParseError::NoTitle(Some(0..0)),
                    ParseError::SecondLineNotEmpty(1..5)
                ]
            );
        }

        /// Generate a parsing function that always produces a vector of an error, matched by the given
        /// pattern.
        macro_rules! parse_error {
            ($error:pat => $res:expr) => {
                |input: &str| {
                    parse(input)
                        .unwrap_err()
                        .into_iter()
                        .map(|e| match e {
                            $error => $res,
                            _ => panic!("{:?}", e),
                        })
                        .collect::<Vec<_>>()
                }
            };
        }

        #[test]
        fn empty_term() {
            let parse =
                parse_error!(ParseError::EmptyOption { side: CardSide::Term, span } => span);

            assert_eq!(parse("t\n\n - b"), [3..3]);
            assert_eq!(parse("t\n\n, - b"), [3..4, 3..4]);
            assert_eq!(parse("t\n\n,, - b"), [3..4, 3..5, 4..5]);
            assert_eq!(parse("t\n\n,,, - b"), [3..4, 3..5, 4..6, 5..6]);
            assert_eq!(parse("t\n\n,,,, - b"), [3..4, 3..5, 4..6, 5..7, 6..7]);
            assert_eq!(parse("t\n\na,, ,   ,c - d"), [4..6, 6..7, 8..11]);
        }

        #[test]
        fn duplicate_term() {
            parse("t\n\na,b,c - d\na,b,c - e").unwrap();

            let parse = parse_error!(
                ParseError::DuplicateOption {
                    side: CardSide::Term,
                    original,
                    duplicate
                } => (original, duplicate)
            );
            assert_eq!(
                parse("t\n\n  term,term ,other,     term    , other - m"),
                [(5..9, 10..14), (5..9, 27..31), (16..21, 37..42)]
            );
        }

        #[test]
        fn no_definitions() {
            let parse = parse_error!(ParseError::NoDefinitions(span) => span);
            assert_eq!(
                parse("t\n\nterm, other term, -- y --\na - b\na-b"),
                [3..28, 35..38]
            );
        }

        #[test]
        fn empty_definition() {
            let parse =
                parse_error!(ParseError::EmptyOption { side: CardSide::Definition, span } => span);
            assert_eq!(
                parse("t\n\na - b ,   ,, ,"),
                [10..13, 13..15, 15..16, 16..17]
            );
        }

        #[test]
        fn duplicate_definition() {
            let parse = parse_error!(
                ParseError::DuplicateOption {
                    side: CardSide::Definition,
                    original,
                    duplicate,
                } => (original, duplicate)
            );
            assert_eq!(
                parse("t\n\nt -  abc,   d ,abc,   abc   ,d"),
                [(8..11, 18..21), (8..11, 25..28), (15..16, 32..33)],
            );
        }

        #[test]
        fn third_part() {
            let parse = parse_error!(ParseError::ThirdPart { before, span } => (before, span));
            assert_eq!(
                parse("t\n\na - b -c- d - e - f\ng - h-i\nj - k - l\n"),
                [(3..14, 14..22), (31..36, 36..40)]
            );
        }

        #[test]
        fn duplicate_card() {
            parse("t\n\na,b - c,d\nc,d - a,b").unwrap();
            let parse =
                parse_error!(ParseError::DuplicateCard { original: o, duplicate: d } => (o, d));
            assert_eq!(
                parse("t\n\na,b - c,d\nx - y\n b , a  -  d , c\r\nx  -  y\r\n"),
                [(3..12, 19..35), (13..18, 37..44)]
            );
        }
    }
}
