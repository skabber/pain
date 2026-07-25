//! Group-aware input routing: keyboard chords to app-level actions, and
//! broadcast target resolution. See `.waypoint/design/input-router.md`.

mod keymap;

use std::collections::{HashMap, HashSet};

use layout::PaneId;

pub use keymap::{Action, Chord, Key, Keymap};

/// How broadcast input fans out from the focused pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BroadcastMode {
    #[default]
    Off,
    Group,
    All,
}

/// Identifies a broadcast group by its user-given name. Names are how the
/// user creates/selects groups (typed for a new one, or picked from a list
/// of existing ones — see the context menu design), so the name *is* the
/// identity rather than a separate id the UI would have to keep mapped to
/// one; two panes assigned the same name are in the same group.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(pub String);

/// Resolves keyboard chords to actions and broadcast input to its targets.
pub struct Router {
    pub keymap: Keymap,
    pub broadcast_mode: BroadcastMode,
    groups: HashMap<GroupId, HashSet<PaneId>>,
    pane_group: HashMap<PaneId, GroupId>,
}

impl Router {
    /// A router using Terminator's verified default keybindings.
    pub fn new() -> Self {
        Self {
            keymap: Keymap::terminator_defaults(),
            broadcast_mode: BroadcastMode::default(),
            groups: HashMap::new(),
            pane_group: HashMap::new(),
        }
    }

    /// Looks up the action bound to `chord`, if any.
    pub fn resolve(&self, chord: Chord) -> Option<Action> {
        self.keymap.lookup(chord)
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    /// Assigns `pane` to the named group, creating it if it doesn't exist
    /// yet. Removes `pane` from whatever group it was previously in first
    /// (deleting that group if `pane` was its last member) — a pane
    /// belongs to at most one group at a time, matching the UI (picking a
    /// different group for an already-grouped pane moves it, it doesn't
    /// add a second membership).
    pub fn assign_to_group(&mut self, pane: PaneId, name: String) {
        self.remove_from_group(pane);
        let group = GroupId(name);
        self.groups.entry(group.clone()).or_default().insert(pane);
        self.pane_group.insert(pane, group);
    }

    /// Removes `pane` from its current group, if any, deleting the group
    /// entirely once it has no members left.
    pub fn remove_from_group(&mut self, pane: PaneId) {
        if let Some(group) = self.pane_group.remove(&pane)
            && let Some(members) = self.groups.get_mut(&group)
        {
            members.remove(&pane);
            if members.is_empty() {
                self.groups.remove(&group);
            }
        }
    }

    /// The group `pane` belongs to, if any.
    pub fn group_of(&self, pane: PaneId) -> Option<GroupId> {
        self.pane_group.get(&pane).cloned()
    }

    /// Every group that currently has at least one member, for populating
    /// a "select an existing group" list — sorted for a stable UI order.
    pub fn group_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.groups.keys().map(|g| g.0.as_str()).collect();
        names.sort_unstable();
        names
    }

    /// Removes `pane` from any group it belongs to — called when a pane
    /// closes, so a stale entry can't resurface if a future pane reuses
    /// nothing (ids are never reused, but this keeps the maps from growing
    /// unboundedly across a long session). Same behavior as
    /// `remove_from_group`; kept as a separate name so call sites read as
    /// "this pane is gone" rather than "the user un-grouped this pane".
    pub fn forget_pane(&mut self, pane: PaneId) {
        self.remove_from_group(pane);
    }

    /// Resolves which panes should receive input typed into `focused`,
    /// given the current broadcast mode. `all_panes` is every pane
    /// currently in the layout (for `BroadcastMode::All`).
    pub fn broadcast_targets(&self, focused: PaneId, all_panes: &[PaneId]) -> HashSet<PaneId> {
        match self.broadcast_mode {
            BroadcastMode::Off => HashSet::from([focused]),
            BroadcastMode::All => all_panes.iter().copied().collect(),
            BroadcastMode::Group => match self.group_of(focused) {
                Some(group) => self
                    .groups
                    .get(&group)
                    .cloned()
                    .unwrap_or_else(|| HashSet::from([focused])),
                None => HashSet::from([focused]),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: u64) -> PaneId {
        // `layout::PaneId` has no public constructor (assigned only by
        // `Layout`); round-trip through a real layout to get valid ids.
        let (mut layout, root) = layout::Layout::new();
        let mut last = root;
        for _ in 0..id {
            last = layout.split(last, layout::Orientation::Horizontal).unwrap();
        }
        last
    }

    #[test]
    fn broadcast_off_targets_only_focused() {
        let router = Router::new();
        let all = vec![pane(0), pane(1)];
        assert_eq!(router.broadcast_targets(pane(0), &all), HashSet::from([pane(0)]));
    }

    #[test]
    fn broadcast_all_targets_every_pane() {
        let mut router = Router::new();
        router.broadcast_mode = BroadcastMode::All;
        let all = vec![pane(0), pane(1), pane(2)];
        assert_eq!(
            router.broadcast_targets(pane(0), &all),
            HashSet::from([pane(0), pane(1), pane(2)])
        );
    }

    #[test]
    fn broadcast_group_targets_group_members_only() {
        let mut router = Router::new();
        router.broadcast_mode = BroadcastMode::Group;
        let (a, b, c) = (pane(0), pane(1), pane(2));
        router.assign_to_group(a, "backend".to_string());
        router.assign_to_group(b, "backend".to_string());
        // c is never grouped.

        assert_eq!(router.broadcast_targets(a, &[a, b, c]), HashSet::from([a, b]));
        assert_eq!(router.broadcast_targets(c, &[a, b, c]), HashSet::from([c]));
    }

    #[test]
    fn remove_from_group_is_reversible() {
        let mut router = Router::new();
        let a = pane(0);
        router.assign_to_group(a, "backend".to_string());
        assert_eq!(router.group_of(a), Some(GroupId("backend".to_string())));
        router.remove_from_group(a);
        assert_eq!(router.group_of(a), None);
    }

    #[test]
    fn forget_pane_clears_group_membership() {
        let mut router = Router::new();
        let a = pane(0);
        router.assign_to_group(a, "backend".to_string());
        router.forget_pane(a);
        assert_eq!(router.group_of(a), None);
    }

    #[test]
    fn reassigning_a_pane_moves_it_between_groups() {
        let mut router = Router::new();
        let a = pane(0);
        router.assign_to_group(a, "backend".to_string());
        router.assign_to_group(a, "frontend".to_string());

        assert_eq!(router.group_of(a), Some(GroupId("frontend".to_string())));
        // The old group is gone entirely, not left behind empty.
        assert_eq!(router.group_names(), vec!["frontend"]);
    }

    #[test]
    fn a_group_disappears_once_its_last_member_leaves() {
        let mut router = Router::new();
        let (a, b) = (pane(0), pane(1));
        router.assign_to_group(a, "backend".to_string());
        router.assign_to_group(b, "backend".to_string());

        router.remove_from_group(a);
        assert_eq!(router.group_names(), vec!["backend"]);

        router.remove_from_group(b);
        assert!(router.group_names().is_empty());
    }

    #[test]
    fn group_names_lists_every_group_with_a_member_sorted() {
        let mut router = Router::new();
        router.assign_to_group(pane(0), "zebra".to_string());
        router.assign_to_group(pane(1), "alpha".to_string());

        assert_eq!(router.group_names(), vec!["alpha", "zebra"]);
    }
}
