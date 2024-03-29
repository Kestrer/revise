#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::non_ascii_literal, clippy::items_after_statements)]

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Context as _;
use clap::Parser as _;
use directories::ProjectDirs;
use thiserror::Error;

use revise_database::{CardKey, Database, Knowledge, KnowledgeLevel};
use revise_parser::Set;

mod ui;

mod learn;

mod report;
use report::{Report, Source};

mod report_parse_error;

#[derive(clap::Parser)]
enum Args {
    /// Learn all the cards in one or more sets.
    Learn {
        /// The sets to learn.
        #[clap(required = true)]
        sets: Vec<PathBuf>,

        /// Whether to invert the terms and definitions.
        #[clap(short, long)]
        invert: bool,

        /// Whether to clear all recorded knowledge before starting.
        #[clap(short, long)]
        clear: bool,

        /// The weights to use for each knowledge category.
        #[clap(short, long, default_value = "10,5,1,0.1")]
        weights: Weights,
    },

    /// Check one or more sets syntactically, but don't learn anything.
    Check {
        /// The sets to check.
        #[clap(required = true)]
        sets: Vec<PathBuf>,
    },

    /// Clear the recorded knowledge of all the cards in the given sets.
    Clear {
        /// The level to clear the knowledge to.
        #[clap(long, default_value = "0")]
        level: KnowledgeLevel,

        /// The sets to clear all knowledge of.
        #[clap(required = true)]
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
                write!(self.stderr, "{report}")
            } else {
                write!(self.stderr, "\n{report}")
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
    match Args::parse() {
        Args::Learn {
            sets,
            invert,
            clear,
            weights,
        } => {
            let mut result = Ok(());

            let sets: Vec<_> = sets
                .into_iter()
                .filter_map(|path| record_err(read_set_file(path, reporter), &mut result))
                .collect();

            result?;

            let mut title = String::new();
            let mut cards = HashMap::new();

            for set in sets {
                if title.is_empty() {
                    title = set.title;
                } else {
                    title.push_str(" + ");
                    title.push_str(&set.title);
                }

                cards.extend(set.cards.into_iter().map(|mut card| {
                    if invert {
                        mem::swap(&mut card.terms, &mut card.definitions);
                    }
                    (CardKey::new(&card.terms, &card.definitions), card)
                }));
            }

            let mut database = open_database().map_err(|e| reporter.error_chain(e))?;
            if clear {
                database
                    .set_knowledge_all(cards.keys(), Knowledge::default())
                    .map_err(|e| reporter.error_chain(&e))?;
            }
            learn::learn(
                &mut database,
                &title,
                &cards,
                weights.0,
                &mut io::stdout().lock(),
            )
            .map_err(|e| reporter.error_chain(&*e))?;
        }
        Args::Check { sets } => {
            let mut result = Ok(());

            for set in sets {
                record_err(read_set_file(set, reporter), &mut result);
            }

            result?;
        }
        Args::Clear { level, sets } => {
            let mut result = Ok(());

            let cards = sets
                .into_iter()
                .filter_map(|set| record_err(read_set_file(set, reporter), &mut result))
                .flat_map(|set| {
                    set.cards.into_iter().flat_map(|card| {
                        [
                            CardKey::new(&card.terms, &card.definitions),
                            CardKey::new(&card.definitions, &card.terms),
                        ]
                    })
                })
                .collect::<HashSet<_>>();

            result?;

            open_database()
                .map_err(|e| reporter.error_chain(e))?
                .set_knowledge_all(
                    &cards,
                    Knowledge {
                        level,
                        safety_net: false,
                    },
                )
                .map_err(|e| reporter.error_chain(e))?;
        }
    }

    Ok(())
}

fn read_set_file<P: AsRef<Path>>(path: P, reporter: &mut impl Reporter) -> Result<Set, ()> {
    let path = path.as_ref();

    if path.extension() != Some("set".as_ref()) {
        reporter.report(report::warning!(
            "{} is recommended to have a file extension of `.set`: `{}`",
            path.display(),
            path.with_extension("set").display(),
        ));
    }

    let text = fs::read_to_string(path).map_err(|e| {
        reporter.report(report::error!("couldn't read to {}: {}", path.display(), e));
    })?;

    revise_parser::parse_set(&text).map_err(|errors| {
        let source = Source {
            origin: Some(path.to_string_lossy().into_owned()),
            text,
        };

        for error in errors {
            reporter.report(self::report_parse_error::report_parse_error(&source, error));
        }
    })
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

struct Weights([f64; 4]);
impl FromStr for Weights {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut parts = input.split(',');
        let parts = (|| {
            let array = [parts.next()?, parts.next()?, parts.next()?, parts.next()?];
            if parts.next().is_some() {
                return None;
            }
            Some(array)
        })()
        .context("expected 4 weights")?;

        let [a, b, c, d] = parts.map(|s| s.trim().parse::<f64>());
        let weights = [a?, b?, c?, d?];

        anyhow::ensure!(
            weights.iter().all(|&w| w > 0.0 && w.is_normal()),
            "all weights must be normal and positive"
        );

        Ok(Self(weights))
    }
}

#[test]
#[allow(clippy::float_cmp)]
fn test_weights() {
    assert_eq!(
        "0.1,0.1,0.1,1".parse::<Weights>().unwrap().0,
        [0.1, 0.1, 0.1, 1.]
    );
    assert_eq!(
        "\t2.5 , 6 , 8,0.001\n".parse::<Weights>().unwrap().0,
        [2.5, 6., 8., 0.001]
    );
}

fn record_err<T, U, E>(res: Result<T, E>, other: &mut Result<U, E>) -> Option<T> {
    res.map_err(|e| *other = Err(e)).ok()
}
