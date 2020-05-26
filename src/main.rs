use std::convert::TryFrom;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::iter::FromIterator;
use std::panic;

use anyhow::{anyhow, Context as _};
use clap::{App, Arg};
use crossterm::{
    cursor,
    event::KeyCode,
    execute, queue,
    style::{style, Colorize, Styler},
    terminal::{self, ClearType},
};
use directories_next::ProjectDirs;
use scopeguard::defer_on_success;
use serde::Deserialize;

use revise::{AnswerType, Database, Term};

mod ui;

fn main() -> anyhow::Result<()> {
    let matches = App::new("revise")
        .version("0.3")
        .about("Utility to help students revise.")
        .author("Koxiaet")
        .arg(
            Arg::with_name("sets")
                .help("The sets to revise")
                .multiple(true)
                .required(true),
        )
        .get_matches();

    let set: Set = matches
        .values_of("sets")
        .unwrap()
        .map(|filename| {
            Ok(
                ron::de::from_bytes(&fs::read(filename).context("Failed to open set")?)
                    .context("Set format invalid")?,
            )
        })
        .collect::<Result<Option<Set>, anyhow::Error>>()?
        .unwrap();

    let dirs =
        ProjectDirs::from("", "", "revise").ok_or_else(|| anyhow!("No home directory found"))?;

    fs::create_dir_all(dirs.data_dir())?;

    let mut storage = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(dirs.data_dir().join("data.ron"))?;

    let mut database = if storage.metadata()?.len() != 0 {
        ron::de::from_reader(&mut storage)?
    } else {
        Database::new()
    };

    database.cap_knowledge(&set.terms);

    let mut out = BufWriter::new(io::stdout());

    match revise_set(&mut database, &set, &mut out) {
        Err(e) if e.is::<ui::QuitEarly>() => (),
        other => other?,
    }

    out.flush()?;

    storage.seek(SeekFrom::Start(0))?;
    storage.set_len(0)?;
    ron::ser::to_writer(&mut storage, &database)?;

    Ok(())
}

fn revise_set(database: &mut Database, set: &Set, mut out: impl io::Write) -> anyhow::Result<()> {
    enter_raw()?;

    // Panic hook so that raw mode is exited before the error message is printed
    let old_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = exit_raw();
        old_hook(info);
    }));

    // Don't exit raw mode twice; only call this when not panicking.
    defer_on_success! {
        let _ = exit_raw();
        let _ = panic::take_hook();
    };

    while let Some((question, term)) = database.question(&set.terms, &mut rand::thread_rng()) {
        queue!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        write!(out, "{}\r\n", style(&set.name).bold())?;
        write!(
            out,
            "{} {} {} {}\r\n",
            database.count_level(&set.terms, 0).to_string().red(),
            database.count_level(&set.terms, 1),
            database.count_level(&set.terms, 2),
            database.count_level(&set.terms, 3).to_string().green(),
        )?;
        let separator = "-".dim().to_string();
        for _ in 0..terminal::size()?.0 {
            out.write_all(separator.as_bytes())?;
        }
        write!(out, "\r\n\r\n")?;

        write!(out, " {}\r\n\r\n", question.prompt.bold())?;
        let answer = match question.answer_type {
            AnswerType::MultipleChoice(choices) => {
                for (i, choice) in choices.iter().enumerate() {
                    write!(out, "{}{}\r\n", format!("{}: ", i + 1).dim(), choice)?;
                }
                write!(out, "\r\n{}", format!("1..{}: ", choices.len()).dim())?;
                out.flush()?;
                let num = loop {
                    if let KeyCode::Char(key @ '1'..='9') = ui::read_key()?.code {
                        if let Some(num) = key.to_digit(10) {
                            let num = usize::try_from(num).unwrap() - 1;
                            if num < choices.len() {
                                break num;
                            }
                        }
                    }
                };
                choices.into_iter().nth(num).unwrap()
            }
            AnswerType::Write => {
                write!(out, "{}", "Term: ".dim())?;
                out.flush()?;
                ui::read_line(&mut out)?
            }
        };
        let correct = match term.check(&answer) {
            Ok(()) => true,
            Err(correct) => {
                write!(out, "\r\n\r\n")?;
                write!(out, " {}\r\n\r\n", "Incorrect".red().bold())?;
                write!(
                    out,
                    "{}{}\r\n\r\n",
                    "Answer: ".dim(),
                    style(correct).green()
                )?;
                write!(out, "Override (c)orrect or continue: ")?;
                out.flush()?;
                ui::read_key()?.code == KeyCode::Char('c')
            }
        };
        database.record(term, correct);
    }

    Ok(())
}

fn enter_raw() -> anyhow::Result<()> {
    execute!(
        io::stdout(),
        terminal::EnterAlternateScreen,
        terminal::Clear(ClearType::All)
    )?;
    terminal::enable_raw_mode()?;

    Ok(())
}

fn exit_raw() -> anyhow::Result<()> {
    execute!(io::stdout(), terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Set {
    name: String,
    terms: Vec<Term>,
}

impl Set {
    fn join_with(&mut self, mut other: Self) {
        self.name.reserve(3 + other.name.len());
        self.name.push_str(" + ");
        self.name.push_str(&other.name);

        self.terms.append(&mut other.terms);
    }
}

impl Extend<Set> for Set {
    fn extend<T: IntoIterator<Item = Set>>(&mut self, iter: T) {
        for elem in iter {
            self.join_with(elem);
        }
    }
}
impl FromIterator<Set> for Option<Set> {
    fn from_iter<T: IntoIterator<Item = Set>>(iter: T) -> Self {
        let mut iter = iter.into_iter();
        let mut set = iter.next()?;
        set.extend(iter);
        Some(set)
    }
}
