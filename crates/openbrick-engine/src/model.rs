//! The instance table and the edits every command is made of (docs/plan/07
//! section 2): a struct-of-arrays table in placement order, the spatial hash of
//! the body boxes and the connector hash, kept in step by [`Model`]'s
//! primitive operations, which check collision and the lattice first.

use std::collections::BTreeMap;

use openbrick_catalog::{Aabb, Catalog, CatalogPart, Conn, Palette, PartId};
use openbrick_units::{LatticePose, Rot24};

use crate::graph::Stats;
use crate::history::History;
use crate::on_stud_grid;
use crate::spatial::{ConnHash, FastMap, SpatialHash};

/// Id of a placed part, unique within a model and kept by `.obrick` files.
/// Small integers in placement order; ULIDs arrive with collaboration (docs/plan/07
/// section 8).
pub type InstanceId = u32;

/// One placed part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Instance {
    pub id: InstanceId,
    pub part: PartId,
    pub pose: LatticePose,
    /// Palette colour code.
    pub colour: u16,
}

/// Struct-of-arrays instance table in placement order (docs/plan/07 section 2).
///
/// The columns are indexed by *slot*, which never changes while a part is placed (a removed part's
/// slot is reused by a later one), and `order` lists the slots in placement order. So adding or
/// removing a part moves 4 bytes in `order`, not a row of every column, and the indexes (the id
/// map, the spatial and connector hashes) keep their slots without renumbering: the cost of an edit
/// does not grow with the number of parts beyond one small memmove (about 20 microseconds for
/// 100,000 parts; bench/scale).
#[derive(Clone, Debug, Default)]
pub struct Instances {
    ids: Vec<InstanceId>,
    parts: Vec<PartId>,
    poses: Vec<LatticePose>,
    colours: Vec<u16>,
    /// The body box and the pick box of each placed part, kept so a collision or a ray test is a
    /// comparison, not a transform of the part's box.
    bodies: Vec<Aabb>,
    picks: Vec<Aabb>,
    /// Slots in placement order.
    order: Vec<u32>,
    /// Slots of removed parts, free for reuse.
    free: Vec<u32>,
}

impl Instances {
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The part at table position `index` (placement order).
    pub fn get(&self, index: usize) -> Instance {
        self.at_slot(self.order[index])
    }

    pub(crate) fn at_slot(&self, slot: u32) -> Instance {
        let s = slot as usize;
        Instance {
            id: self.ids[s],
            part: self.parts[s],
            pose: self.poses[s],
            colour: self.colours[s],
        }
    }

    /// Slots in placement order.
    pub(crate) fn slots(&self) -> &[u32] {
        &self.order
    }

    /// Slots ever used (the length of the columns): the size of a table indexed by slot.
    pub(crate) fn slot_capacity(&self) -> usize {
        self.ids.len()
    }

    pub(crate) fn id_of(&self, slot: u32) -> InstanceId {
        self.ids[slot as usize]
    }

    /// Where in placement order the part in `slot` is: a binary search when ids ascend (they do
    /// in every file this program writes), a scan from the end otherwise.
    pub(crate) fn position_of(&self, slot: u32) -> usize {
        let id = self.ids[slot as usize];
        let guess = self.order.partition_point(|&s| self.ids[s as usize] < id);
        if self.order.get(guess) == Some(&slot) {
            return guess;
        }
        self.order
            .iter()
            .rposition(|&s| s == slot)
            .expect("a placed slot is in the order")
    }

    pub(crate) fn insert(&mut self, position: usize, i: Instance, body: Aabb, pick: Aabb) -> u32 {
        let slot = if let Some(slot) = self.free.pop() {
            let s = slot as usize;
            self.ids[s] = i.id;
            self.parts[s] = i.part;
            self.poses[s] = i.pose;
            self.colours[s] = i.colour;
            self.bodies[s] = body;
            self.picks[s] = pick;
            slot
        } else {
            self.ids.push(i.id);
            self.parts.push(i.part);
            self.poses.push(i.pose);
            self.colours.push(i.colour);
            self.bodies.push(body);
            self.picks.push(pick);
            (self.ids.len() - 1) as u32
        };
        self.order.insert(position.min(self.order.len()), slot);
        slot
    }

    /// Remove the part in `slot`; returns its table position and the part.
    pub(crate) fn remove_slot(&mut self, slot: u32) -> (usize, Instance) {
        let position = self.position_of(slot);
        let inst = self.at_slot(slot);
        self.order.remove(position);
        self.ids[slot as usize] = 0;
        self.free.push(slot);
        (position, inst)
    }

    pub(crate) fn set_pose_at(&mut self, slot: u32, pose: LatticePose, body: Aabb, pick: Aabb) {
        self.poses[slot as usize] = pose;
        self.bodies[slot as usize] = body;
        self.picks[slot as usize] = pick;
    }

    pub(crate) fn body_at(&self, slot: u32) -> &Aabb {
        &self.bodies[slot as usize]
    }

    pub(crate) fn pick_at(&self, slot: u32) -> &Aabb {
        &self.picks[slot as usize]
    }

    pub(crate) fn set_colour_at(&mut self, slot: u32, colour: u16) -> u16 {
        std::mem::replace(&mut self.colours[slot as usize], colour)
    }

    /// The placed parts in placement order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Instance> + ExactSizeIterator + '_ {
        self.order.iter().map(|&s| self.at_slot(s))
    }

    /// The part numbers of the placed parts (any order).
    pub fn parts(&self) -> impl Iterator<Item = PartId> + '_ {
        self.order.iter().map(|&s| self.parts[s as usize])
    }

    /// The colour codes of the placed parts (any order).
    pub fn colours(&self) -> impl Iterator<Item = u16> + '_ {
        self.order.iter().map(|&s| self.colours[s as usize])
    }
}

impl PartialEq for Instances {
    /// Two tables are equal when they hold the same parts in the same order.
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl Eq for Instances {}

/// Why an edit was refused. Every message says what to do instead.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("there is no part `{0}` in the catalog; choose one from the parts palette")]
    UnknownPart(String),
    #[error("there is no part number {0} in the catalog of {1} parts")]
    UnknownPartIndex(PartId, usize),
    #[error("there is no placed part with id {0}; it may have been removed")]
    UnknownInstance(InstanceId),
    #[error("colour {0} is not in the palette; choose a colour from the colour palette")]
    UnknownColour(u16),
    #[error("rotation {0} is not one of the 24 lattice rotations (0 to 23)")]
    UnknownRotation(u8),
    #[error("the part would overlap part {with} ({name}); move it so the two only touch")]
    Collides { with: InstanceId, name: String },
    #[error("the part would reach {depth} LDU below the ground; place it on the ground or on another part")]
    BelowGround { depth: i32 },
    #[error("the part's connectors would be off the stud grid at {pos:?}; positions come from snapping, or put each stud on an odd multiple of half a stud in X and Z and a whole plate in Y")]
    OffGrid { pos: [i32; 3] },
    #[error("there is nothing to undo")]
    NothingToUndo,
    #[error("there is nothing to redo")]
    NothingToRedo,
    #[error("id {0} is used by two placed parts; give each its own id")]
    DuplicateId(InstanceId),
    #[error("id 0 is not an id; part ids start at 1")]
    ZeroId,
    #[error("the model has used every part id up to {0}; save it and start a new model")]
    IdsExhausted(InstanceId),
    #[error("the part's body covers {cells} lattice cells; the limit is {limit}. A pack part this large is not placeable; import the pack again from a library file that is not damaged")]
    BoxTooLarge { cells: u64, limit: u64 },
    #[error("position {pos:?} is beyond {limit} LDU from the origin; keep every coordinate within plus or minus {limit}")]
    OutOfRange { pos: [i32; 3], limit: i32 },
    #[error("the model already has {limit} parts, the most this program holds; split it into smaller models, or remove parts first")]
    TooManyParts { limit: usize },
}

/// A model: the catalog it places parts from, the instance table and its
/// indexes, and the command history.
#[derive(Clone, Debug)]
pub struct Model<'c> {
    catalog: &'c Catalog,
    pub(crate) instances: Instances,
    /// Table slot of each placed part by id.
    index: FastMap<InstanceId, u32>,
    spatial: SpatialHash,
    pub(crate) conns: ConnHash,
    next_id: InstanceId,
    pub(crate) history: History,
    /// Parts placed by (catalog part number, colour code): the bill of materials, kept in step by
    /// every primitive instead of counted again on request.
    bom: BTreeMap<(PartId, u16), u32>,
    /// Ids of the parts changed since [`Model::take_changes`], with the flag for "too many to list".
    changes: Vec<InstanceId>,
    changes_overflow: bool,
    /// Bumped by every primitive edit: the change tick results are cached against.
    revision: u64,
    pub(crate) stats_cache: Option<(u64, Stats)>,
}

/// The parts changed since the last [`Model::take_changes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changes {
    /// Ids touched (added, removed, moved or recoloured), ascending, each once. Empty with
    /// `overflow` when more changed than are worth listing: read the table again.
    pub ids: Vec<InstanceId>,
    pub overflow: bool,
}

/// The indexes as lists, for the property tests: they compare the incrementally kept hashes with
/// ones built from scratch from the instance table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexSnapshot {
    pub cells: Vec<([i32; 3], InstanceId)>,
    pub connectors: Vec<([i32; 3], InstanceId, openbrick_catalog::ConnKind, [i8; 3])>,
    pub bom: Vec<((PartId, u16), u32)>,
}

impl<'c> Model<'c> {
    /// An empty model over `catalog`.
    pub fn new(catalog: &'c Catalog) -> Model<'c> {
        Model {
            catalog,
            instances: Instances::default(),
            index: FastMap::default(),
            spatial: SpatialHash::default(),
            conns: ConnHash::default(),
            next_id: 1,
            history: History::default(),
            bom: BTreeMap::new(),
            changes: Vec::new(),
            changes_overflow: false,
            revision: 0,
            stats_cache: None,
        }
    }

    pub fn catalog(&self) -> &'c Catalog {
        self.catalog
    }

    /// Make room for about `parts` more parts in the indexes (opening a big file).
    pub fn reserve(&mut self, parts: usize) {
        let parts = parts.min(crate::MAX_INSTANCES);
        self.index.reserve(parts);
        self.spatial.reserve(parts);
        self.conns.reserve(parts);
    }

    pub fn instances(&self) -> &Instances {
        &self.instances
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// The change tick: bumped by every edit (and by undo and redo). Two reads with the same
    /// revision saw the same model.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True when the spatial and connector hashes hold nothing: after every
    /// part is removed they must be empty again (a property test checks it).
    pub fn indexes_are_empty(&self) -> bool {
        self.spatial.is_empty() && self.conns.is_empty() && self.bom.is_empty()
    }

    /// The id the next added part gets.
    pub fn next_id(&self) -> InstanceId {
        self.next_id
    }

    /// The placed part with this id.
    pub fn instance(&self, id: InstanceId) -> Option<Instance> {
        self.index.get(&id).map(|&s| self.instances.at_slot(s))
    }

    pub(crate) fn slot_of(&self, id: InstanceId) -> Result<u32, EngineError> {
        self.index
            .get(&id)
            .copied()
            .ok_or(EngineError::UnknownInstance(id))
    }

    pub fn part(&self, part: PartId) -> Result<&'c CatalogPart, EngineError> {
        self.catalog
            .get(part)
            .ok_or(EngineError::UnknownPartIndex(part, self.catalog.len()))
    }

    /// The body box of `part` at `pose`.
    pub fn body_box(&self, part: PartId, pose: &LatticePose) -> Result<Aabb, EngineError> {
        Ok(self.part(part)?.shape.body.transformed(pose.rot, pose.pos))
    }

    /// True for a baseplate whose bottom is on the ground (the floor of a model).
    pub fn is_ground_baseplate(&self, i: &Instance) -> bool {
        let p = &self.catalog.parts[i.part as usize];
        p.base && p.shape.body.transformed(i.pose.rot, i.pose.pos).max[1] == 0
    }

    /// The pick box (body and studs) of a placed part.
    pub fn pick_box(&self, i: &Instance) -> Aabb {
        let shape = &self.catalog.parts[i.part as usize].shape;
        shape.pick.transformed(i.pose.rot, i.pose.pos)
    }

    /// The box the spatial hash stores for a part: its pick box and its body box together (the
    /// pick box holds the body box for every part, but the hash must not depend on it).
    fn cover_box(&self, part: PartId, pose: &LatticePose) -> Result<Aabb, EngineError> {
        let shape = &self.part(part)?.shape;
        Ok(shape
            .body
            .transformed(pose.rot, pose.pos)
            .union(&shape.pick.transformed(pose.rot, pose.pos)))
    }

    /// The connectors of `part` at `pose`.
    pub fn placed_connectors(
        &self,
        part: PartId,
        pose: &LatticePose,
    ) -> Result<Vec<Conn>, EngineError> {
        Ok(self
            .part(part)?
            .shape
            .connectors
            .iter()
            .map(|c| c.transformed(pose.rot, pose.pos))
            .collect())
    }

    /// Why `part` cannot stand at `pose` (ignoring the placed part `skip`),
    /// or `Ok` when it can: on the stud grid, above the ground, overlapping
    /// nothing. Touching is allowed.
    pub fn check_pose(
        &self,
        part: PartId,
        pose: &LatticePose,
        skip: Option<InstanceId>,
    ) -> Result<(), EngineError> {
        if pose.rot.0 >= 24 {
            return Err(EngineError::UnknownRotation(pose.rot.0));
        }
        if pose
            .pos
            .iter()
            .any(|c| c.unsigned_abs() > crate::MAX_COORD as u32)
        {
            return Err(EngineError::OutOfRange {
                pos: pose.pos,
                limit: crate::MAX_COORD,
            });
        }
        // The studs and receptacles of a built-in part must be on the stud grid (snapping only
        // produces such poses, and the model refuses others). A part of an LDraw pack is drawn by
        // its authors, with studs half a stud apart (jumper plates) or a clip hanging two units
        // below its plate, so its connectors are not required to be on the grid: they mate only
        // where they coincide with another part's, which is exact whatever the grid. A part turned
        // on its side is checked by its box alone, and pins and axles have no grid of their own.
        let cat_part = self.part(part)?;
        if cat_part.origin == openbrick_catalog::Origin::Builtin {
            for c in cat_part
                .shape
                .connectors
                .iter()
                .map(|c| c.transformed(pose.rot, pose.pos))
            {
                if c.kind.is_stud_kind() && c.dir[1] != 0 && !on_stud_grid(c.pos) {
                    return Err(EngineError::OffGrid { pos: c.pos });
                }
            }
        }
        let b = self.body_box(part, pose)?;
        crate::spatial::check_box(&b)?;
        if b.max[1] > 0 {
            return Err(EngineError::BelowGround { depth: b.max[1] });
        }
        if let Some(with) = self.collision(&b, skip)? {
            let name = self
                .part(self.instance(with).expect("indexed").part)?
                .name
                .clone();
            return Err(EngineError::Collides { with, name });
        }
        Ok(())
    }

    /// The first placed part (lowest id) whose body overlaps `b`, ignoring `skip`.
    pub fn collision(
        &self,
        b: &Aabb,
        skip: Option<InstanceId>,
    ) -> Result<Option<InstanceId>, EngineError> {
        let mut best: Option<InstanceId> = None;
        self.spatial.for_each_near(b, |slot| {
            let id = self.instances.id_of(slot);
            if Some(id) != skip
                && best.is_none_or(|other| id < other)
                && self.instances.body_at(slot).overlaps(b)
            {
                best = Some(id);
            }
        })?;
        Ok(best)
    }

    /// How many connectors of `part` at `pose` mate with placed parts (other
    /// than `skip`).
    pub fn mates(
        &self,
        part: PartId,
        pose: &LatticePose,
        skip: Option<InstanceId>,
    ) -> Result<u32, EngineError> {
        let skip = skip.and_then(|id| self.index.get(&id).copied());
        Ok(self
            .part(part)?
            .shape
            .connectors
            .iter()
            .map(|c| c.transformed(pose.rot, pose.pos))
            .filter(|c| self.conns.mate_of(c, skip).is_some())
            .count() as u32)
    }

    /// Check a colour against the bundled palette.
    pub fn check_colour(colour: u16) -> Result<(), EngineError> {
        Palette::bundled()
            .get(colour)
            .map(|_| ())
            .ok_or(EngineError::UnknownColour(colour))
    }

    // ------------------------------------------------------------ primitives

    fn touch(&mut self, id: InstanceId) {
        self.revision += 1;
        self.stats_cache = None;
        if self.changes.len() >= 2 * crate::MAX_INSTANCES {
            self.changes.clear();
            self.changes_overflow = true;
        }
        if !self.changes_overflow {
            self.changes.push(id);
        }
    }

    /// The parts changed since the last call, and forget them. See [`Changes`].
    pub fn take_changes(&mut self) -> Changes {
        let overflow = std::mem::take(&mut self.changes_overflow);
        let mut ids = std::mem::take(&mut self.changes);
        ids.sort_unstable();
        ids.dedup();
        Changes { ids, overflow }
    }

    /// Insert `inst` at table position `index` (clamped to the end), after
    /// checking it. The building block of add and of undoing a removal.
    pub(crate) fn insert(&mut self, index: usize, inst: Instance) -> Result<(), EngineError> {
        if inst.id == 0 {
            return Err(EngineError::ZeroId);
        }
        if self.index.contains_key(&inst.id) {
            return Err(EngineError::DuplicateId(inst.id));
        }
        if self.instances.len() >= crate::MAX_INSTANCES {
            return Err(EngineError::TooManyParts {
                limit: crate::MAX_INSTANCES,
            });
        }
        Model::check_colour(inst.colour)?;
        self.check_pose(inst.part, &inst.pose, None)?;
        let shape = &self.part(inst.part)?.shape;
        let body = shape.body.transformed(inst.pose.rot, inst.pose.pos);
        let pick = shape.pick.transformed(inst.pose.rot, inst.pose.pos);
        let cover = body.union(&pick);
        let slot = self.instances.insert(index, inst, body, pick);
        if let Err(e) = self.spatial.insert(slot, &cover) {
            self.instances.remove_slot(slot);
            return Err(e);
        }
        self.index.insert(inst.id, slot);
        let shape = &self.catalog.parts[inst.part as usize].shape;
        self.conns.insert(
            slot,
            shape
                .connectors
                .iter()
                .map(|c| c.transformed(inst.pose.rot, inst.pose.pos)),
        );
        *self.bom.entry((inst.part, inst.colour)).or_default() += 1;
        self.next_id = self.next_id.max(inst.id.saturating_add(1));
        self.touch(inst.id);
        Ok(())
    }

    /// Remove the placed part `id`; returns it and where it was in the table.
    pub(crate) fn delete(&mut self, id: InstanceId) -> Result<(usize, Instance), EngineError> {
        let slot = self.slot_of(id)?;
        let inst = self.instances.at_slot(slot);
        let cover = self
            .instances
            .body_at(slot)
            .union(self.instances.pick_at(slot));
        self.spatial.remove(slot, &cover)?;
        let shape = &self.catalog.parts[inst.part as usize].shape;
        self.conns.remove(
            slot,
            shape
                .connectors
                .iter()
                .map(|c| c.transformed(inst.pose.rot, inst.pose.pos)),
        );
        let (position, inst) = self.instances.remove_slot(slot);
        self.index.remove(&id);
        self.bom_take(inst.part, inst.colour);
        self.touch(id);
        Ok((position, inst))
    }

    fn bom_take(&mut self, part: PartId, colour: u16) {
        if let Some(n) = self.bom.get_mut(&(part, colour)) {
            *n -= 1;
            if *n == 0 {
                self.bom.remove(&(part, colour));
            }
        }
    }

    /// Move the placed part `id` to `pose`; returns the old pose.
    pub(crate) fn set_pose(
        &mut self,
        id: InstanceId,
        pose: LatticePose,
    ) -> Result<LatticePose, EngineError> {
        let slot = self.slot_of(id)?;
        let inst = self.instances.at_slot(slot);
        self.check_pose(inst.part, &pose, Some(id))?;
        let old_cover = self
            .instances
            .body_at(slot)
            .union(self.instances.pick_at(slot));
        let shape = &self.catalog.parts[inst.part as usize].shape;
        let body = shape.body.transformed(pose.rot, pose.pos);
        let pick = shape.pick.transformed(pose.rot, pose.pos);
        let new_cover = body.union(&pick);
        self.spatial.remove(slot, &old_cover)?;
        self.conns.remove(
            slot,
            shape
                .connectors
                .iter()
                .map(|c| c.transformed(inst.pose.rot, inst.pose.pos)),
        );
        self.instances.set_pose_at(slot, pose, body, pick);
        self.spatial.insert(slot, &new_cover)?;
        self.conns.insert(
            slot,
            shape
                .connectors
                .iter()
                .map(|c| c.transformed(pose.rot, pose.pos)),
        );
        self.touch(id);
        Ok(inst.pose)
    }

    /// Recolour the placed part `id`; returns the old colour.
    pub(crate) fn set_colour(&mut self, id: InstanceId, colour: u16) -> Result<u16, EngineError> {
        Model::check_colour(colour)?;
        let slot = self.slot_of(id)?;
        let inst = self.instances.at_slot(slot);
        let old = self.instances.set_colour_at(slot, colour);
        self.bom_take(inst.part, old);
        *self.bom.entry((inst.part, colour)).or_default() += 1;
        self.touch(id);
        Ok(old)
    }

    /// The parts by (catalog part number, colour code), ascending, with their counts.
    pub(crate) fn bom_counts(&self) -> impl Iterator<Item = ((PartId, u16), u32)> + '_ {
        self.bom.iter().map(|(&k, &n)| (k, n))
    }

    /// The same counts as [`Model::bom_counts`], counted again from the instance table (the
    /// property tests compare the two).
    pub fn bom_counts_from_scratch(&self) -> Vec<((PartId, u16), u32)> {
        let mut counts: BTreeMap<(PartId, u16), u32> = BTreeMap::new();
        for i in self.instances.iter() {
            *counts.entry((i.part, i.colour)).or_default() += 1;
        }
        counts.into_iter().collect()
    }

    /// The incrementally kept indexes as lists (see [`IndexSnapshot`]).
    pub fn index_snapshot(&self) -> IndexSnapshot {
        let mut cells: Vec<_> = self
            .spatial
            .snapshot()
            .into_iter()
            .map(|(c, s)| (c, self.instances.id_of(s)))
            .collect();
        cells.sort_unstable();
        let mut connectors: Vec<_> = self
            .conns
            .snapshot()
            .into_iter()
            .map(|(p, s, k, d)| (p, self.instances.id_of(s), k, d))
            .collect();
        connectors.sort_unstable();
        IndexSnapshot {
            cells,
            connectors,
            bom: self.bom.iter().map(|(&k, &n)| (k, n)).collect(),
        }
    }

    /// The indexes built again from the instance table, for the property tests.
    pub fn index_snapshot_from_scratch(&self) -> IndexSnapshot {
        let mut spatial = SpatialHash::default();
        let mut conns = ConnHash::default();
        for (slot, i) in self.instances.iter().enumerate() {
            let slot = slot as u32;
            spatial
                .insert(slot, &self.cover_box(i.part, &i.pose).expect("placed"))
                .expect("placed");
            conns.insert(
                slot,
                self.placed_connectors(i.part, &i.pose).expect("placed"),
            );
        }
        let ids: Vec<InstanceId> = self.instances.iter().map(|i| i.id).collect();
        let mut cells: Vec<_> = spatial
            .snapshot()
            .into_iter()
            .map(|(c, s)| (c, ids[s as usize]))
            .collect();
        cells.sort_unstable();
        let mut connectors: Vec<_> = conns
            .snapshot()
            .into_iter()
            .map(|(p, s, k, d)| (p, ids[s as usize], k, d))
            .collect();
        connectors.sort_unstable();
        IndexSnapshot {
            cells,
            connectors,
            bom: self.bom_counts_from_scratch(),
        }
    }

    /// Slots near `b` (candidates for overlap), as ids ascending: for the property tests.
    pub fn candidates_near(&self, b: &Aabb) -> Result<Vec<InstanceId>, EngineError> {
        let mut ids: Vec<_> = self
            .spatial
            .near(b)?
            .into_iter()
            .map(|s| self.instances.id_of(s))
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }

    /// Entries of the spatial hash and of the connector hash: how big the indexes are.
    pub fn index_entries(&self) -> (usize, usize) {
        (self.spatial.entries(), self.conns.entries())
    }

    pub(crate) fn spatial(&self) -> &SpatialHash {
        &self.spatial
    }

    /// The pose of placed part `id` turned by `quarter_turns` about the
    /// vertical axis through its body centre, moved by the least amount (up to
    /// half a stud, towards minus X and minus Z on a tie) that puts its studs
    /// back on the stud grid: a 1 x 2 turned in place moves half a stud.
    pub fn turned_pose(
        &self,
        id: InstanceId,
        quarter_turns: i32,
    ) -> Result<LatticePose, EngineError> {
        let inst = self.instance(id).ok_or(EngineError::UnknownInstance(id))?;
        let shape = &self.part(inst.part)?.shape;
        let turn = Rot24::yaw_quarter_turns(quarter_turns.rem_euclid(4) as u8);
        let rot = turn.compose(inst.pose.rot);
        let old = shape.body.transformed(inst.pose.rot, inst.pose.pos);
        let local = shape.body.transformed(rot, [0, 0, 0]);
        // Twice the centres, to stay in integers.
        let mut pos = inst.pose.pos;
        for k in [0, 2] {
            let want2 = old.min[k] + old.max[k];
            let have2 = local.min[k] + local.max[k];
            pos[k] = (want2 - have2).div_euclid(2);
        }
        pos[1] = old.max[1] - local.max[1];
        // Put the first vertical stud or receptacle on the grid; the rest follow.
        if let Some(c) = shape
            .connectors
            .iter()
            .map(|c| c.transformed(rot, pos))
            .find(|c| c.kind.is_stud_kind() && c.dir[1] != 0)
        {
            for k in [0, 2] {
                let off = (c.pos[k] - openbrick_units::HALF_STUD_LDU)
                    .rem_euclid(openbrick_units::STUD_PITCH_LDU);
                if off != 0 {
                    pos[k] -= if off <= openbrick_units::HALF_STUD_LDU {
                        off
                    } else {
                        off - openbrick_units::STUD_PITCH_LDU
                    };
                }
            }
        }
        Ok(LatticePose { pos, rot })
    }
}

impl PartialEq for Model<'_> {
    /// Two models are equal when they place the same parts in the same order
    /// with the same ids, poses and colours (indexes and history aside).
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.catalog, other.catalog) && self.instances == other.instances
    }
}
