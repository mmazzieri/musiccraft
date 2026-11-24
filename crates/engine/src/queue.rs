//! The play queue.

use musiccraft_library::Item;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

/// Songs in the order they play. `pos` is the current one.
#[derive(Debug, Clone, Default)]
pub struct Queue {
    items: Vec<Item>,
    /// The order before shuffling.
    base: Vec<Item>,
    pos: usize,
    shuffle: bool,
    pub repeat: Repeat,
    seed: u64,
}

impl Queue {
    pub fn new(shuffle: bool, repeat: Repeat) -> Queue {
        // Any odd number does; the id is different at each start.
        let seed = uuid::Uuid::new_v4().as_u64_pair().0 | 1;
        Queue { shuffle, repeat, seed, ..Queue::default() }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn current(&self) -> Option<&Item> {
        self.items.get(self.pos)
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// A random number below `n` (xorshift64).
    fn below(&mut self, n: usize) -> usize {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        usize::try_from(self.seed % (n.max(1) as u64)).unwrap_or(0)
    }

    fn shuffle_from(&mut self, from: usize) {
        for i in (from + 1..self.items.len()).rev() {
            let j = from + self.below(i + 1 - from);
            self.items.swap(i, j);
        }
    }

    /// Replaces the queue and goes to `start`.
    pub fn set(&mut self, items: Vec<Item>, start: usize) {
        self.base.clone_from(&items);
        self.items = items;
        self.pos = start.min(self.items.len().saturating_sub(1));
        if self.shuffle && !self.items.is_empty() {
            self.items.swap(0, self.pos);
            self.pos = 0;
            self.shuffle_from(1);
        }
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.shuffle_from((self.pos + 1).min(self.items.len()));
        } else {
            let current = self.current().cloned();
            self.items.clone_from(&self.base);
            self.pos = current.and_then(|c| self.items.iter().position(|i| *i == c)).unwrap_or(0);
        }
    }

    /// The position that follows the current one, if any. `ended`: the song played to its end.
    pub fn next_position(&self, ended: bool) -> Option<usize> {
        if self.items.is_empty() {
            None
        } else if ended && self.repeat == Repeat::One {
            Some(self.pos)
        } else if self.pos + 1 < self.items.len() {
            Some(self.pos + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else {
            None
        }
    }

    pub fn go(&mut self, pos: usize) -> bool {
        if pos >= self.items.len() {
            return false;
        }
        // A new round of a shuffled queue gets a new order.
        if pos == 0 && self.pos + 1 == self.items.len() && self.items.len() > 2 && self.shuffle {
            self.shuffle_from(0);
        }
        self.pos = pos;
        true
    }

    pub fn previous_position(&self) -> Option<usize> {
        self.pos.checked_sub(1)
    }

    /// Plays `more` right after the current song.
    pub fn insert_next(&mut self, more: Vec<Item>) {
        let at = if self.items.is_empty() { 0 } else { self.pos + 1 };
        self.base.extend(more.iter().cloned());
        self.items.splice(at..at, more);
    }

    pub fn append(&mut self, more: Vec<Item>) {
        self.base.extend(more.iter().cloned());
        self.items.extend(more);
    }

    /// Takes out an item that is not the current one.
    pub fn remove(&mut self, at: usize) {
        if at >= self.items.len() || at == self.pos {
            return;
        }
        let item = self.items.remove(at);
        if at < self.pos {
            self.pos -= 1;
        }
        if let Some(i) = self.base.iter().position(|b| *b == item) {
            self.base.remove(i);
        }
    }

    /// Moves an upcoming item one place earlier or later.
    pub fn shift(&mut self, at: usize, later: bool) {
        let to = if later { at + 1 } else { at.saturating_sub(1) };
        if at > self.pos && to > self.pos && to < self.items.len() && at < self.items.len() {
            self.items.swap(at, to);
        }
    }

    /// Takes out everything after the current song.
    pub fn clear_upcoming(&mut self) {
        self.items.truncate(self.pos + 1);
        self.base.clone_from(&self.items);
    }
}

#[cfg(test)]
mod tests {
    use super::{Queue, Repeat};
    use musiccraft_library::Item;

    fn items(n: usize) -> Vec<Item> {
        (0..n).map(|i| Item::local(&i.to_string())).collect()
    }

    fn ids(q: &Queue) -> Vec<String> {
        q.items().iter().map(|i| i.id.clone()).collect()
    }

    /// Shuffling keeps the current song playing and every song once; turning it off brings the
    /// original order back around the current song.
    #[test]
    fn shuffle_keeps_the_current_song_and_loses_none() {
        let mut q = Queue::new(false, Repeat::Off);
        q.set(items(50), 7);
        q.set_shuffle(true);
        assert_eq!(q.current().unwrap().id, "7");
        let mut sorted = ids(&q);
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 50);
        assert_ne!(ids(&q), items(50).into_iter().map(|i| i.id).collect::<Vec<_>>());
        q.go(20);
        let now = q.current().unwrap().id.clone();
        q.set_shuffle(false);
        assert_eq!(q.current().unwrap().id, now);
        assert_eq!(ids(&q)[..3], ["0", "1", "2"]);
    }

    /// A started song is first when shuffle is already on.
    #[test]
    fn a_shuffled_queue_starts_with_the_chosen_song() {
        let mut q = Queue::new(true, Repeat::Off);
        q.set(items(30), 12);
        assert_eq!((q.position(), q.current().unwrap().id.as_str()), (0, "12"));
    }

    /// Repeating one song holds only when it ends by itself, not when the person skips it.
    #[test]
    fn repeat_decides_what_follows() {
        let mut q = Queue::new(false, Repeat::One);
        q.set(items(2), 1);
        assert_eq!(q.next_position(true), Some(1));
        assert_eq!(q.next_position(false), None);
        q.repeat = Repeat::All;
        assert_eq!(q.next_position(true), Some(0));
        q.repeat = Repeat::Off;
        assert_eq!(q.next_position(true), None);
    }

    /// Editing the queue never changes which song is current.
    #[test]
    fn edits_keep_the_current_song() {
        let mut q = Queue::new(false, Repeat::Off);
        q.set(items(4), 2);
        q.insert_next(vec![Item::local("new")]);
        q.remove(0);
        q.remove(q.position());
        q.shift(q.position() + 1, true);
        assert_eq!(q.current().unwrap().id, "2");
        assert_eq!(ids(&q), ["1", "2", "3", "new"]);
        q.shift(q.position() + 1, false);
        assert_eq!(q.current().unwrap().id, "2");
    }
}
