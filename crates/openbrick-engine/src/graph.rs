//! The connection graph (docs/plan/07 section 7): an edge for every pair of
//! mated connectors, connected components by union-find, and the parts that
//! float (no path to a part standing on the ground).

use std::collections::BTreeMap;

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

/// Union-find over table positions.
struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // The smaller root wins, so the result does not depend on edge order.
            let (lo, hi) = (ra.min(rb), ra.max(rb));
            self.0[hi] = lo;
        }
    }
}

impl Model<'_> {
    /// Every mated pair once, as (smaller id, larger id), sorted; a pair of
    /// parts joined by several studs appears once per stud.
    pub fn connections(&self) -> Vec<(InstanceId, InstanceId)> {
        let mut out = Vec::new();
        for inst in self.instances.iter() {
            let conns = self
                .placed_connectors(inst.part, &inst.pose)
                .expect("placed parts are in the catalog");
            for c in conns.iter().filter(|c| c.kind.is_male()) {
                if let Some(other) = self.conns.mate_of(c, Some(inst.id)) {
                    out.push((inst.id.min(other), inst.id.max(other)));
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Parts, connections, components and floating parts.
    pub fn stats(&self) -> Stats {
        let n = self.instances.len();
        let edges = self.connections();
        let pos: BTreeMap<InstanceId, usize> = self
            .instances
            .id
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();
        let mut sets = Sets((0..n).collect());
        for &(a, b) in &edges {
            sets.union(pos[&a], pos[&b]);
        }
        let grounded: Vec<bool> = self
            .instances
            .iter()
            .map(|i| self.body_box(i.part, &i.pose).expect("placed").max[1] == 0)
            .collect();
        let mut roots_on_ground = vec![false; n];
        let mut roots = vec![false; n];
        for (i, &g) in grounded.iter().enumerate() {
            let r = sets.find(i);
            roots[r] = true;
            roots_on_ground[r] |= g;
        }
        let floating = (0..n)
            .filter(|&i| {
                let r = sets.find(i);
                !roots_on_ground[r]
            })
            .map(|i| self.instances.id[i])
            .collect();
        Stats {
            parts: n,
            connections: edges.len(),
            components: roots.iter().filter(|r| **r).count(),
            grounded: grounded.iter().filter(|g| **g).count(),
            floating,
        }
    }
}
