use std::collections::{BTreeSet, HashMap};
use std::fmt::{self, Display, Formatter};
use std::io;
use std::panic;

use crossterm::{
    cursor,
    event::KeyCode,
    execute, queue,
    style::{style, Stylize},
    terminal::{self, ClearType},
};

use rand::seq::IteratorRandom as _;
use rand::Rng;

use revise_database::{CardKey, Database};
use revise_parser::Card;

pub fn learn(
    database: &mut Database,
    title: &str,
    cards: &HashMap<CardKey, Card>,
    mut out: impl io::Write,
) -> anyhow::Result<()> {
    database.make_incomplete(cards.keys())?;

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
        let answer = match crate::ui::read_line(&mut out)? {
            Some(line) => revise_parser::parse_guess(&line),
            None => break,
        };

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

            let key = match crate::ui::read_key()? {
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
                    let answer = match crate::ui::read_line(&mut out)? {
                        Some(line) => revise_parser::parse_guess(&line),
                        None => break,
                    };

                    if card.definitions == answer {
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

struct DisplayAnswer<'a>(&'a BTreeSet<String>);
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
