use crate::{now_ms, save_json};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Column {
    Backlog,
    InProgress,
    Blocked,
    Done,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub component: String,
    pub column: Column,
    pub session_id: Option<String>,
    pub jobs: Vec<String>,
    pub updated_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub title: String,
    pub description: String,
    pub priority: String,
    pub component: String,
    pub column: Column,
    pub session_id: Option<String>,
}
pub struct Board {
    pub cards: Vec<Card>,
    path: PathBuf,
    pub storage_error: Option<String>,
    sequence: u64,
}
impl Board {
    pub fn new(data: &Path) -> Self {
        let path = data.join("board.json");
        let loaded = File::open(&path).and_then(|f| {
            serde_json::from_reader::<_, Vec<Card>>(f.take(4 * 1024 * 1024))
                .map_err(io::Error::other)
        });
        let (cards, storage_error) = match loaded {
            Ok(cards) if cards.len() <= 1000 => (cards, None),
            Ok(_) => (vec![], Some("Board exceeds 1000 cards".into())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], None),
            Err(e) => (vec![], Some(e.to_string())),
        };
        Self {
            cards,
            path,
            storage_error,
            sequence: 0,
        }
    }
    fn commit(&mut self, cards: Vec<Card>) -> io::Result<()> {
        if self.storage_error.is_some() {
            return Err(io::Error::other(
                "Board storage could not be loaded; recover board.json before editing",
            ));
        }
        if serde_json::to_vec(&cards)?.len() > 4 * 1024 * 1024 {
            return Err(io::Error::other("Board exceeds 4 MiB storage limit"));
        }
        save_json(&self.path, &cards)?;
        self.cards = cards;
        Ok(())
    }
    pub fn save(&mut self, id: Option<&str>, draft: Draft) -> io::Result<Card> {
        if draft.title.trim().is_empty()
            || draft.title.len() > 200
            || draft.description.len() > 16000
            || draft.component.len() > 100
            || draft.session_id.as_ref().is_some_and(|s| s.len() > 200)
            || !["Low", "Normal", "High", "Urgent"].contains(&draft.priority.as_str())
        {
            return Err(io::Error::other("Invalid card fields"));
        }
        let mut cards = self.cards.clone();
        let card = if let Some(id) = id {
            cards
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| io::Error::other("Card no longer exists"))?
        } else {
            if cards.len() >= 1000 {
                return Err(io::Error::other("Board is full"));
            }
            self.sequence += 1;
            cards.push(Card {
                id: format!("card-{}-{}-{}", std::process::id(), now_ms(), self.sequence),
                title: String::new(),
                description: String::new(),
                priority: String::new(),
                component: String::new(),
                column: Column::Backlog,
                session_id: None,
                jobs: vec![],
                updated_ms: 0,
            });
            cards.last_mut().unwrap()
        };
        card.title = draft.title.trim().into();
        card.description = draft.description;
        card.priority = draft.priority;
        card.component = draft.component;
        card.column = draft.column;
        card.session_id = draft.session_id.filter(|s| !s.is_empty());
        card.updated_ms = now_ms();
        let result = card.clone();
        self.commit(cards)?;
        Ok(result)
    }
    pub fn delete(&mut self, id: &str) -> io::Result<()> {
        let mut cards = self.cards.clone();
        let index = cards
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| io::Error::other("Unknown card"))?;
        cards.remove(index);
        self.commit(cards)
    }
    pub fn move_card(&mut self, id: &str, column: Column, before: Option<&str>) -> io::Result<()> {
        let mut cards = self.cards.clone();
        let index = cards
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| io::Error::other("Unknown card"))?;
        let mut card = cards.remove(index);
        card.column = column.clone();
        card.updated_ms = now_ms();
        let index = match before {
            Some(before) => cards
                .iter()
                .position(|c| c.id == before && c.column == column)
                .ok_or_else(|| io::Error::other("Invalid drop target"))?,
            None => cards.len(),
        };
        cards.insert(index, card);
        self.commit(cards)
    }
    pub fn link(&mut self, id: &str, job: &str) -> io::Result<()> {
        let mut cards = self.cards.clone();
        let card = cards
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| io::Error::other("Unknown card"))?;
        if !card.jobs.iter().any(|j| j == job) {
            if card.jobs.len() >= 100 {
                return Err(io::Error::other("Card already has 100 linked jobs"));
            }
            card.jobs.push(job.into());
            card.updated_ms = now_ms();
        }
        self.commit(cards)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_order_and_missing_links() {
        let path = std::env::temp_dir().join(format!("board-{}", now_ms()));
        let mut board = Board::new(&path);
        let draft = |title: &str| Draft {
            title: title.into(),
            description: "details".into(),
            priority: "Normal".into(),
            component: "renderer".into(),
            column: Column::Backlog,
            session_id: Some("session".into()),
        };
        let first = board.save(None, draft("first")).unwrap();
        let second = board.save(None, draft("second")).unwrap();
        board.link(&first.id, "expired-job").unwrap();
        board
            .move_card(&second.id, Column::Backlog, Some(&first.id))
            .unwrap();
        let restored = Board::new(&path);
        assert_eq!(restored.cards[0].id, second.id);
        assert_eq!(restored.cards[1].jobs, vec!["expired-job"]);
        board.move_card(&first.id, Column::Done, None).unwrap();
        board.delete(&second.id).unwrap();
        assert_eq!(Board::new(&path).cards.len(), 1);
    }
    #[test]
    fn failed_save_keeps_memory() {
        let path = std::env::temp_dir().join(format!("board-fail-{}", now_ms()));
        std::fs::create_dir_all(path.join("board.json")).unwrap();
        let mut board = Board::new(&path);
        assert!(
            board
                .save(
                    None,
                    Draft {
                        title: "Task".into(),
                        description: String::new(),
                        priority: "Normal".into(),
                        component: String::new(),
                        column: Column::Backlog,
                        session_id: None
                    }
                )
                .is_err()
        );
        assert!(board.cards.is_empty());
    }
}
