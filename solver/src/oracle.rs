use crate::data::{Character, find_by_name};
use crate::feedback::{Feedback, Table};
use crate::supabase::Supabase;
use anyhow::Context;
use chrono::NaiveDate;

/// Anything that answers guesses with feedback. The solver only ever sees this.
pub trait Game {
    fn guess(&mut self, guess: usize) -> Feedback;
}

/// Holds the secret target and answers guesses, nothing else.
/// The target is private and has no accessor: this is the no-cheating barrier.
pub struct Oracle<'a> {
    table: &'a Table,
    target: usize,
}

impl<'a> Oracle<'a> {
    pub fn new(table: &'a Table, target: usize) -> Self {
        Self { table, target }
    }
}

impl Game for Oracle<'_> {
    fn guess(&mut self, guess: usize) -> Feedback {
        self.table.get(guess, self.target)
    }
}

impl<'a> Oracle<'a> {
    /// Fetches today's target the way the web app does and seals it. Errors never reveal the name.
    pub fn daily(
        table: &'a Table,
        db: &[Character],
        supabase: &Supabase,
        today: NaiveDate,
    ) -> anyhow::Result<Self> {
        let name = supabase.today_row(today)?;
        let target = find_by_name(db, &name)
            .context("today's character is not in the character DB (DB drift?)")?;
        Ok(Self::new(table, target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse;
    use crate::feedback::WIN;

    #[test]
    fn answers_with_the_target_feedback() {
        let db = parse(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../src/data/characters.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let table = Table::new(&db);
        for (g, t) in [(0, 0), (3, 7), (100, 5), (42, 42)] {
            let fb = Oracle::new(&table, t).guess(g);
            assert_eq!(fb, table.get(g, t));
            assert_eq!(fb == WIN, g == t);
        }
    }

    #[test]
    fn daily_unknown_target_is_an_error_without_the_name() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/rest/v1/daily_index")
            .match_query(mockito::Matcher::Any)
            .with_body(r#"[{"day":"2026-10-07","character_name":"Secret Person"}]"#)
            .create();
        let db = crate::data::parse(r#"[{"name":"A","anime":{"name":"X","year":2000,"studio":"S"},"age":1,"height":1,"hair_color":"R","sex":null,"seiyuu":"V"}]"#).unwrap();
        let table = Table::new(&db);
        let err = Oracle::daily(
            &table,
            &db,
            &Supabase::new(&server.url(), "k"),
            "2026-10-07".parse().unwrap(),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(!err.contains("Secret"), "{err}");
    }
}
