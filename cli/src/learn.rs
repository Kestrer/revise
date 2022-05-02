use std::collections::{BTreeSet, HashMap};
use std::fmt::{self, Display, Formatter};
use std::io;
use std::marker::PhantomData;
use std::panic;

use crossterm::{
    cursor,
    event::KeyCode,
    execute, queue,
    style::{style, Stylize},
    terminal::{self, ClearType},
};

use rand::distributions::Distribution as _;
use rand::seq::IteratorRandom as _;
use rand::Rng;

use revise_database::{CardKey, Database};
use revise_parser::Card;

pub fn learn(
    database: &mut Database,
    title: &str,
    cards: &HashMap<CardKey, Card>,
    knowledge_weights: [f64; 4],
    mut out: impl io::Write,
) -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let _raw_guard = enter_raw()?;

    let mut session = Session::new();

    loop {
        let question =
            session.generate_question(database, cards.keys(), knowledge_weights, &mut rng)?;
        let card = &cards[question.card_key()];

        queue!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        write!(out, "{}\r\n", title.bold())?;

        let distribution = question.level_distribution();
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

        question.record_result(correct)?;
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

/// All state stored in a learning session.
struct Session<'cards> {
    /// The previous card that was asked.
    /// This is used to avoid asking the same card twice in a row.
    previous_card: Option<&'cards CardKey>,
}
impl<'cards> Session<'cards> {
    fn new() -> Self {
        Self {
            previous_card: None,
        }
    }

    fn generate_question<'database, C, R>(
        &mut self,
        database: &'database mut Database,
        cards: C,
        knowledge_weights: [f64; 4],
        rng: &mut R,
    ) -> anyhow::Result<Question<'_, 'database, 'cards>>
    where
        C: IntoIterator<Item = &'cards CardKey>,
        C::IntoIter: 'cards + Clone + ExactSizeIterator,
        R: Rng,
    {
        let cards = cards.into_iter();

        match cards.len() {
            0 => panic!("no cards given to `generate_question`"),
            1 => self.previous_card = None,
            _ => {}
        }

        let card_knowledges = database
            .knowledge_all(cards)?
            .map(|(card, knowledge)| (card, usize::from(knowledge.level.get())));

        let mut level_distribution = [0; 4];
        let mut choosable_distribution = [0; 4];
        for (card, knowledge) in card_knowledges.clone() {
            level_distribution[knowledge] += 1;
            if self.previous_card != Some(card) {
                choosable_distribution[knowledge] += 1;
            }
        }

        #[allow(clippy::cast_precision_loss)]
        let weights = choosable_distribution
            .into_iter()
            .zip(knowledge_weights)
            .map(|(weight, multiplier)| (weight as f64) * multiplier);
        let card_level = rand::distributions::WeightedIndex::new(weights)
            .unwrap()
            .sample(rng);
        let card_number = rng.gen_range(0..choosable_distribution[card_level]);

        let (_card_index, (card_key, _)) = card_knowledges
            .enumerate()
            .filter(|&(_, (card_key, knowledge))| {
                Some(card_key) != self.previous_card && knowledge == card_level
            })
            .nth(card_number)
            .unwrap();

        self.previous_card = Some(card_key);

        #[allow(clippy::used_underscore_binding)]
        Ok(Question {
            _session: PhantomData,
            database,
            #[cfg(test)]
            card_index: _card_index,
            card_key,
            level_distribution,
        })
    }
}

struct Question<'session, 'database, 'cards> {
    // a session should only support one question at once
    _session: PhantomData<&'session mut Session<'cards>>,
    database: &'database mut Database,
    #[cfg(test)]
    card_index: usize,
    card_key: &'cards CardKey,
    level_distribution: [usize; 4],
}

impl<'session, 'database, 'cards> Question<'session, 'database, 'cards> {
    fn card_key(&self) -> &'cards CardKey {
        self.card_key
    }

    fn level_distribution(&self) -> [usize; 4] {
        self.level_distribution
    }

    fn record_result(self, correct: bool) -> anyhow::Result<()> {
        if correct {
            self.database.record_correct(self.card_key)?;
        } else {
            self.database.record_incorrect(self.card_key)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::btree_set::BTreeSet;

    use rand::Rng;

    use revise_database::{CardKey, Database};

    use super::Session;

    fn btreeset<I, S>(iter: I) -> BTreeSet<S>
    where
        I: IntoIterator<Item = S>,
        S: Ord + AsRef<[u8]>,
    {
        iter.into_iter().collect()
    }

    fn cards(n: usize) -> Vec<CardKey> {
        (0_u8..=255)
            .map(|b| {
                let set = btreeset([[b]]);
                CardKey::new(&set, &set)
            })
            .take(n)
            .collect()
    }

    #[test]
    fn no_duplicates() {
        let mut database = Database::open_in_memory().unwrap();
        let mut rng = rand::thread_rng();
        let mut session = Session::new();

        let cards = cards(2);

        let mut previous = None;
        for _ in 0..1000 {
            let question = session
                .generate_question(&mut database, &cards, [1.0; 4], &mut rng)
                .unwrap();
            if let Some(previous) = previous {
                assert_ne!(question.card_index, previous);
            }
            previous = Some(question.card_index);

            question.record_result(rng.gen()).unwrap();
        }

        assert!(previous.is_some());
    }

    #[test]
    fn equal_distribution() {
        let mut database = Database::open_in_memory().unwrap();
        let mut rng = rand::thread_rng();
        let mut session = Session::new();

        let cards = cards(5);
        let mut occurrences = (0..cards.len()).map(|_| 0).collect::<Vec<_>>();

        const ITERATIONS: usize = 1000;
        for _ in 0..ITERATIONS {
            let question = session
                .generate_question(&mut database, &cards, [1.0; 4], &mut rng)
                .unwrap();
            occurrences[question.card_index] += 1;
            question.record_result(true).unwrap();
        }

        let average = ITERATIONS / cards.len();
        for occurrences in occurrences {
            assert!(
                ((average - 50)..(average + 50)).contains(&occurrences),
                "{occurrences} is too far off {average}"
            );
        }
    }
}
