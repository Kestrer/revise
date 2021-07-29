#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::non_ascii_literal)]

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use structopt::StructOpt;
use thiserror::Error;

use revise_database::{CardKey, Database};
use revise_set_parser::{ParseError, Set};

mod ui;

mod learn;

mod report;
use report::{Annotation, Report, Source};

#[derive(StructOpt)]
#[structopt(name = "revise", author = "Kestrer")]
enum Opts {
    /// Learn all the cards in one or more sets.
    Learn {
        /// The sets to learn.
        #[structopt(required = true)]
        sets: Vec<PathBuf>,

        /// Whether to invert the terms and definitions.
        #[structopt(short, long)]
        invert: bool,
    },

    /// Check one or more sets syntactically, but don't learn anything.
    Check {
        /// The sets to check.
        #[structopt(required = true)]
        sets: Vec<PathBuf>,
    },
}

fn main() {
    struct StderrReporter<'a> {
        first_report: bool,
        stderr: io::StderrLock<'a>,
    }
    impl Reporter for StderrReporter<'_> {
        fn report(&mut self, report: Report<'_>) {
            drop(if self.first_report {
                self.first_report = false;
                write!(self.stderr, "{}", report)
            } else {
                write!(self.stderr, "\n{}", report)
            });
        }
    }

    let stderr = io::stderr();
    let mut reporter = StderrReporter {
        first_report: true,
        stderr: stderr.lock(),
    };

    if try_main(&mut reporter).is_err() {
        reporter.report(report::error!("aborting due to previous error"));
        drop(reporter);
        std::process::exit(1);
    }
}

trait Reporter {
    fn report(&mut self, report: Report<'_>);
    fn error_chain(&mut self, error: impl Error) {
        self.report(Report::error_chain(error));
    }
}
fn try_main(reporter: &mut impl Reporter) -> Result<(), ()> {
    match Opts::from_args() {
        Opts::Learn { sets, invert } => {
            let mut result = Ok(());

            let sources = sets
                .into_iter()
                .filter_map(|path| read_file(path, reporter).map_err(|e| result = Err(e)).ok())
                .collect::<Vec<_>>();

            let mut title = String::new();
            let mut cards = HashMap::new();

            for source in &sources {
                let set = match parse_set(source, reporter) {
                    Ok(set) => set,
                    Err(()) => {
                        result = Err(());
                        continue;
                    }
                };

                if title.is_empty() {
                    title = set.title.to_owned();
                } else {
                    title.push_str(" + ");
                    title.push_str(set.title);
                }

                cards.extend(set.cards.into_iter().map(|mut card| {
                    if invert {
                        mem::swap(&mut card.terms, &mut card.definitions);
                    }
                    (CardKey::new(&card.terms, &card.definitions), card)
                }));
            }

            result?;

            let mut database = open_database().map_err(|e| reporter.error_chain(e))?;
            learn::learn(&mut database, &title, &cards, &mut io::stdout().lock())
                .map_err(|e| reporter.error_chain(&*e))?;
        }
        Opts::Check { sets } => {
            let mut result = Ok(());

            for path in sets {
                if read_file(path, reporter)
                    .and_then(|source| parse_set(&source, reporter).map(drop))
                    .is_err()
                {
                    result = Err(());
                }
            }

            result?;
        }
    }

    Ok(())
}

fn read_file<P: AsRef<Path>>(path: P, reporter: &mut impl Reporter) -> Result<Source, ()> {
    let path = path.as_ref();

    if path.extension() != Some("set".as_ref()) {
        reporter.report(report::warning!(
            "{} is recommended to have a file extension of `.set`: `{}`",
            path.display(),
            path.with_extension("set").display(),
        ));
    }

    match fs::read_to_string(path) {
        Ok(text) => Ok(Source {
            origin: Some(path.to_string_lossy().into_owned()),
            text,
        }),
        Err(e) => {
            reporter.report(report::error!("couldn't read {}: {}", path.display(), e));
            Err(())
        }
    }
}

fn parse_set<'a>(source: &'a Source, reporter: &mut impl Reporter) -> Result<Set<'a>, ()> {
    revise_set_parser::parse(&source.text).map_err(|errors| {
        for error in errors {
            reporter.report(report_parse_error(&error, &source));
        }
    })
}

fn report_parse_error<'a>(error: &ParseError, source: &'a Source) -> Report<'a> {
    match error {
        ParseError::NoTitle(span) => {
            Report::error("set does not have a title").with_section(span.clone().map_or_else(
                || source.label_all(Annotation::error("expected a title")),
                |span| source.label(span, Annotation::error("expected a title")),
            ))
        }
        ParseError::SecondLineNotEmpty(span) => Report::error("second line is not empty")
            .with_section(source.label(span, Annotation::error("this line must be empty")))
            .with_footer(Annotation::help("consider moving your cards down a line")),
        ParseError::EmptySet => Report::error("expected one or more cards in the set")
            .with_section(source.label_all(Annotation::error("no cards found in this set"))),
        ParseError::EmptyOption { side, span } => Report::error(format!("empty {}", side))
            .with_section(
                source.label(span, Annotation::error(format!("expected a {} here", side))),
            )
            .with_footer(Annotation::help("consider filling in a value")),
        ParseError::DuplicateOption {
            side,
            original,
            duplicate,
        } => Report::error(format!("duplicate {}", side)).with_section(
            source
                .label(
                    original,
                    Annotation::warning(format!("original {} here", side)),
                )
                .label(
                    duplicate,
                    Annotation::error(format!("{} declared again here", side)),
                ),
        ),
        ParseError::NoDefinitions(span) => Report::error("no definitions provided")
            .with_section(source.label(
                span,
                Annotation::error("expected a list of definitions in this card"),
            ))
            .with_footer(Annotation::help(
                "add a comma-separated list of definitions to this card after a ` - ` separator",
            )),
        ParseError::ThirdPart { before, span } => {
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
        ParseError::DuplicateCard {
            original,
            duplicate,
        } => Report::error("encountered duplicate card").with_section(
            source
                .label(original, Annotation::warning("original card declared here"))
                .label(
                    duplicate,
                    Annotation::error("identical card declared again here"),
                ),
        ),
    }
}

fn open_database() -> Result<Database, OpenDatabaseError> {
    let dirs =
        ProjectDirs::from("", "", "revise").ok_or(OpenDatabaseErrorInner::NoHomeDirectory)?;

    fs::create_dir_all(dirs.data_dir()).map_err(|source| OpenDatabaseErrorInner::CreateDir {
        path: dirs.data_dir().to_owned(),
        source,
    })?;

    let database_path = dirs.data_dir().join("data.sqlite3");
    Ok(Database::open(database_path).map_err(OpenDatabaseErrorInner::Open)?)
}

#[derive(Debug, Error)]
#[error("failed to open card database")]
struct OpenDatabaseError(
    #[source]
    #[from]
    OpenDatabaseErrorInner,
);

#[derive(Debug, Error)]
enum OpenDatabaseErrorInner {
    #[error("couldn't find home directory")]
    NoHomeDirectory,
    #[error("couldn't create `{path}`")]
    CreateDir { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Open(revise_database::OpenError),
}
