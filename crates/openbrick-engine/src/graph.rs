//! The connection graph (docs/plan/07 section 7): an edge for every pair of
//! mated connectors, connected components by union-find, and the parts that
//! float (no path to a part standing on the ground).

use crate::model::{InstanceId, Model};

/// Counts for the stats panel and the command line.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub parts: usize,
    /// Mated connector pairs (a stud in a receptacle is one, a pin in a hole is one).
    pub connections: usize,
    /// Connected groups of parts.
    pub components: usize,
    /// Parts standing on the ground (their bottom at Y = 0).
    pub grounded: usize,
    /// Parts with no chain of connections to a part on the ground, by id.
    pub floating: Vec<InstanceId>,
}

/// Union-find over table slots.
struct Sets(Vec<u32>);

impl Sets {
    fn find(&mut self, mut a: u32) -> u32 {
        while self.0[a as usize] != a {
            let up = self.0[self.0[a as usize] as usize];
            self.0[a as usize] = up;
            a = up;
        }
        a
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // The smaller root wins, so the result does not depend on edge order.
            let (lo, hi) = (ra.min(rb), ra.max(rb));
            self.0[hi as usize] = lo;
        }
    }
}

impl Model<'_> {
    /// Every mated pair once, as (smaller id, larger id), sorted; a pair of
    /// parts joined by several studs appears once per stud.
    pub fn connections(&self) -> Vec<(InstanceId, InstanceId)> {
        let mut out = Vec::new();
        self.for_each_edge(|a, b| out.push((a.min(b), a.max(b))));
        out.sort_unstable();
        out
    }

    /// Calls `f(slot, slot)` for every male connector that has a mate (the lowest slot when several).
    fn for_each_edge_slots(&self, mut f: impl FnMut(u32, u32)) {
        for &slot in self.instances.slots() {
            let inst = self.instances.at_slot(slot);
            let shape = &self.catalog().parts[inst.part as usize].shape;
            for c in shape.connectors.iter().filter(|c| c.kind.is_male()) {
                let placed = c.transformed(inst.pose.rot, inst.pose.pos);
                if let Some(other) = self.conns.mate_of(&placed, Some(slot)) {
                    f(slot, other);
                }
            }
        }
    }

    fn for_each_edge(&self, mut f: impl FnMut(InstanceId, InstanceId)) {
        self.for_each_edge_slots(|a, b| f(self.instances.id_of(a), self.instances.id_of(b)));
    }

    /// Parts, connections, components and floating parts, computed from the whole table (a
    /// fresh answer, no cache; see [`Model::stats_cached`]).
    pub fn stats(&self) -> Stats {
        let n = self.instances.len();
        let mut sets = Sets((0..self.instances.slot_capacity() as u32).collect());
        let mut connections = 0usize;
        self.for_each_edge_slots(|a, b| {
            connections += 1;
            sets.union(a, b);
        });
        let mut root_on_ground = vec![false; self.instances.slot_capacity()];
        let mut is_root = vec![false; self.instances.slot_capacity()];
        let mut grounded = 0usize;
        for &slot in self.instances.slots() {
            let on_ground = self.instances.body_at(slot).max[1] == 0;
            let r = sets.find(slot) as usize;
            is_root[r] = true;
            root_on_ground[r] |= on_ground;
            grounded += usize::from(on_ground);
        }
        let floating = self
            .instances
            .slots()
            .iter()
            .filter(|&&slot| !root_on_ground[sets.find(slot) as usize])
            .map(|&slot| self.instances.id_of(slot))
            .collect();
        Stats {
            parts: n,
            connections,
            components: is_root.iter().filter(|r| **r).count(),
            grounded,
            floating,
        }
    }

    /// [`Model::stats`], kept until the next edit: asking again after no change costs nothing, and
    /// the answer is always the one a fresh computation gives (a property test checks it after
    /// random edits, undos and redos).
    pub fn stats_cached(&mut self) -> &Stats {
        let rev = self.revision();
        if self.stats_cache.as_ref().is_none_or(|(r, _)| *r != rev) {
            self.stats_cache = Some((rev, self.stats()));
        }
        &self.stats_cache.as_ref().expect("just set").1
    }
}
