#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::non_ascii_literal)]

use std::collections::{BTreeSet, HashMap};
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io;
use std::mem;
use std::panic;
use std::path::PathBuf;

use crossterm::{
    cursor,
    event::KeyCode,
    execute, queue,
    style::{style, Stylize},
    terminal::{self, ClearType},
};
use directories::ProjectDirs;
use rand::seq::IteratorRandom as _;
use rand::Rng;
use structopt::StructOpt;

use revise_database::{CardKey, Database};
use revise_set_parser::{Card, ParseError};

mod ui;

mod report;
use report::{Annotation, Report, Source};

#[derive(StructOpt)]
#[structopt(name = "revise", author = "Kestrer")]
struct Opts {
    /// The sets to revise.
    #[structopt(required = true)]
    sets: Vec<PathBuf>,

    /// Whether to invert the terms and definitions.
    #[structopt(short, long)]
    invert: bool,
}

fn main() {
    if try_main().is_err() {
        report::error!("aborting due to previous error");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), ()> {
    let Opts { sets, invert } = Opts::from_args();

    let mut errored = false;

    let sources = sets
        .into_iter()
        .inspect(|filename| {
            if filename.extension() != Some("set".as_ref()) {
                let mut new_filename = filename.clone();
                new_filename.set_extension("set");
                report::warning!(
                    "{} is recommended to have a file extension of `.set`: `{}`",
                    filename.display(),
                    new_filename.display(),
                );
            }
        })
        .filter_map(|filename| match fs::read_to_string(&filename) {
            Ok(source) => Some(Source {
                origin: Some(filename.to_string_lossy().into_owned()),
                text: source,
            }),
            Err(e) => {
                report::error!("couldn't read {}: {}", filename.display(), e);
                errored = true;
                None
            }
        })
        .collect::<Vec<_>>();

    let mut title = String::new();
    let mut cards = HashMap::new();

    for source in &sources {
        let set = match revise_set_parser::parse(&source.text) {
            Ok(set) => set,
            Err(errors) => {
                for e in errors {
                    report_parse_error(&e, &source).eprint();
                }
                errored = true;
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

    if errored {
        return Err(());
    }

    let dirs = ProjectDirs::from("", "", "revise")
        .ok_or_else(|| report::error!("couldn't find home directory"))?;

    fs::create_dir_all(dirs.data_dir())
        .map_err(|e| report::error!("couldn't create `{}`: {}", dirs.data_dir().display(), e))?;

    let database_path = dirs.data_dir().join("data.sqlite3");
    let mut database = Database::open(database_path).map_err(report::error_chain)?;

    database
        .make_incomplete(cards.keys())
        .map_err(report::error_chain)?;

    let out = io::stdout();
    let mut out = out.lock();

    revise_set(&mut database, &title, &cards, &mut out).map_err(|e| report::error_chain(&*e))?;

    Ok(())
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

fn revise_set(
    database: &mut Database,
    title: &str,
    cards: &HashMap<CardKey, Card<'_>>,
    mut out: impl io::Write,
) -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let _raw_guard = enter_raw()?;

    let mut session = Session::new(database, cards.keys(), rand::thread_rng())?;

    while let Some(card_key) = session.question_card() {
        let card = &cards[card_key];

        queue!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        write!(out, "{}\r\n", title.bold())?;

        let distribution = session.database.level_distribution(cards.keys())?;
        write!(
            out,
            "{} {} {} {}\r\n",
            style(distribution[0]).dark_red(),
            distribution[1],
            distribution[2],
            style(distribution[3]).dark_green(),
        )?;
        let separator = "─".dim();
        for _ in 0..terminal::size()?.0 {
            write!(out, "{}", separator)?;
        }
        write!(out, "\r\n\r\n")?;

        write!(
            out,
            "{}\r\n\r\n",
            card.terms.iter().choose(&mut rng).unwrap()
        )?;

        write!(out, "{}", "Term: ".dim())?;
        out.flush()?;
        let answer = match ui::read_line(&mut out)? {
            Some(line) => line,
            None => break,
        };
        let answer = answer.split(',').map(str::trim).collect::<BTreeSet<_>>();

        let correct = if card.definitions == answer {
            true
        } else {
            write!(out, "\r\n\r\n")?;
            write!(out, " {}\r\n\r\n", "Incorrect".dark_red().bold())?;
            write!(
                out,
                "{}{}\r\n\r\n",
                "Answer: ".dim(),
                style(DisplayAnswer(&card.definitions)).dark_green(),
            )?;
            write!(out, "Override (c)orrect or continue: ")?;
            out.flush()?;

            let key = match ui::read_key()? {
                Some(key) => key,
                None => break,
            };

            if key.code == KeyCode::Char('c') {
                true
            } else {
                writeln!(out, "\r\n")?;
                loop {
                    write!(
                        out,
                        "\r{}{}",
                        terminal::Clear(ClearType::UntilNewLine),
                        "Type it out: ".dim(),
                    )?;
                    out.flush()?;
                    let answer = match ui::read_line(&mut out)? {
                        Some(line) => line,
                        None => break,
                    };
                    let answer = answer.split(',').map(str::trim).collect::<BTreeSet<_>>();

                    if answer == card.definitions {
                        break;
                    }
                }
                false
            }
        };

        session.record_result(correct)?;
    }

    Ok(())
}

fn enter_raw() -> io::Result<impl Drop> {
    fn exit() {
        drop(execute!(io::stdout(), terminal::LeaveAlternateScreen));
        drop(terminal::disable_raw_mode());
    }

    execute!(
        io::stdout(),
        terminal::EnterAlternateScreen,
        terminal::Clear(ClearType::All)
    )?;
    terminal::enable_raw_mode()?;

    // Panic hook so that raw mode is exited before the error message is printed
    let old_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        exit();
        old_hook(info);
    }));

    // Don't exit raw mode twice; only call this when not panicking.
    Ok(scopeguard::guard_on_success((), |()| {
        exit();
        drop(panic::take_hook());
    }))
}

struct DisplayAnswer<'a>(&'a BTreeSet<&'a str>);
impl Display for DisplayAnswer<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut answers = self.0.iter();
        f.write_str(answers.next().unwrap())?;
        for answer in answers {
            write!(f, ", {}", answer)?;
        }
        Ok(())
    }
}

struct Session<'a, R: Rng> {
    database: &'a mut Database,
    active_cards: Vec<&'a CardKey>,
    question: Option<usize>,
    rng: R,
}
impl<'a, R: Rng> Session<'a, R> {
    fn new<I>(database: &'a mut Database, cards: I, mut rng: R) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = &'a CardKey>,
        I::IntoIter: ExactSizeIterator + Clone + 'a,
    {
        let cards = cards.into_iter();

        let active_cards = cards
            .clone()
            .zip(database.knowledge_all(cards)?)
            .filter(|(_, knowledge)| knowledge.level.get() < 3)
            .map(|(key, _)| key)
            .collect::<Vec<_>>();

        let question = if active_cards.is_empty() {
            None
        } else {
            Some(rng.gen_range(0..active_cards.len()))
        };

        Ok(Self {
            database,
            active_cards,
            question,
            rng,
        })
    }

    fn question_card(&self) -> Option<&'a CardKey> {
        Some(self.active_cards[self.question?])
    }

    fn record_result(&mut self, correct: bool) -> anyhow::Result<()> {
        let prev_index = self.question.unwrap();
        let prev_key = self.active_cards[prev_index];

        let mut avoid_picking_prev_index = self.active_cards.len() > 1;

        if correct {
            self.database.record_correct(prev_key)?;

            if self.database.knowledge(prev_key)?.level.get() == 3 {
                self.active_cards.swap_remove(prev_index);
                avoid_picking_prev_index = false;
            }
        } else {
            self.database.record_incorrect(prev_key)?;
        }

        self.question = if self.active_cards.is_empty() {
            None
        } else {
            if avoid_picking_prev_index {
                self.active_cards.swap_remove(prev_index);
            }

            let index = self.rng.gen_range(0..self.active_cards.len());

            if avoid_picking_prev_index {
                self.active_cards.push(prev_key);
            }

            Some(index)
        };

        Ok(())
    }
}

#[test]
fn test_session() {
    use maplit::btreeset;
    use rand::rngs::mock::StepRng;

    let mut database = Database::open_in_memory().unwrap();
    let cards = [
        CardKey::new(&btreeset!("A"), &btreeset!("a")),
        CardKey::new(&btreeset!("B"), &btreeset!("b")),
        CardKey::new(&btreeset!("C"), &btreeset!("c")),
    ];
    let rng = StepRng::new(0, 15_701_263_798_120_398_361);
    let mut session = Session::new(&mut database, &cards, rng).unwrap();

    for i in [0, 1, 0, 2, 1, 2, 0, 2, 1] {
        assert_eq!(session.question_card(), Some(&cards[i]));
        session.record_result(true).unwrap();
    }

    assert_eq!(session.question_card(), None);
}
