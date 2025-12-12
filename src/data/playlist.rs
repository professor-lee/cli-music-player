use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PlaylistItem {
    pub path: PathBuf,
    pub title: String,
}

#[derive(Debug, Default, Clone)]
pub struct Playlist {
    pub items: Vec<PlaylistItem>,
    pub selected: usize,
    pub current: Option<usize>,
}

impl Playlist {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clamp_selected(&mut self) {
        if self.items.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.items.len() {
            self.selected = self.items.len() - 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1).min(self.items.len() - 1);
        }
    }

    pub fn current_path(&self) -> Option<&PathBuf> {
        self.current.and_then(|i| self.items.get(i)).map(|it| &it.path)
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.items.get(self.selected).map(|it| &it.path)
    }

    pub fn set_current_selected(&mut self) {
        if self.items.is_empty() {
            self.current = None;
        } else {
            self.current = Some(self.selected);
        }
    }

    pub fn next_index_sequence(&self) -> Option<usize> {
        let cur = self.current?;
        if self.items.is_empty() {
            None
        } else {
            Some((cur + 1) % self.items.len())
        }
    }

    pub fn prev_index_sequence(&self) -> Option<usize> {
        let cur = self.current?;
        if self.items.is_empty() {
            None
        } else {
            Some((cur + self.items.len() - 1) % self.items.len())
        }
    }
}
