use anyhow::{Context, ensure};
use serde::Deserialize;

pub const DEFAULT_CHARACTERS_URL: &str = "https://raw.githubusercontent.com/Hewigkeyt/anime_wordle/refs/heads/master/src/data/characters.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::One(s) => std::slice::from_ref(s),
            Self::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Anime {
    pub name: String,
    pub year: f64,
    pub studio: OneOrMany,
}

// `deserialize_with = "Option::deserialize"` makes the field required while still allowing null:
// JS `buildResult` treats a missing field (undefined) differently from null.
#[derive(Debug, Clone, Deserialize)]
pub struct Character {
    pub name: String,
    pub anime: Anime,
    #[serde(deserialize_with = "Option::deserialize")]
    pub age: Option<f64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub height: Option<f64>,
    pub hair_color: OneOrMany,
    #[serde(deserialize_with = "Option::deserialize")]
    pub sex: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub seiyuu: Option<String>,
}

pub fn parse(json: &str) -> anyhow::Result<Vec<Character>> {
    let db: Vec<Character> = serde_json::from_str(json).context("invalid characters.json")?;
    ensure!(
        !db.is_empty() && db.len() <= 512,
        "expected 1..=512 characters, got {}",
        db.len()
    );
    Ok(db)
}

pub fn load(source: &str) -> anyhow::Result<Vec<Character>> {
    let json = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::blocking::get(source)?.error_for_status()?.text()?
    } else {
        std::fs::read_to_string(source).with_context(|| format!("reading {source}"))?
    };
    parse(&json)
}

/// First match in DB order, like `db.find((c) => c.name === name)` in the web app.
pub fn find_by_name(db: &[Character], name: &str) -> Option<usize> {
    db.iter().position(|c| c.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: &str = r#"{"name":"A","anime":{"name":"X","year":2000,"studio":["S1","S2"]},"age":null,"height":105.9,"hair_color":["Red","Blue"],"sex":null,"seiyuu":"V"}"#;

    fn repo_db() -> Vec<Character> {
        parse(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../src/data/characters.json"
            ))
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn parses_repo_db() {
        let db = repo_db();
        assert!(db.len() > 400);
        let luffy = &db[find_by_name(&db, "Monkey D. Luffy").unwrap()];
        assert_eq!(luffy.anime.name, "One Piece");
        assert_eq!(luffy.hair_color.as_slice(), ["Black"]);
    }

    #[test]
    fn multi_value_nulls_and_floats() {
        let db = parse(&format!("[{ROW}]")).unwrap();
        assert_eq!(db[0].anime.studio.as_slice(), ["S1", "S2"]);
        assert_eq!(
            (db[0].age, db[0].height, db[0].sex.as_deref()),
            (None, Some(105.9), None)
        );
    }

    #[test]
    fn missing_nullable_field_is_an_error_not_null() {
        for field in ["\"age\":null,", "\"sex\":null,"] {
            assert!(
                parse(&format!("[{}]", ROW.replace(field, ""))).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn rejects_empty_and_oversized_db() {
        assert!(parse("[]").is_err());
        assert!(parse(&format!("[{}]", vec![ROW; 513].join(","))).is_err());
    }

    #[test]
    fn find_by_name_returns_first_match() {
        let db = repo_db();
        assert_eq!(
            find_by_name(&db, "Kagura"),
            db.iter().position(|c| c.name == "Kagura")
        );
    }
}
