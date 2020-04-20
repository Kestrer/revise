use crate::ui;
use arrayvec::ArrayVec;
use rand::Rng;
use regex::Regex;
use serde::de::{Deserializer, Error, MapAccess, Visitor};
use std::convert::TryInto;
use std::{cmp, fmt};
use std::io::{self, Write};
use crossterm::style::{Colorize, Styler, style};
use crossterm::event::{KeyEvent, KeyCode, KeyModifiers};

#[derive(Clone)]
pub struct Term {
    term: Regex,
    definition: Regex,
}

impl Term {
    pub fn new(term: &str, definition: &str) -> Result<Self, regex::Error> {
        Ok(Self {
            term: Regex::new(term)?,
            definition: Regex::new(definition)?,
        })
    }

    pub fn write(&self, inverted: bool) -> Result<bool, anyhow::Error> {
        let (term, definition, prompt) = if inverted {
            (&self.definition, &self.term, "Definition: ")
        } else {
            (&self.term, &self.definition, "Term: ")
        };
        println!("\n  {}\n", style(term.as_str()).bold());

        let line = ui::get_line(&prompt.dark_grey().to_string())?;

        let mut right = if definition.as_str() == line {
            true
        } else if let Some(matched) = definition.find(&line) {
            matched.start() == 0 && matched.end() == line.len()
        } else {
            false
        };

        if !right {
            print!(
                "\n  {}\n\n{}{}\n\noverride (c)orrect, or continue...",
                "Incorrect".red().bold(),
                prompt.dark_grey(),
                style(definition.as_str()).green(),
            );
            io::stdout().flush()?;
            if ui::get_key_ln()? == KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE) {
                right = true;
            }
        }

        Ok(right)
    }

    pub fn write_definition(terms: &[Self], i: usize) -> Result<bool, anyhow::Error> {
        terms[i].write(false)
    }

    pub fn write_term(terms: &[Self], i: usize) -> Result<bool, anyhow::Error> {
        terms[i].write(true)
    }

    pub fn choose(terms: &[Self], answer: usize, inverted: bool) -> Result<bool, anyhow::Error> {
        let mut rng = rand::thread_rng();

        let mut options = ArrayVec::<[&Self; 4]>::new();

        while options.len() < cmp::min(options.capacity()-1, terms.len()-1) {
            let i = rng.gen_range(0, terms.len());
            if i == answer {
                continue;
            }
            options.push(&terms[i]);
        }

        let answer_option = rng.gen_range(0, options.len() + 1);
        options.insert(answer_option, &terms[answer]);

        println!("\n   {}\n", style(terms[answer].term.as_str()).bold());

        for (option, term) in options.iter().enumerate() {
            println!(
                "{}{} {}",
                style(option).dark_grey(),
                ":".dark_grey(),
                if inverted {
                    &term.term
                } else {
                    &term.definition
                }
            );
        }

        print!(
            "\n{}{}{} ",
            "0..".dark_grey(),
            style(options.len() - 1).dark_grey(),
            ":".dark_grey()
        );
        io::stdout().flush()?;

        let num = ui::get_key_map(|key| match key {
            KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE } => c.to_digit(10).and_then(|n| {
                let n: usize = n.try_into().unwrap();
                if n < options.len() {
                    Some(n)
                } else {
                    None
                }
            }),
            _ => None,
        })?;
        let mut right = num == answer_option;

        if !right {
            print!(
                "\n   {}\n\n{}\n\noverride (c)orrect, or continue...",
                "Incorrect".red().bold(),
                style(if inverted {
                        &terms[answer].term
                    } else {
                        &terms[answer].definition
                    }
                    .as_str()
                ).green(),
            );
            io::stdout().flush()?;
            if ui::get_key_ln()? == KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE) {
                right = true;
            }
        }

        Ok(right)
    }

    pub fn choose_definition(terms: &[Self], i: usize) -> Result<bool, anyhow::Error> {
        Self::choose(terms, i, false)
    }

    pub fn choose_term(terms: &[Self], i: usize) -> Result<bool, anyhow::Error> {
        Self::choose(terms, i, true)
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.term, self.definition)
    }
}

pub fn deserialize_terms<'de, D>(deserializer: D) -> Result<Vec<Term>, D::Error>
where
    D: Deserializer<'de>,
{
    struct TermVisitor;

    impl<'de> Visitor<'de> for TermVisitor {
        type Value = Vec<Term>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "map of terms")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut terms = match map.size_hint() {
                Some(size) => Vec::with_capacity(size),
                None => Vec::new(),
            };

            while let Some((term, definition)) = map.next_entry::<String, String>()? {
                terms.push(Term::new(&term, &definition).map_err(A::Error::custom)?);
            }

            Ok(terms)
        }
    }

    deserializer.deserialize_map(TermVisitor)
}
