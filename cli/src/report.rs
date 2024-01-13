//! Error reporting, built on `annotate-snippets`.

use std::borrow::{Borrow, Cow};
use std::cmp;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;

use annotate_snippets::display_list::{DisplayList, FormatOptions};
use annotate_snippets::snippet::{self, Snippet};

pub use annotate_snippets::snippet::AnnotationType;

#[must_use]
pub struct Report<'a> {
    pub title: Annotation<'a>,
    sections: Vec<Section<'a>>,
    footers: Vec<Annotation<'a>>,
}

impl<'a> Report<'a> {
    pub fn new(title: Annotation<'a>) -> Self {
        Self {
            title,
            sections: Vec::new(),
            footers: Vec::new(),
        }
    }
    pub fn error(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(Annotation::error(title))
    }
    pub fn warning(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(Annotation::warning(title))
    }

    pub fn with_section(mut self, section: Section<'a>) -> Self {
        assert!(!section.labels.is_empty());
        self.sections.push(section);
        self
    }

    pub fn with_footer(mut self, footer: Annotation<'a>) -> Self {
        self.footers.push(footer);
        self
    }

    pub fn error_chain(error: impl Error) -> Self {
        let mut this = Self::new(Annotation::error(error.to_string()));

        let mut parent: &dyn Error = &error;
        while let Some(source) = parent.source() {
            this = this.with_footer(Annotation::note(format!("caused by: {source}")));
            parent = source;
        }

        this
    }
}

impl Display for Report<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let display_list = DisplayList::from(Snippet {
            title: Some(snippet::Annotation {
                id: None,
                label: Some(&self.title.message),
                annotation_type: self.title.annotation_type,
            }),
            slices: self
                .sections
                .iter()
                .map(|section| {
                    let min_start = section
                        .labels
                        .iter()
                        .map(|label| label.span.start)
                        .min()
                        .unwrap();
                    let max_end = section
                        .labels
                        .iter()
                        .map(|label| label.span.end)
                        .max()
                        .unwrap();

                    let (context, context_line_start) =
                        context_to(min_start..max_end, &section.source.text);

                    let contextual_source = &section.source.text[context.clone()];

                    snippet::Slice {
                        origin: section.source.origin.as_deref(),
                        source: contextual_source,
                        line_start: context_line_start,
                        annotations: section
                            .labels
                            .iter()
                            .map(|label| {
                                let start = label.span.start - context.start;
                                let end = label.span.end - context.start;
                                let start = bytes_to_chars(contextual_source, start);
                                let end = bytes_to_chars(contextual_source, end);
                                snippet::SourceAnnotation {
                                    range: (start, end),
                                    label: &label.annotation.message,
                                    annotation_type: label.annotation.annotation_type,
                                }
                            })
                            .collect(),
                        fold: section.fold,
                    }
                })
                .collect(),
            footer: self
                .footers
                .iter()
                .map(|footer| snippet::Annotation {
                    id: None,
                    label: Some(&footer.message),
                    annotation_type: footer.annotation_type,
                })
                .collect(),
            opt: FormatOptions {
                color: true,
                ..FormatOptions::default()
            },
        });
        writeln!(f, "{display_list}")
    }
}

pub struct Annotation<'a> {
    pub annotation_type: AnnotationType,
    message: Cow<'a, str>,
}

impl<'a> Annotation<'a> {
    pub fn new(annotation_type: AnnotationType, message: impl Into<Cow<'a, str>>) -> Self {
        Self {
            message: message.into(),
            annotation_type,
        }
    }
    pub fn error(label: impl Into<Cow<'a, str>>) -> Self {
        Self::new(AnnotationType::Error, label)
    }
    pub fn warning(label: impl Into<Cow<'a, str>>) -> Self {
        Self::new(AnnotationType::Warning, label)
    }
    pub fn note(label: impl Into<Cow<'a, str>>) -> Self {
        Self::new(AnnotationType::Note, label)
    }
    pub fn help(label: impl Into<Cow<'a, str>>) -> Self {
        Self::new(AnnotationType::Help, label)
    }
}

pub struct Section<'a> {
    source: &'a Source,
    labels: Vec<Label<'a>>,
    fold: bool,
}

impl<'a> Section<'a> {
    pub fn new(source: &'a Source) -> Self {
        Self {
            source,
            labels: Vec::new(),
            fold: false,
        }
    }
    pub fn label(mut self, span: impl Borrow<Range<usize>>, annotation: Annotation<'a>) -> Self {
        self.labels.push(Label {
            span: span.borrow().clone(),
            annotation,
        });
        self
    }
    pub fn label_all(self, annotation: Annotation<'a>) -> Self {
        #[allow(clippy::bool_to_int_with_if)]
        let span_end = self.source.text.len()
            - if self.source.text.ends_with("\r\n") {
                2
            } else if self.source.text.ends_with('\n') {
                1
            } else {
                0
            };
        self.label(0..span_end, annotation).fold()
    }
    pub fn fold(mut self) -> Self {
        self.fold = true;
        self
    }
}

struct Label<'a> {
    span: Range<usize>,
    annotation: Annotation<'a>,
}

pub struct Source {
    pub origin: Option<String>,
    pub text: String,
}

impl Source {
    pub fn label<'a>(
        &'a self,
        span: impl Borrow<Range<usize>>,
        annotation: Annotation<'a>,
    ) -> Section<'a> {
        Section::new(self).label(span, annotation)
    }

    pub fn label_all<'a>(&'a self, annotation: Annotation<'a>) -> Section<'a> {
        Section::new(self).label_all(annotation)
    }
}

fn context_to(span: Range<usize>, s: &str) -> (Range<usize>, usize) {
    let (line_num, start_line) = s
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, offset_of(line, s)))
        .take_while(|&(_, line)| line <= span.start)
        .last()
        .unwrap_or((1, 0));
    let end_line = s
        .lines()
        .map(|line| offset_of(line, s))
        .find(|&line| {
            if span.end == span.start {
                line > span.end
            } else {
                line >= span.end
            }
        })
        .unwrap_or(s.len());
    (start_line..end_line, line_num)
}

#[test]
fn test_context_to() {
    assert_eq!(context_to(0..0, ""), (0..0, 1));
    assert_eq!(context_to(2..2, "a\nbcd\nefgh"), (2..6, 2));
    assert_eq!(context_to(0..10, "abc\ndef\r\ngh"), (0..11, 1));
    assert_eq!(context_to(3..5, "a\nbc\nc\nd\n"), (2..5, 2));
    assert_eq!(context_to(3..6, "a\nbc\r\nd\n"), (2..6, 2));
    assert_eq!(context_to(2..8, "a\nbc\nde\nfg\n"), (2..8, 2));
    assert_eq!(context_to(2..9, "a\nbc\nde\nfg\n"), (2..11, 2));
}

fn offset_of(needle: &str, source: &str) -> usize {
    let offset = (<*const str>::cast::<*const ()>(needle) as usize)
        .checked_sub(<*const str>::cast::<*const ()>(source) as usize)
        .unwrap();
    assert!(offset <= source.len());
    offset
}

fn bytes_to_chars(s: &str, bytes: usize) -> usize {
    match bytes.cmp(&s.len()) {
        cmp::Ordering::Less => {
            s.char_indices()
                .enumerate()
                .find(|(_, (i, _))| *i == bytes)
                .unwrap()
                .0
        }
        cmp::Ordering::Equal => s.chars().count(),
        cmp::Ordering::Greater => panic!("bytes > s.len() ({} > {})", bytes, s.len()),
    }
}

macro_rules! error {
    ($($tt:tt)*) => {
        $crate::report::Report::error(::std::format!($($tt)*))
    }
}
pub(crate) use error;
macro_rules! warning {
    ($($tt:tt)*) => {
        $crate::report::Report::warning(::std::format!($($tt)*))
    }
}
pub(crate) use warning;
