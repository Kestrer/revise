//! The database behind `revise`.
#![warn(clippy::all, clippy::pedantic)]
#![warn(missing_docs)]
#![allow(
    clippy::items_after_statements,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc
)]

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use bincode::Options as _;
use rusqlite::types::ToSql;
use rusqlite::OptionalExtension as _;
use serde::ser::{Serialize, Serializer};
use thiserror::Error;

/// The database of how well you know which cards.
#[derive(Debug)]
pub struct Database {
    connection: rusqlite::Connection,
}

impl Database {
    /// Open a database with the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, OpenError> {
        rusqlite::Connection::open(&path)
            .and_then(Self::new)
            .map_err(|inner| OpenError {
                inner,
                path: path.as_ref().to_owned(),
            })
    }

    /// Open an in-memory database.
    pub fn open_in_memory() -> Result<Self, OpenInMemoryError> {
        rusqlite::Connection::open_in_memory()
            .and_then(Self::new)
            .map_err(|inner| OpenInMemoryError { inner })
    }

    fn new(connection: rusqlite::Connection) -> rusqlite::Result<Self> {
        connection.execute_batch(
            "\
                CREATE TABLE IF NOT EXISTS v1 (
                    card BLOB NOT NULL PRIMARY KEY,
                    knowledge_level INTEGER NOT NULL CHECK(knowledge_level >= 1 AND knowledge_level <= 3),
                    safety_net INTEGER NOT NULL CHECK(safety_net = 0 OR safety_net = 1)
                ) WITHOUT ROWID
            ",
        )?;
        Ok(Self { connection })
    }

    /// Get how well known a card is.
    pub fn knowledge(&self, card: &CardKey) -> Result<Knowledge, GetKnowledgeError> {
        self.connection
            .query_row(
                "SELECT knowledge_level,safety_net FROM v1 WHERE card = ?",
                [card.as_sql()],
                |row| {
                    Ok(Knowledge {
                        level: KnowledgeLevel::new(row.get_unwrap(0)).unwrap(),
                        safety_net: row.get_unwrap(1),
                    })
                },
            )
            .map(|knowledge| {
                assert_ne!(knowledge.level.get(), 0);
                knowledge
            })
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(|inner| GetKnowledgeError { inner })
    }

    /// Get how well known a set of cards are.
    pub fn knowledge_all<'a, I>(
        &self,
        cards: I,
    ) -> Result<impl Iterator<Item = Knowledge> + 'a, GetKnowledgeError>
    where
        I: IntoIterator<Item = &'a CardKey>,
        I::IntoIter: ExactSizeIterator + Clone + 'a,
    {
        (|| {
            let cards = cards.into_iter();
            let cards_len = cards.len();

            let mut sql =
                "SELECT card,knowledge_level,safety_net FROM v1 WHERE card IN (".to_owned();
            if cards_len != 0 {
                sql.push('?');
            }
            for _ in 1..cards_len {
                sql.push_str(",?");
            }
            sql.push(')');

            let result = self
                .connection
                .prepare(&sql)?
                .query_map(
                    rusqlite::params_from_iter(cards.clone().map(CardKey::as_sql)),
                    |row| {
                        Ok((
                            CardKey::from_sql(row.get_unwrap(0)),
                            Knowledge {
                                level: KnowledgeLevel::new(row.get_unwrap(1)).unwrap(),
                                safety_net: row.get_unwrap(2),
                            },
                        ))
                    },
                )?
                .collect::<Result<HashMap<_, _>, _>>()?;

            Ok(cards.map(move |card| result.get(&card).copied().unwrap_or_default()))
        })()
        .map_err(|inner| GetKnowledgeError { inner })
    }

    /// Set the knowledge of a card.
    pub fn set_knowledge(
        &mut self,
        card: &CardKey,
        knowledge: Knowledge,
    ) -> Result<(), SetKnowledgeError> {
        if knowledge.level.get() == 0 {
            self.connection
                .execute("DELETE FROM v1 WHERE card = ?", [card.as_sql()])
                .map_err(SetKnowledgeErrorKind::Remove)?;
        } else {
            self.connection
                .execute(
                    "INSERT INTO v1 VALUES (?, ?, ?) ON CONFLICT(card) DO UPDATE SET knowledge_level = ?2, safety_net = ?3",
                    rusqlite::params![card.as_sql(), knowledge.level.get(), knowledge.safety_net],
                )
                .map_err(SetKnowledgeErrorKind::Insert)?;
        }
        Ok(())
    }

    /// Record the answer to a question as correct.
    pub fn record_correct(&mut self, card: &CardKey) -> Result<(), RecordCorrectError> {
        self.connection
            .execute(
                "INSERT INTO v1 VALUES (?, 1, true) ON CONFLICT(card) DO UPDATE SET knowledge_level=min(knowledge_level+1, 3), safety_net=true",
                [card.as_sql()],
            )
            .map_err(|inner| RecordCorrectError { inner })?;
        Ok(())
    }

    /// Record the answer to a question as incorrect.
    pub fn record_incorrect(&mut self, card: &CardKey) -> Result<(), RecordIncorrectError> {
        (|| {
            let transaction = self.connection.transaction()?;
            transaction.execute(
                "DELETE FROM v1 WHERE card = ? AND knowledge_level = 1 AND safety_net = false",
                [card.as_sql()],
            )?;
            transaction.execute(
                "UPDATE v1 SET knowledge_level = IIF(safety_net, knowledge_level, knowledge_level - 1), safety_net = false WHERE card = ?",
                [card.as_sql()],
            )?;
            transaction.commit()?;
            Ok(())
        })()
        .map_err(|inner| RecordIncorrectError { inner})
    }

    /// Get the distribution of how well known the given list of cards are.
    ///
    /// The cards in `cards` should all be unique.
    pub fn level_distribution<'a, I>(
        &self,
        cards: I,
    ) -> Result<[usize; 4], GetLevelDistributionError>
    where
        I: IntoIterator<Item = &'a CardKey>,
        I::IntoIter: ExactSizeIterator,
    {
        (|| {
            let cards = cards.into_iter();
            let cards_len = cards.len();

            let mut sql = "SELECT knowledge_level FROM v1 WHERE card IN (".to_owned();
            if cards_len != 0 {
                sql.push('?');
            }
            for _ in 1..cards_len {
                sql.push_str(",?");
            }
            sql.push(')');

            let mut retrieved = 0;

            let mut distribution = self
                .connection
                .prepare(&sql)?
                .query_map(
                    rusqlite::params_from_iter(cards.map(CardKey::as_sql)),
                    |row| Ok(KnowledgeLevel::new(row.get_unwrap(0)).unwrap()),
                )?
                .try_fold(
                    [0; 4],
                    |mut distribution, knowledge_level| -> rusqlite::Result<_> {
                        distribution[usize::from(knowledge_level?.get())] += 1;
                        retrieved += 1;
                        Ok(distribution)
                    },
                )?;

            assert_eq!(distribution[0], 0);
            distribution[0] = cards_len - retrieved;

            Ok(distribution)
        })()
        .map_err(|inner| GetLevelDistributionError { inner })
    }

    /// Set the knowledge of all terms to level 2 if they are all level 3.
    ///
    /// The cards in `cards` should all be unique.
    pub fn make_incomplete<'a, I>(&mut self, cards: I) -> Result<(), MakeIncompleteError>
    where
        I: IntoIterator<Item = &'a CardKey>,
        I::IntoIter: ExactSizeIterator + Clone,
    {
        (|| {
            let cards = cards.into_iter();
            let cards_len = cards.len();

            let mut query_sql =
                "SELECT COUNT(*) FROM v1 WHERE knowledge_level = 3 AND card IN (".to_owned();
            let mut sql =
                "UPDATE v1 SET knowledge_level = 2, safety_net = false WHERE card IN (".to_owned();
            if cards_len != 0 {
                query_sql.push('?');
                sql.push('?');
            }
            for _ in 1..cards_len {
                query_sql.push_str(",?");
                sql.push_str(",?");
            }
            query_sql.push(')');
            sql.push(')');

            let transaction = self
                .connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

            let learnt: usize = transaction.query_row(
                &query_sql,
                rusqlite::params_from_iter(cards.clone().map(CardKey::as_sql)),
                |row| Ok(row.get_unwrap(0)),
            )?;

            if learnt == cards.len() {
                transaction
                    .execute(&sql, rusqlite::params_from_iter(cards.map(CardKey::as_sql)))?;
            }

            transaction.commit()?;

            Ok(())
        })()
        .map_err(|inner| MakeIncompleteError { inner })
    }
}

/// Error in [`Database::open`].
#[derive(Debug, Error)]
#[error("failed to open database at `{}`", path.display())]
pub struct OpenError {
    #[source]
    inner: rusqlite::Error,
    path: PathBuf,
}

/// Error in [`Database::open_in_memory`].
#[derive(Debug, Error)]
#[error("failed to open in-memory database")]
pub struct OpenInMemoryError {
    #[source]
    inner: rusqlite::Error,
}

/// Error in [`Database::knowledge`].
#[derive(Debug, Error)]
#[error("failed to retrieve knowledge of a card")]
pub struct GetKnowledgeError {
    #[source]
    inner: rusqlite::Error,
}

/// Error in [`Database::set_knowledge`].
#[derive(Debug, Error)]
#[error("failed to set the knowledge of a card")]
pub struct SetKnowledgeError(
    #[source]
    #[from]
    SetKnowledgeErrorKind,
);

#[derive(Debug, Error)]
enum SetKnowledgeErrorKind {
    #[error("failed to remove card from database")]
    Remove(#[source] rusqlite::Error),
    #[error("failed to add or update card in database")]
    Insert(#[source] rusqlite::Error),
}

/// Error in [`Database::record_correct`].
#[derive(Debug, Error)]
#[error("failed to record card as correct")]
pub struct RecordCorrectError {
    #[source]
    inner: rusqlite::Error,
}

/// Error in [`Database::record_incorrect`].
#[derive(Debug, Error)]
#[error("failed to record card as incorrect")]
pub struct RecordIncorrectError {
    #[source]
    inner: rusqlite::Error,
}

/// Error in [`Database::level_distribution`].
#[derive(Debug, Error)]
#[error("failed to get distribution of card knowledge")]
pub struct GetLevelDistributionError {
    #[source]
    inner: rusqlite::Error,
}

/// Error in [`Database::make_incomplete`].
#[derive(Debug, Error)]
#[error("failed to make cards incomplete")]
pub struct MakeIncompleteError {
    #[source]
    inner: rusqlite::Error,
}

#[test]
fn test_database() {
    use maplit::btreeset;

    let mut db = Database::open_in_memory().unwrap();

    let cards = [
        CardKey::new(&btreeset!("t"), &btreeset!("definition")),
        CardKey::new(&btreeset!("a", "b"), &btreeset!("c", "d", "e")),
    ];

    for card in &cards {
        assert_eq!(db.knowledge(card).unwrap(), Knowledge::default());

        for level in (0..=3).rev() {
            let level = KnowledgeLevel(level);
            for safety_net in [false, true] {
                let knowledge = Knowledge { level, safety_net };
                db.set_knowledge(&card, knowledge).unwrap();
                let stored = db.knowledge(&card).unwrap();
                assert_eq!(stored.level.get(), knowledge.level.get());
                if knowledge.level.get() == 0 {
                    assert!(!stored.safety_net);
                } else {
                    assert_eq!(stored.safety_net, knowledge.safety_net);
                }
            }
        }

        for level in 1..8 {
            db.record_correct(card).unwrap();
            let knowledge = db.knowledge(card).unwrap();
            assert_eq!(knowledge.level.get(), std::cmp::min(level, 3));
            assert!(knowledge.safety_net);
        }

        #[allow(clippy::cast_sign_loss)]
        for level in (-5_i8..=3).rev() {
            db.record_incorrect(card).unwrap();
            let knowledge = db.knowledge(card).unwrap();
            assert_eq!(knowledge.level.get(), std::cmp::max(level, 0) as u8);
            assert!(!knowledge.safety_net);
        }
    }

    let assert_knowledge = |db: &Database, levels: [(u8, bool); 2]| {
        for (card, (level, safety_net)) in cards.iter().zip(levels) {
            let knowledge = db.knowledge(card).unwrap();
            assert_eq!(knowledge.level.get(), level);
            assert_eq!(knowledge.safety_net, safety_net);
        }

        for (knowledge, (level, safety_net)) in db.knowledge_all(&cards).unwrap().zip(levels) {
            assert_eq!(knowledge.level.get(), level);
            assert_eq!(knowledge.safety_net, safety_net);
        }

        let distribution = db.level_distribution(&cards).unwrap();
        for i in 0..4 {
            let expected = levels.iter().filter(|(level, _)| *level == i).count();
            assert_eq!(distribution[usize::from(i)], expected);
        }
    };

    assert_knowledge(&db, [(0, false), (0, false)]);

    db.make_incomplete(&cards).unwrap();
    assert_knowledge(&db, [(0, false), (0, false)]);

    db.record_correct(&cards[0]).unwrap();
    assert_knowledge(&db, [(1, true), (0, false)]);

    db.record_correct(&cards[0]).unwrap();
    assert_knowledge(&db, [(2, true), (0, false)]);

    db.record_correct(&cards[1]).unwrap();
    assert_knowledge(&db, [(2, true), (1, true)]);

    db.record_correct(&cards[1]).unwrap();
    assert_knowledge(&db, [(2, true), (2, true)]);

    db.record_correct(&cards[1]).unwrap();
    assert_knowledge(&db, [(2, true), (3, true)]);

    db.make_incomplete(&cards).unwrap();
    assert_knowledge(&db, [(2, true), (3, true)]);

    db.record_correct(&cards[0]).unwrap();
    assert_knowledge(&db, [(3, true), (3, true)]);

    db.make_incomplete(&cards).unwrap();
    assert_knowledge(&db, [(2, false), (2, false)]);
}

/// A unique key that every card has.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CardKey(Vec<u8>);

impl CardKey {
    /// Compute the card key for the given card's terms and definitions.
    ///
    /// # Panics
    ///
    /// Panics if either set is empty.
    #[must_use]
    pub fn new<T, D>(terms: &BTreeSet<T>, definitions: &BTreeSet<D>) -> Self
    where
        T: AsRef<[u8]>,
        D: AsRef<[u8]>,
    {
        assert!(!terms.is_empty());
        assert!(!definitions.is_empty());

        struct SerializeSet<'a, T>(&'a BTreeSet<T>);
        impl<T: AsRef<[u8]>> Serialize for SerializeSet<'_, T> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_seq(self.0.iter().map(AsRef::as_ref))
            }
        }

        let bincode = bincode::DefaultOptions::new()
            .serialize(&(SerializeSet(terms), SerializeSet(definitions)))
            .unwrap();

        Self(bincode)
    }

    fn as_sql(&self) -> impl ToSql + '_ {
        &self.0
    }

    fn from_sql(sql: Vec<u8>) -> Self {
        Self(sql)
    }
}

/// How well you know a card.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Knowledge {
    /// The level from 0 to 3.
    pub level: KnowledgeLevel,
    /// A safety net prevents you from going down a level if you get it wrong. It is replenished
    /// once you get a question right.
    pub safety_net: bool,
}

/// Integer ranging from 0 to 3.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KnowledgeLevel(u8);

impl KnowledgeLevel {
    /// Create a new `KnowledgeLevel`. Returns `None` if the value is >3.
    #[must_use]
    pub const fn new(value: u8) -> Option<Self> {
        if value <= 3 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Get the knowledge level as a `u8`.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}
