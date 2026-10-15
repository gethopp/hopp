#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppVeilState {
    pub applications: Vec<TrackedApplication>,
    pub control_panels_ids: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedApplication {
    pub bundle_id: String,
    pub windows: Vec<TrackedWindow>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedWindow {
    pub window_id: u32,
    pub owner_pid: i32,
    pub visible: bool,
}

impl AppVeilState {
    pub fn capture_policy_eq(&self, other: &Self) -> bool {
        self.control_panels_ids == other.control_panels_ids
            && self.applications.len() == other.applications.len()
            && self.applications.iter().all(|application| {
                other.applications.iter().any(|candidate| {
                    application.bundle_id == candidate.bundle_id
                        && application
                            .windows
                            .iter()
                            .filter(|window| window.visible)
                            .count()
                            == candidate
                                .windows
                                .iter()
                                .filter(|window| window.visible)
                                .count()
                        && application
                            .windows
                            .iter()
                            .filter(|window| window.visible)
                            .all(|window| candidate.windows.contains(window))
                })
            })
    }

    pub fn new(bundle_ids: Vec<String>) -> Self {
        Self {
            applications: bundle_ids
                .into_iter()
                .map(|bundle_id| TrackedApplication {
                    bundle_id,
                    windows: Vec::new(),
                })
                .collect(),
            control_panels_ids: Vec::new(),
        }
    }

    pub fn update_windows(&mut self, applications: Vec<TrackedApplication>) {
        for mut current in applications {
            let Some(application) = self
                .applications
                .iter_mut()
                .find(|application| application.bundle_id == current.bundle_id)
            else {
                continue;
            };
            for window in &mut current.windows {
                window.visible = application.windows.iter().any(|previous| {
                    previous.window_id == window.window_id
                        && previous.owner_pid == window.owner_pid
                        && previous.visible
                });
            }
            application.windows = current.windows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_policy_ignores_hidden_inventory_but_tracks_visibility_and_owners() {
        let original = AppVeilState::new(vec!["protected".into()]);
        let mut updated = original.clone();
        updated.applications[0].windows.push(TrackedWindow {
            window_id: 1,
            owner_pid: 10,
            visible: false,
        });
        assert!(original.capture_policy_eq(&updated));
        updated.applications[0].windows[0].visible = true;
        assert!(!original.capture_policy_eq(&updated));
        let visible = updated.clone();
        updated.applications[0].windows.push(TrackedWindow {
            window_id: 2,
            owner_pid: 10,
            visible: false,
        });
        updated.applications[0].windows.reverse();
        assert!(visible.capture_policy_eq(&updated));
        updated.applications[0].windows[1].owner_pid = 11;
        assert!(!visible.capture_policy_eq(&updated));
        updated = visible.clone();
        updated.control_panels_ids.push(5);
        assert!(!visible.capture_policy_eq(&updated));
        updated = visible.clone();
        updated.applications.clear();
        assert!(!visible.capture_policy_eq(&updated));
        assert!(!visible.capture_policy_eq(&original));
    }

    #[test]
    fn inventory_preserves_choices_and_hides_new_or_reused_windows() {
        let mut state = AppVeilState::new(vec!["protected".into()]);
        let window = |window_id, owner_pid, visible| TrackedWindow {
            window_id,
            owner_pid,
            visible,
        };
        state.applications[0].windows = vec![
            window(1, 10, true),
            window(2, 10, true),
            window(3, 10, true),
        ];
        let windows = vec![
            window(1, 10, false),
            window(2, 20, false),
            window(4, 10, true),
        ];
        let inventory_allocation = windows.as_ptr();
        state.update_windows(vec![TrackedApplication {
            bundle_id: "protected".into(),
            windows,
        }]);
        assert_eq!(state.applications[0].windows.as_ptr(), inventory_allocation);
        assert_eq!(
            state.applications[0]
                .windows
                .iter()
                .map(|window| (window.window_id, window.visible))
                .collect::<Vec<_>>(),
            vec![(1, true), (2, false), (4, false)]
        );
        state.update_windows(vec![TrackedApplication {
            bundle_id: "protected".into(),
            windows: Vec::new(),
        }]);
        assert_eq!(state.applications.len(), 1);
        assert!(state.applications[0].windows.is_empty());
    }
}
