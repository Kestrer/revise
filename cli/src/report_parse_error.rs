use std::ops::Range;

use revise_parser::ParseError;

use crate::report::{Annotation, Report, Source};

pub(crate) fn report_parse_error(source: &Source, error: ParseError) -> Report<'_> {
    match error {
        ParseError::NoTitle { line } => no_title(source, line),
        ParseError::EmptySet => empty_set(source),
        ParseError::DuplicateCard {
            original,
            duplicate,
        } => duplicate_card(source, original, duplicate),
        ParseError::ThirdPart { before, span } => third_part(source, before, span),
        ParseError::MissingWhitespaceAroundDash { dash } => {
            missing_whitespace_around_dash(source, dash)
        }
        ParseError::NoTerms { card } => no_terms(source, card),
        ParseError::NoDefinitions { card } => no_definitions(source, card),
        ParseError::DuplicateOption {
            original,
            duplicate,
        } => duplicate_option(source, original, duplicate),
        ParseError::EmptyOption { span } => empty_option(source, span),
        ParseError::TrailingOptionChars { span } => trailing_option_chars(source, span),
        ParseError::UnknownEscape { escape, span } => unknown_escape(source, escape, span),
        ParseError::UnclosedQuote { span } => unclosed_quote(source, span),
        ParseError::UnexpectedControlChar { character, span } => {
            unexpected_control_char(source, character, span)
        }
        ParseError::ExpectedSpace { character, span } => expected_space(source, character, span),
        ParseError::MissingLineFeed { cr_span } => missing_line_feed(source, cr_span),
    }
}

fn no_title(source: &Source, line: Range<usize>) -> Report<'_> {
    Report::error("set does not have a title").with_section(if line.is_empty() {
        source.label_all(Annotation::error("expected a title"))
    } else {
        source.label(line, Annotation::error("expected a title"))
    })
}

fn empty_set(source: &Source) -> Report<'_> {
    Report::error("expected one or more cards in the set")
        .with_section(source.label_all(Annotation::error("no cards found in this set")))
}

fn duplicate_card(source: &Source, original: Range<usize>, duplicate: Range<usize>) -> Report<'_> {
    Report::error("encountered duplicate card").with_section(
        source
            .label(original, Annotation::warning("original card declared here"))
            .label(
                duplicate,
                Annotation::error("identical card declared again here"),
            ),
    )
}

fn third_part(source: &Source, before: Range<usize>, span: Range<usize>) -> Report<'_> {
    Report::error("encountered unexpected third section")
        .with_section(
            source
                .label(
                    span,
                    Annotation::error("unexpected third section to the card"),
                )
                .label(
                    before,
                    Annotation::warning("this card already has terms and definitions"),
                ),
        )
        .with_footer(Annotation::help(
            "consider removing the unnecessary section",
        ))
}

fn missing_whitespace_around_dash(source: &Source, dash: Range<usize>) -> Report<'_> {
    Report::error("missing whitespace around dash").with_section(source.label(
        dash,
        Annotation::error("this dash should be surrounded by whitespace on both sides"),
    ))
}

fn no_terms(source: &Source, card: Range<usize>) -> Report<'_> {
    Report::error("no terms provided").with_section(source.label(
        card,
        Annotation::error("this card requires one or more terms"),
    ))
}

fn no_definitions(source: &Source, card: Range<usize>) -> Report<'_> {
    Report::error("no definitions provided")
        .with_section(source.label(
            card,
            Annotation::error("this card requires one or more definitions"),
        ))
        .with_footer(Annotation::help(
            "add a comma-separated list of definitions to this card after a ` - ` separator",
        ))
}

fn duplicate_option(
    source: &Source,
    original: Range<usize>,
    duplicate: Range<usize>,
) -> Report<'_> {
    Report::error("duplicate option").with_section(
        source
            .label(original, Annotation::warning("original option here"))
            .label(
                duplicate,
                Annotation::error("identical option declared again here"),
            ),
    )
}

fn empty_option(source: &Source, span: Range<usize>) -> Report<'_> {
    Report::error("empty option")
        .with_section(source.label(span, Annotation::error("expected a value here")))
        .with_footer(Annotation::help("consider filling in a value"))
}

fn trailing_option_chars(source: &Source, span: Range<usize>) -> Report<'_> {
    Report::error("characters after a quote in an option")
        .with_section(source.label(span, Annotation::help("remove these characters")))
}

fn unknown_escape(source: &Source, escape: char, span: Range<usize>) -> Report<'_> {
    let msg = format!("unknown escape sequence \\{}", escape);
    Report::error(msg.clone())
        .with_section(source.label(span, Annotation::error(msg)))
        .with_footer(Annotation::help("known escape sequences are \\\" and \\\\"))
}

fn unclosed_quote(source: &Source, span: Range<usize>) -> Report<'_> {
    Report::error("unclosed quote")
        .with_section(source.label(span, Annotation::error("this string lacks a closing quote")))
}

fn unexpected_control_char(source: &Source, character: char, span: Range<usize>) -> Report<'_> {
    Report::error("unexpected control character").with_section(source.label(
        span,
        Annotation::error(format!(
            "the control character {:?} is not allowed here",
            character
        )),
    ))
}

fn expected_space(source: &Source, character: char, span: Range<usize>) -> Report<'_> {
    Report::error("expected space character").with_section(source.label(
        span,
        Annotation::help(format!(
            "the whitespace character {:?} looks like a space, but is not",
            character
        )),
    ))
}

fn missing_line_feed(source: &Source, cr_span: Range<usize>) -> Report<'_> {
    Report::error("missing LF in CRLF pair").with_section(source.label(
        cr_span,
        Annotation::error("found a bare CR with no following LF"),
    ))
}
