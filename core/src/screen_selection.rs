use crate::capture::sources::{
    list_displays, list_windows, windows_supported, ListedWindows, ShareableSource,
};
use crate::capture::thumbnail::Thumbnail;
use winit::monitor::MonitorHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSelectionTab {
    Screens,
    Windows,
}

/// One row/tile in the share picker.
#[derive(Debug, Clone)]
pub struct ScreenSelectionItemUi {
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail: Option<iced_core::image::Handle>,
}

/// Snapshot passed to the overlay renderer for the share picker.
#[derive(Debug, Clone)]
pub struct ScreenSelectionUi {
    pub tab: ScreenSelectionTab,
    pub windows_supported: bool,
    pub items: Vec<ScreenSelectionItemUi>,
    pub selected_index: usize,
    /// Shown on the Windows tab when the list is empty (e.g. missing permission).
    pub windows_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScreenSelectionState {
    pub tab: ScreenSelectionTab,
    pub displays: Vec<ShareableSource>,
    pub windows: Vec<ShareableSource>,
    pub selected_index: usize,
    pub windows_supported: bool,
    pub windows_hint: Option<String>,
}

impl ScreenSelectionState {
    pub fn new(monitors: &[MonitorHandle]) -> Self {
        let displays = list_displays(monitors);
        let windows_supported = windows_supported();
        let ListedWindows { windows, error } = if windows_supported {
            list_windows(monitors)
        } else {
            ListedWindows {
                windows: Vec::new(),
                error: Some("Window sharing is not available on this platform.".to_string()),
            }
        };
        Self {
            tab: ScreenSelectionTab::Screens,
            displays,
            windows,
            selected_index: 0,
            windows_supported,
            windows_hint: error,
        }
    }

    pub fn current_sources(&self) -> &[ShareableSource] {
        match self.tab {
            ScreenSelectionTab::Screens => &self.displays,
            ScreenSelectionTab::Windows => &self.windows,
        }
    }

    pub fn selected(&self) -> Option<&ShareableSource> {
        self.current_sources().get(self.selected_index)
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.current_sources().len();
        if len == 0 {
            self.selected_index = 0;
            return;
        }
        let current = self.selected_index as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        self.selected_index = next;
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        if index < self.current_sources().len() {
            self.selected_index = index;
            true
        } else {
            false
        }
    }

    /// Selects the display matching `monitor_content_id` when on the Screens tab.
    pub fn select_display_for_monitor(&mut self, monitor_content_id: u64) -> bool {
        if self.tab != ScreenSelectionTab::Screens {
            return false;
        }
        let Some(index) = self
            .displays
            .iter()
            .position(|source| source.monitor_content_id == monitor_content_id)
        else {
            return false;
        };
        self.selected_index = index;
        true
    }

    pub fn set_tab(&mut self, tab: ScreenSelectionTab) {
        if tab == ScreenSelectionTab::Windows && !self.windows_supported {
            return;
        }
        if self.tab != tab {
            self.tab = tab;
            self.selected_index = 0;
        }
    }

    pub fn toggle_tab(&mut self) {
        if !self.windows_supported {
            return;
        }
        let next = match self.tab {
            ScreenSelectionTab::Screens => ScreenSelectionTab::Windows,
            ScreenSelectionTab::Windows => ScreenSelectionTab::Screens,
        };
        self.set_tab(next);
    }

    pub fn ui_snapshot(&self) -> ScreenSelectionUi {
        ScreenSelectionUi {
            tab: self.tab,
            windows_supported: self.windows_supported,
            items: self
                .current_sources()
                .iter()
                .map(|source| {
                    let (title, subtitle) = match self.tab {
                        ScreenSelectionTab::Screens => {
                            (source.title.clone(), Some("Entire screen".to_string()))
                        }
                        ScreenSelectionTab::Windows => {
                            (source.title.clone(), source.app_name.clone())
                        }
                    };
                    ScreenSelectionItemUi {
                        title,
                        subtitle,
                        thumbnail: source.thumbnail.as_ref().map(Thumbnail::image_handle),
                    }
                })
                .collect(),
            selected_index: self.selected_index,
            windows_hint: self.windows_hint.clone(),
        }
    }
}
