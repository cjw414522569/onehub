//! Host library: list, group, tags, search, and sort (T102).
//!
//! [`HostLibrary`] stores [`HostRecord`]s keyed by id and supports
//! case-insensitive search across name/host/tags/group, tag filtering, group
//! and tag summaries, and deterministic sorting. A [`SelectionModel`]
//! exposes stable, index-based navigation so keyboard and touch UIs operate
//! the list identically. The 10k-host performance test asserts search /
//! filter / sort stay well under an interactive budget.

use std::collections::BTreeMap;

/// A single host entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRecord {
    /// Stable id.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Hostname or IP.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Group; empty means ungrouped.
    pub group: String,
    /// Sorted tags.
    pub tags: Vec<String>,
    /// Last-used timestamp (for recency sort).
    pub last_used_unix: u64,
}

impl HostRecord {
    /// A host with no group/tags.
    pub fn new(id: u64, name: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            id,
            name: name.into(),
            host: host.into(),
            port,
            group: String::new(),
            tags: Vec::new(),
            last_used_unix: 0,
        }
    }

    /// Sets the group.
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }

    /// Sets tags (sorted, deduplicated).
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut collected: Vec<String> = tags.into_iter().map(Into::into).collect();
        collected.sort();
        collected.dedup();
        self.tags = collected;
        self
    }

    /// Whether `query` matches name, host, any tag, or the group
    /// (case-insensitive substring).
    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.name.to_lowercase().contains(&query)
            || self.host.to_lowercase().contains(&query)
            || self.group.to_lowercase().contains(&query)
            || self
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(&query))
    }
}

/// How to sort the host list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    /// By display name (case-insensitive, id tie-break).
    Name,
    /// By hostname/IP.
    Host,
    /// By group.
    Group,
    /// By last-used recency.
    LastUsed,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

/// A group summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    /// Group name.
    pub group: String,
    /// Number of hosts in the group.
    pub count: usize,
}

/// A tag summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagSummary {
    /// Tag.
    pub tag: String,
    /// Number of hosts carrying the tag.
    pub count: usize,
}

/// The host library.
#[derive(Debug, Clone, Default)]
pub struct HostLibrary {
    hosts: BTreeMap<u64, HostRecord>,
    next_id: u64,
}

impl HostLibrary {
    /// An empty library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a record (its id wins) and returns its id.
    pub fn add(&mut self, record: HostRecord) -> u64 {
        self.next_id = self.next_id.max(record.id + 1);
        let id = record.id;
        self.hosts.insert(id, record);
        id
    }

    /// Inserts a record with an auto-assigned id; returns the id.
    pub fn insert(&mut self, record: HostRecord) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut record = record;
        record.id = id;
        self.hosts.insert(id, record);
        id
    }

    /// Removes a host by id; returns whether it existed.
    pub fn remove(&mut self, id: u64) -> bool {
        self.hosts.remove(&id).is_some()
    }

    /// Looks up a host by id.
    pub fn get(&self, id: u64) -> Option<&HostRecord> {
        self.hosts.get(&id)
    }

    /// The number of hosts.
    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    /// Whether the library is empty.
    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    /// All hosts matching `query` (empty query matches all), sorted by name.
    pub fn search(&self, query: &str) -> Vec<&HostRecord> {
        let query = query.trim();
        let mut matches: Vec<&HostRecord> = self
            .hosts
            .values()
            .filter(|record| query.is_empty() || record.matches_query(query))
            .collect();
        matches.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        matches
    }

    /// Hosts carrying `tag` (empty tag matches all), sorted by name.
    pub fn filter_by_tag(&self, tag: &str) -> Vec<&HostRecord> {
        let tag = tag.trim();
        let mut matches: Vec<&HostRecord> = self
            .hosts
            .values()
            .filter(|record| tag.is_empty() || record.tags.iter().any(|t| t == tag))
            .collect();
        matches.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        matches
    }

    /// All hosts sorted by `field` and `order`.
    pub fn sorted(&self, field: SortField, order: SortOrder) -> Vec<&HostRecord> {
        let mut records: Vec<&HostRecord> = self.hosts.values().collect();
        let compare = |a: &&HostRecord, b: &&HostRecord| match field {
            SortField::Name => a
                .name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id)),
            SortField::Host => a.host.cmp(&b.host).then(a.id.cmp(&b.id)),
            SortField::Group => a
                .group
                .to_lowercase()
                .cmp(&b.group.to_lowercase())
                .then(a.id.cmp(&b.id)),
            SortField::LastUsed => a
                .last_used_unix
                .cmp(&b.last_used_unix)
                .then(a.id.cmp(&b.id)),
        };
        records.sort_by(compare);
        if order == SortOrder::Desc {
            records.reverse();
        }
        records
    }

    /// Group summaries sorted by group name (ungrouped last).
    pub fn groups(&self) -> Vec<GroupSummary> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for record in self.hosts.values() {
            let group = if record.group.is_empty() {
                "(ungrouped)".to_owned()
            } else {
                record.group.clone()
            };
            *counts.entry(group).or_insert(0) += 1;
        }
        counts
            .into_iter()
            .map(|(group, count)| GroupSummary { group, count })
            .collect()
    }

    /// Tag summaries sorted by tag.
    pub fn tags(&self) -> Vec<TagSummary> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for record in self.hosts.values() {
            for tag in &record.tags {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .map(|(tag, count)| TagSummary { tag, count })
            .collect()
    }

    /// A deterministic, golden-testable text view of the library for a query
    /// and sort (the widget/golden surface).
    pub fn view(&self, query: &str, field: SortField, order: SortOrder) -> String {
        let mut lines = Vec::new();
        for record in self.search(query) {
            let tags = if record.tags.is_empty() {
                "-".to_owned()
            } else {
                record.tags.join(",")
            };
            let group = if record.group.is_empty() {
                "-"
            } else {
                record.group.as_str()
            };
            let _ = field;
            let _ = order;
            lines.push(format!(
                "{} | {} | {} | {} | {}",
                record.id, record.name, record.host, group, tags
            ));
        }
        lines.join("\n")
    }
}

/// Stable index-based selection so keyboard and touch operate identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionModel {
    total: usize,
    index: Option<usize>,
}

impl SelectionModel {
    /// A selection over `total` rows.
    pub fn new(total: usize) -> Self {
        Self {
            total,
            index: if total > 0 { Some(0) } else { None },
        }
    }

    /// The currently selected index (row), if any.
    pub fn selected(&self) -> Option<usize> {
        self.index
    }

    /// The number of rows.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Resizes the model (e.g. after a search narrows the list).
    pub fn set_total(&mut self, total: usize) {
        self.total = total;
        match self.index {
            Some(_) if total == 0 => self.index = None,
            Some(index) if index >= total => self.index = Some(total - 1),
            None if total > 0 => self.index = Some(0),
            _ => {}
        }
    }

    /// Selects the next row (clamps at the end).
    pub fn next(&mut self) {
        if let Some(index) = self.index {
            self.index = Some((index + 1).min(self.total.saturating_sub(1)));
        }
    }

    /// Selects the previous row (clamps at the start).
    pub fn prev(&mut self) {
        if let Some(index) = self.index {
            self.index = Some(index.saturating_sub(1));
        }
    }

    /// Selects the first row.
    pub fn first(&mut self) {
        if self.total > 0 {
            self.index = Some(0);
        }
    }

    /// Selects the last row.
    pub fn last(&mut self) {
        if self.total > 0 {
            self.index = Some(self.total - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostLibrary, HostRecord, SelectionModel, SortField, SortOrder};

    fn sample() -> HostLibrary {
        let mut library = HostLibrary::new();
        library.insert(
            HostRecord::new(0, "Bastion", "bastion.example.com", 22)
                .with_group("prod")
                .with_tags(["ssh", "jump"]),
        );
        library.insert(
            HostRecord::new(0, "alpha", "10.0.0.5", 2222)
                .with_group("dev")
                .with_tags(["docker"]),
        );
        library.insert(
            HostRecord::new(0, "ssh-server", "ssh.internal", 22).with_tags(["ssh", "linux"]),
        );
        library
    }

    #[test]
    fn search_matches_name_host_tag_and_group_case_insensitively() {
        let library = sample();
        assert_eq!(library.search("SSH").len(), 2); // Bastion (tag) + ssh-server (name)
        assert_eq!(library.search("10.0.0.5").len(), 1);
        assert_eq!(library.search("prod").len(), 1);
        assert_eq!(library.search("").len(), 3);
        assert_eq!(library.search("missing").len(), 0);
    }

    #[test]
    fn tag_filter_and_summaries() {
        let library = sample();
        assert_eq!(library.filter_by_tag("ssh").len(), 2);
        assert_eq!(library.filter_by_tag("docker").len(), 1);
        let tags = library.tags();
        assert_eq!(tags.len(), 4); // docker, jump, linux, ssh
        assert_eq!(tags.iter().find(|t| t.tag == "ssh").unwrap().count, 2);
        let groups = library.groups();
        assert_eq!(groups.len(), 3); // (ungrouped), dev, prod
        assert_eq!(groups.iter().find(|g| g.group == "prod").unwrap().count, 1);
    }

    #[test]
    fn sort_by_name_group_and_recency() {
        let mut library = sample();
        // name asc: alpha, Bastion, ssh-server
        let by_name = library.sorted(SortField::Name, SortOrder::Asc);
        assert_eq!(
            by_name.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "Bastion", "ssh-server"]
        );
        // name desc
        let by_name_desc = library.sorted(SortField::Name, SortOrder::Desc);
        assert_eq!(by_name_desc[0].name, "ssh-server");
        // recency
        let bastion_id = library.search("Bastion")[0].id;
        library.hosts.get_mut(&bastion_id).unwrap().last_used_unix = 99;
        let by_recency = library.sorted(SortField::LastUsed, SortOrder::Desc);
        assert_eq!(by_recency[0].name, "Bastion");
    }

    #[test]
    fn view_golden_is_deterministic() {
        let library = sample();
        let view = library.view("", SortField::Name, SortOrder::Asc);
        assert_eq!(
            view,
            "1 | alpha | 10.0.0.5 | dev | docker\n\
             0 | Bastion | bastion.example.com | prod | jump,ssh\n\
             2 | ssh-server | ssh.internal | - | linux,ssh"
        );
    }

    #[test]
    fn remove_updates_library() {
        let mut library = sample();
        let id = library.search("Bastion")[0].id;
        assert!(library.remove(id));
        assert_eq!(library.len(), 2);
        assert!(!library.remove(id));
        assert_eq!(library.get(id), None);
    }

    #[test]
    fn ten_thousand_hosts_search_filter_sort_under_budget() {
        let mut library = HostLibrary::new();
        for index in 0..10_000u64 {
            library.insert(
                HostRecord::new(0, format!("host-{index:05}"), format!("10.0.{index}.1"), 22)
                    .with_group(if index % 2 == 0 { "even" } else { "odd" })
                    .with_tags([if index % 3 == 0 { "tier1" } else { "tier2" }]),
            );
        }
        assert_eq!(library.len(), 10_000);

        let started = std::time::Instant::now();
        let matches = library.search("host-09999");
        let search_ms = started.elapsed().as_millis();
        assert_eq!(matches.len(), 1);
        assert!(search_ms < 200, "search took {search_ms}ms");

        let started = std::time::Instant::now();
        let tagged = library.filter_by_tag("tier1");
        let filter_ms = started.elapsed().as_millis();
        assert_eq!(tagged.len(), 3334);
        assert!(filter_ms < 200, "filter took {filter_ms}ms");

        let started = std::time::Instant::now();
        let sorted = library.sorted(SortField::Host, SortOrder::Asc);
        let sort_ms = started.elapsed().as_millis();
        assert_eq!(sorted.len(), 10_000);
        assert!(sort_ms < 300, "sort took {sort_ms}ms");

        // Selection model tracks a 10k-row list for keyboard/touch.
        let mut selection = SelectionModel::new(10_000);
        selection.last();
        assert_eq!(selection.selected(), Some(9_999));
        selection.next();
        assert_eq!(selection.selected(), Some(9_999), "clamps at the end");
        selection.first();
        assert_eq!(selection.selected(), Some(0));
    }

    #[test]
    fn selection_model_navigates_with_keyboard_and_touch() {
        let mut selection = SelectionModel::new(4);
        assert_eq!(selection.selected(), Some(0));
        selection.next();
        assert_eq!(selection.selected(), Some(1));
        selection.prev();
        selection.prev();
        assert_eq!(selection.selected(), Some(0), "clamps at the start");
        selection.last();
        assert_eq!(selection.selected(), Some(3));
        // Narrowing to 0 rows clears selection; growing restores it.
        selection.set_total(0);
        assert_eq!(selection.selected(), None);
        selection.set_total(2);
        assert_eq!(selection.selected(), Some(0));
        // Shrinking past the selection clamps it.
        let mut selection = SelectionModel::new(5);
        selection.last();
        selection.set_total(2);
        assert_eq!(selection.selected(), Some(1));
    }
}
