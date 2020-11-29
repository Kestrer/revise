#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::non_ascii_literal)]

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
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

use revise::{Database, Term};

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

    let mut storage_bytes = Vec::new();
    storage.read_to_end(&mut storage_bytes)?;

    let mut database = if storage_bytes.is_empty() {
        Database::new()
    } else {
        ron::de::from_bytes(&storage_bytes)?
    };

    database.make_incomplete(&set.terms);

    let out = io::stdout();
    let mut out = out.lock();

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
    let mut rng = rand::thread_rng();

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

    while let Some(term) = database.question(&set.terms, &mut rng) {
        queue!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        write!(out, "{}\r\n", style(&set.name).bold())?;
        write!(
            out,
            "{} {} {} {}\r\n",
            style(database.count_level(&set.terms, 0)).dark_red(),
            style(database.count_level(&set.terms, 1)),
            style(database.count_level(&set.terms, 2)),
            style(database.count_level(&set.terms, 3)).dark_green(),
        )?;
        let separator = "─".dim();
        for _ in 0..terminal::size()?.0 {
            write!(out, "{}", separator)?;
        }
        write!(out, "\r\n\r\n")?;

        write!(out, " {}\r\n\r\n", term.prompt(&mut rng).bold())?;

        write!(out, "{}", "Term: ".dim())?;
        out.flush()?;
        let answer = ui::read_line(&mut out)?;

        let correct = match term.check(&answer) {
            Ok(()) => true,
            Err(correct) => {
                write!(out, "\r\n\r\n")?;
                write!(out, " {}\r\n\r\n", "Incorrect".dark_red().bold())?;
                write!(
                    out,
                    "{}{}\r\n\r\n",
                    "Answer: ".dim(),
                    style(correct).dark_green()
                )?;
                write!(out, "Override (c)orrect or continue: ")?;
                out.flush()?;
                if ui::read_key()?.code == KeyCode::Char('c') {
                    true
                } else {
                    writeln!(out, "\r\n")?;
                    loop {
                        write!(
                            out,
                            "\r{}{}",
                            terminal::Clear(ClearType::UntilNewLine),
                            "Type it out: ".dim()
                        )?;
                        out.flush()?;
                        if term.check(&ui::read_line(&mut out)?).is_ok() {
                            break;
                        }
                    }
                    false
                }
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
