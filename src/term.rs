use crate::ui;
use arrayvec::ArrayVec;
use colored::Colorize;
use rand::Rng;
use regex::Regex;
use rustyline::error::ReadlineError;
use serde::de::{Deserializer, Error, MapAccess, Visitor};
use std::convert::TryInto;
use std::{cmp, fmt};
use termion::event::Key;

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

    pub fn write(&self, inverted: bool) -> Result<bool, ReadlineError> {
        let (term, definition, prompt) = if inverted {
            (&self.definition, &self.term, "Definition: ")
        } else {
            (&self.term, &self.definition, "Term: ")
        };
        println!("\n  {}\n", term.as_str().bold());
        let line = ui::get_line(&prompt.bright_black().to_string())?;

        let mut right = if definition.as_str() == line {
            true
        } else if let Some(matched) = definition.find(&line) {
            matched.start() == 0 && matched.end() == line.len()
        } else {
            false
        };

        if !right {
            println!("\n  {}\n", "Incorrect".bright_red().bold());
            println!(
                "{}{}",
                prompt.bright_black(),
                definition.as_str().bright_green()
            );
            print!("\noverride (c)orrect, or continue...");
            if let Key::Char('c') = ui::get_one_key()? {
                right = true;
            }
        }

        Ok(right)
    }

    pub fn write_definition(terms: &[Self], i: usize) -> Result<bool, ReadlineError> {
        terms[i].write(false)
    }

    pub fn write_term(terms: &[Self], i: usize) -> Result<bool, ReadlineError> {
        terms[i].write(true)
    }

    pub fn choose(terms: &[Self], answer: usize, inverted: bool) -> Result<bool, ReadlineError> {
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

        println!("\n  {}\n", terms[answer].term.as_str().bold());

        for (option, term) in options.iter().enumerate() {
            println!(
                "{}{} {}",
                option.to_string().bright_black(),
                ":".bright_black(),
                if inverted {
                    &term.term
                } else {
                    &term.definition
                }
            );
        }

        print!(
            "\n{}{}{} ",
            "0..".bright_black(),
            (options.len() - 1).to_string().bright_black(),
            ":".bright_black()
        );
        let num = ui::get_key_map(|key| match key {
            Key::Char(c) => c.to_digit(10).and_then(|n| {
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
            println!("\n  {}\n", "Incorrect".bright_red().bold());
            println!(
                "{}",
                if inverted {
                    &terms[answer].term
                } else {
                    &terms[answer].definition
                }
                .as_str()
                .bright_green()
            );
            print!("\noverride (c)orrect, or continue...");
            if let Key::Char('c') = ui::get_one_key()? {
                right = true;
            }
        }

        Ok(right)
    }

    pub fn choose_definition(terms: &[Self], i: usize) -> Result<bool, ReadlineError> {
        Self::choose(terms, i, false)
    }

    pub fn choose_term(terms: &[Self], i: usize) -> Result<bool, ReadlineError> {
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
