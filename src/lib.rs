#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::items_after_statements)]
// There is a bug in either Clippy or Serde that requires me to do this; the Deserialize derive of
// Database doesn't compile otherwise.
#![allow(clippy::mutable_key_type)]

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::hash::{Hash, Hasher};

use rand::Rng;
use rand::{distributions::Distribution as _, seq::IteratorRandom as _};
use rand_regex::Regex as RandRegex;
use regex::Regex as MatchRegex;
use serde::de::{self, Deserializer, Unexpected, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

/// The database of how well you know which terms.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Database {
    terms: HashMap<Term, Knowledge>,
    previous: Option<Term>,
}

impl Database {
    /// Create a new empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get how well known a term is.
    pub fn knowledge(&self, term: &Term) -> Knowledge {
        self.terms.get(term).copied().unwrap_or_default()
    }
    /// Set the knowledge of a term.
    pub fn set_knowledge(&mut self, term: &Term, knowledge: Knowledge) {
        if knowledge.level.0 == 0 {
            self.terms.remove(term);
        } else if let Some(old_knowledge) = self.terms.get_mut(term) {
            *old_knowledge = knowledge;
        } else {
            self.terms.insert(term.clone(), knowledge);
        }
    }
    /// Record the answer to a question on a term right or wrong.
    pub fn record(&mut self, term: &Term, correct: bool) {
        self.set_knowledge(
            term,
            (if correct {
                Knowledge::correct
            } else {
                Knowledge::incorrect
            })(self.knowledge(term)),
        );
    }
    /// Get the number of terms with this knowledge level.
    pub fn count_level<'a>(&self, terms: impl IntoIterator<Item = &'a Term>, level: u8) -> usize {
        terms
            .into_iter()
            .filter(|term| self.knowledge(term).level.0 == level)
            .count()
    }

    /// Set the knowledge of all terms to level 2 if they are all level 3.
    pub fn make_incomplete(&mut self, terms: &[Term]) {
        if terms.iter().all(|term| self.knowledge(term).level.0 == 3) {
            for term in terms {
                self.set_knowledge(
                    term,
                    Knowledge {
                        level: KnowledgeLevel(2),
                        safety_net: false,
                    },
                );
            }
        }
    }

    /// Ask a question from some list of terms. Returns None if all the questions in the set are
    /// fully known.
    pub fn question<'a>(&mut self, terms: &'a [Term], rand: &mut impl Rng) -> Option<&'a Term> {
        let unknown_terms = terms.iter().filter(|term| self.knowledge(term).level.0 < 3);
        let term = self
            .previous
            .as_ref()
            .map(|previous| unknown_terms.clone().filter(move |&term| term != previous))
            .and_then(|terms| terms.choose(rand))
            .or_else(|| unknown_terms.choose(rand))?;

        self.previous = Some(term.clone());

        Some(term)
    }
}

#[cfg(test)]
#[test]
#[allow(clippy::shadow_unrelated)]
fn test_database() {
    use rand::rngs::mock::StepRng;

    let mut rng = StepRng::new(0, u64::MAX / 3);

    let mut db = Database::new();

    let terms = vec![
        Term::from((re("[Oo]2"), re("oxygen"))),
        Term::from((re("[Hh]2[Oo]"), re("water|(di)?hydrogen monoxide"))),
    ];

    let term = db.question(&terms, &mut rng).unwrap();
    assert_eq!(term.term.as_str(), "[Hh]2[Oo]");
    assert_eq!(term.definition.as_str(), "water|(di)?hydrogen monoxide");

    let term = db.question(&terms, &mut rng).unwrap();
    assert_eq!(term.term.as_str(), "[Oo]2");
    assert_eq!(term.definition.as_str(), "oxygen");

    assert_eq!(
        db.knowledge(term),
        Knowledge {
            level: KnowledgeLevel(0),
            safety_net: false
        }
    );
    db.record(term, true);
    assert_eq!(
        db.knowledge(term),
        Knowledge {
            level: KnowledgeLevel(1),
            safety_net: true
        }
    );
    db.record(term, false);
    assert_eq!(
        db.knowledge(term),
        Knowledge {
            level: KnowledgeLevel(1),
            safety_net: false
        }
    );
    db.record(term, false);
    assert_eq!(
        db.knowledge(term),
        Knowledge {
            level: KnowledgeLevel(0),
            safety_net: false
        }
    );
    for _ in 0..20 {
        db.record(term, true);
    }
    assert_eq!(
        db.knowledge(term),
        Knowledge {
            level: KnowledgeLevel(3),
            safety_net: true
        }
    );

    fn re(re: &str) -> Regex {
        Regex {
            matcher: MatchRegex::new(re).unwrap(),
            rand: RandRegex::compile(re, 3).unwrap(),
        }
    }
}

/// How well you know a term.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Knowledge {
    pub level: KnowledgeLevel,
    /// A safety net prevents you from going down a level if you get it wrong. It is replenished
    /// once you get a question right.
    pub safety_net: bool,
}

impl Knowledge {
    /// Get a question right.
    ///
    /// This puts you up a level and restores the safety net.
    #[must_use]
    pub fn correct(self) -> Self {
        Self {
            level: KnowledgeLevel::from(self.level.0 + 1),
            safety_net: true,
        }
    }
    /// Get a question wrong.
    ///
    /// If there is a safety net, it uses that up, otherwise it puts you a level down.
    #[must_use]
    pub fn incorrect(self) -> Self {
        Self {
            level: KnowledgeLevel(
                self.level
                    .0
                    .saturating_sub(if self.safety_net { 0 } else { 1 }),
            ),
            safety_net: false,
        }
    }
}

/// Integer ranging from 0 to 3.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(from = "u8", into = "u8")]
pub struct KnowledgeLevel(u8);

impl From<u8> for KnowledgeLevel {
    fn from(level: u8) -> Self {
        Self(level.min(3))
    }
}

impl From<KnowledgeLevel> for u8 {
    fn from(level: KnowledgeLevel) -> Self {
        level.0
    }
}

/// A term and defintion.
///
/// Confusingly, this library uses the word "term" to mean both the term, and the combination of
/// the term and definition.
///
/// It can be deserialized from and serializes into a tuple of two regexes.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(from = "(Regex, Regex)", into = "(Regex, Regex)")]
pub struct Term {
    /// The term of the term.
    pub term: Regex,
    /// The definition of the term.
    pub definition: Regex,
}

impl Term {
    /// Check whether an answer to a question using this term is correct.
    ///
    /// # Errors
    ///
    /// When the answer is not correct it returns the correct answer regex.
    pub fn check(&self, answer: &str) -> Result<(), &str> {
        if self
            .term
            .matcher
            .find(answer)
            .map_or(false, |m| m.start() == 0 && m.end() == answer.len())
        {
            Ok(())
        } else {
            Err(self.term.as_str())
        }
    }

    /// Get a prompt for the user. This is a randomly generated string that matches the definition
    /// regex.
    pub fn prompt(&self, rng: &mut impl Rng) -> String {
        self.definition.rand.sample(rng)
    }
}

impl From<(Regex, Regex)> for Term {
    fn from((term, definition): (Regex, Regex)) -> Self {
        Self { term, definition }
    }
}

impl From<Term> for (Regex, Regex) {
    fn from(term: Term) -> Self {
        (term.term, term.definition)
    }
}

/// A regex.
#[derive(Debug, Clone)]
pub struct Regex {
    matcher: MatchRegex,
    rand: RandRegex,
}

impl Regex {
    /// Get the regex as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.matcher.as_str()
    }
}

impl<'de> Deserialize<'de> for Regex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RegexVisitor;

        impl<'de> Visitor<'de> for RegexVisitor {
            type Value = Regex;
            fn expecting(&self, f: &mut Formatter) -> fmt::Result {
                f.write_str("a regex")
            }

            #[allow(clippy::match_wildcard_for_single_variants)]
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Regex {
                    matcher: MatchRegex::new(v).map_err(|e| match e {
                        regex::Error::Syntax(e) => {
                            de::Error::custom(format_args!("invalid regex: {}", e))
                        }
                        regex::Error::CompiledTooBig(size) => de::Error::custom(format_args!(
                            "regex too large, maximum size is {}",
                            size
                        )),
                        _ => de::Error::invalid_value(Unexpected::Str(v), &self),
                    })?,
                    rand: RandRegex::compile(v, 3).map_err(|e| match e {
                        rand_regex::Error::Anchor => de::Error::custom(format_args!(
                            "regex contains an anchor, which aren't supported"
                        )),
                        rand_regex::Error::Syntax(e) => {
                            de::Error::custom(format_args!("invalid regex: {}", e))
                        }
                    })?,
                })
            }
        }

        deserializer.deserialize_str(RegexVisitor)
    }
}

impl Serialize for Regex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl PartialEq<Regex> for Regex {
    fn eq(&self, other: &Regex) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Regex {}

impl Hash for Regex {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl AsRef<str> for Regex {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for Regex {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Display for Regex {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
