pub mod term;
pub mod ui;

use crate::term::{deserialize_terms, Term};
use colored::Colorize;
use rand::{Rng, seq::SliceRandom};
use rustyline::error::ReadlineError;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct Set {
    name: String,
    #[serde(deserialize_with = "deserialize_terms")]
    terms: Vec<Term>,
}

pub trait Tester: Fn(&[Term], usize) -> Result<bool, ReadlineError> {}
impl<T: Fn(&[Term], usize) -> Result<bool, ReadlineError>> Tester for T {}

impl Set {
    pub fn new(name: String) -> Set {
        Set {
            name,
            terms: Vec::new(),
        }
    }

    pub fn push_set(&mut self, mut set: Set) {
        self.name.push_str(" + ");
        self.name.push_str(&set.name);
        self.terms.append(&mut set.terms);
    }

    pub fn shuffle(&mut self) {
        self.terms.shuffle(&mut rand::thread_rng());
    }

    pub fn test<T: Tester>(&self, tester: T) -> Result<Vec<bool>, ReadlineError> {
        let mut results = Vec::with_capacity(self.terms.len());
        let mut correct = 0;
        let mut incorrect = 0;

        for i in 0..self.terms.len() {
            ui::clear()?;
            println!("{}", self.name);
            ui::show_separator()?;
            println!(
                "{} {} {}",
                correct.to_string().bright_green(),
                incorrect.to_string().bright_red(),
                (self.terms.len() - correct - incorrect)
                    .to_string()
                    .bright_black()
            );
            ui::show_separator()?;
            println!();

            let result = tester(&self.terms, i)?;
            if result {
                correct += 1;
            } else {
                incorrect += 1;
            }
            results.push(result);
        }
        ui::show_separator()?;
        println!("\n\nFinal score: {}/{}\n", correct, self.terms.len());
        if incorrect == 0 {
            println!("All correct, well done!");
        } else {
            println!("Incorrect terms:");
            for term in self
                .terms
                .iter()
                .zip(&results)
                .filter_map(|(term, correct)| if !correct { Some(term) } else { None })
            {
                println!("- {}", term);
            }
        }
        ui::wait_key()?;
        Ok(results)
    }

    pub fn rounds<T: Tester>(&self, tester: T) -> Result<(), ReadlineError> {
        let mut round = 0;
        let mut set = self.clone();
        while !set.terms.is_empty() {
            round += 1;
            set.name = format!("{}: test round {}", self.name, round);
            set.shuffle();
            let results = set.test(&tester)?;
            set.terms = set
                .terms
                .into_iter()
                .zip(&results)
                .filter_map(|(term, correct)| if !correct { Some(term) } else { None })
                .collect();
        }
        Ok(())
    }

    pub fn learn(&self, stages: &[Box<dyn Tester>]) -> Result<(), ReadlineError> {
        let mut rand = rand::thread_rng();
        let mut term_stages = vec![0; self.terms.len()];

        let mut i = self.terms.len();
        loop {
            let incomplete = term_stages.iter().filter(|&&stage| stage < stages.len()).count();
            if incomplete == 0 {
                break;
            }

            loop {
                let new_i = rand.gen_range(0, self.terms.len());
                if term_stages[new_i] < stages.len() && (new_i != i || incomplete == 1) { 
                    i = new_i;
                    break;
                }
            }

            ui::clear()?;
            println!("{}", self.name);
            ui::show_separator()?;
            print!("{} ", term_stages.iter().filter(|&&s| s == 0).count().to_string().bright_black());
            for stage in 1..stages.len() {
                print!("{} ", term_stages.iter().filter(|&&s| s == stage).count());
            }
            print!("{}", term_stages.iter().filter(|&&s| s == stages.len()).count().to_string().bright_green());
            println!();
            ui::show_separator()?;
            println!();

            if stages[term_stages[i]](&self.terms, i)? {
                term_stages[i] += 1;
            } else if term_stages[i] > 0 {
                term_stages[i] -= 1;
            }
        }
        Ok(())
    }
}
