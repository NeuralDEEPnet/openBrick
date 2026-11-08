//! The instance table and the edits every command is made of (docs/plan/07
//! section 2): a struct-of-arrays table in placement order, the spatial hash of
//! the body boxes and the connector hash, kept in step by [`Model`]'s
//! primitive operations, which check collision and the lattice first.

use std::collections::HashMap;

use openbrick_catalog::{Aabb, Catalog, CatalogPart, Conn, Palette, PartId};
use openbrick_units::{LatticePose, Rot24};

use crate::history::History;
use crate::spatial::{ConnHash, SpatialHash};
use crate::{on_fine_grid, on_stud_grid};

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

/// Struct-of-arrays instance table, in placement order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Instances {
    pub id: Vec<InstanceId>,
    pub part: Vec<PartId>,
    pub pose: Vec<LatticePose>,
    pub colour: Vec<u16>,
}

impl Instances {
    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    pub fn get(&self, index: usize) -> Instance {
        Instance {
            id: self.id[index],
            part: self.part[index],
            pose: self.pose[index],
            colour: self.colour[index],
        }
    }

    fn insert(&mut self, index: usize, i: Instance) {
        self.id.insert(index, i.id);
        self.part.insert(index, i.part);
        self.pose.insert(index, i.pose);
        self.colour.insert(index, i.colour);
    }

    fn remove(&mut self, index: usize) -> Instance {
        Instance {
            id: self.id.remove(index),
            part: self.part.remove(index),
            pose: self.pose.remove(index),
            colour: self.colour.remove(index),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Instance> + '_ {
        (0..self.len()).map(|i| self.get(i))
    }
}

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
    #[error("the part's studs or receptacles would be off the lattice at {pos:?}; positions come from snapping, or put each one on a multiple of half a stud in X and Z and of half a plate in Y")]
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
    #[error("position {pos:?} is beyond {limit} LDU from the origin; keep every coordinate within plus or minus {limit}")]
    OutOfRange { pos: [i32; 3], limit: i32 },
}

/// A model: the catalog it places parts from, the instance table and its
/// indexes, and the command history.
#[derive(Clone, Debug)]
pub struct Model<'c> {
    catalog: &'c Catalog,
    pub(crate) instances: Instances,
    index: HashMap<InstanceId, usize>,
    spatial: SpatialHash,
    pub(crate) conns: ConnHash,
    next_id: InstanceId,
    pub(crate) history: History,
}

impl<'c> Model<'c> {
    /// An empty model over `catalog`.
    pub fn new(catalog: &'c Catalog) -> Model<'c> {
        Model {
            catalog,
            instances: Instances::default(),
            index: HashMap::new(),
            spatial: SpatialHash::default(),
            conns: ConnHash::default(),
            next_id: 1,
            history: History::default(),
        }
    }

    pub fn catalog(&self) -> &'c Catalog {
        self.catalog
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

    /// True when the spatial and connector hashes hold nothing: after every
    /// part is removed they must be empty again (a property test checks it).
    pub fn indexes_are_empty(&self) -> bool {
        self.spatial.is_empty() && self.conns.is_empty()
    }

    /// The id the next added part gets.
    pub fn next_id(&self) -> InstanceId {
        self.next_id
    }

    /// The placed part with this id.
    pub fn instance(&self, id: InstanceId) -> Option<Instance> {
        self.index.get(&id).map(|&i| self.instances.get(i))
    }

    pub(crate) fn index_of(&self, id: InstanceId) -> Result<usize, EngineError> {
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
        // Vertical studs and receptacles must be on the lattice: the stud grid for a
        // built-in part, the finer one (half a stud, half a plate) for a part of an LDraw
        // pack, whose studs may be half a stud apart. A part turned on its side is
        // checked by its box alone, and pins and axles have no grid of their own.
        let builtin = self.part(part)?.origin == openbrick_catalog::Origin::Builtin;
        for c in self.placed_connectors(part, pose)? {
            let on = if builtin {
                on_stud_grid(c.pos)
            } else {
                on_fine_grid(c.pos)
            };
            if c.kind.is_stud_kind() && c.dir[1] != 0 && !on {
                return Err(EngineError::OffGrid { pos: c.pos });
            }
        }
        let b = self.body_box(part, pose)?;
        if b.max[1] > 0 {
            return Err(EngineError::BelowGround { depth: b.max[1] });
        }
        if let Some(with) = self.collision(&b, skip) {
            let name = self
                .part(self.instance(with).expect("indexed").part)?
                .name
                .clone();
            return Err(EngineError::Collides { with, name });
        }
        Ok(())
    }

    /// The first placed part (lowest id) whose body overlaps `b`, ignoring `skip`.
    pub fn collision(&self, b: &Aabb, skip: Option<InstanceId>) -> Option<InstanceId> {
        self.spatial.near(b).into_iter().find(|&id| {
            Some(id) != skip && {
                let i = self.instance(id).expect("indexed");
                self.catalog.parts[i.part as usize]
                    .shape
                    .body
                    .transformed(i.pose.rot, i.pose.pos)
                    .overlaps(b)
            }
        })
    }

    /// How many connectors of `part` at `pose` mate with placed parts (other
    /// than `skip`).
    pub fn mates(
        &self,
        part: PartId,
        pose: &LatticePose,
        skip: Option<InstanceId>,
    ) -> Result<u32, EngineError> {
        Ok(self
            .placed_connectors(part, pose)?
            .iter()
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

    /// Insert `inst` at table position `index` (clamped to the end), after
    /// checking it. The building block of add and of undoing a removal.
    pub(crate) fn insert(&mut self, index: usize, inst: Instance) -> Result<(), EngineError> {
        if inst.id == 0 {
            return Err(EngineError::ZeroId);
        }
        if self.index.contains_key(&inst.id) {
            return Err(EngineError::DuplicateId(inst.id));
        }
        Model::check_colour(inst.colour)?;
        self.check_pose(inst.part, &inst.pose, None)?;
        let index = index.min(self.instances.len());
        let b = self.body_box(inst.part, &inst.pose)?;
        let conns = self.placed_connectors(inst.part, &inst.pose)?;
        self.instances.insert(index, inst);
        self.reindex_from(index);
        self.spatial.insert(inst.id, &b);
        self.conns.insert(inst.id, &conns);
        self.next_id = self.next_id.max(inst.id.saturating_add(1));
        Ok(())
    }

    /// Remove the placed part `id`; returns it and where it was in the table.
    pub(crate) fn delete(&mut self, id: InstanceId) -> Result<(usize, Instance), EngineError> {
        let index = self.index_of(id)?;
        let inst = self.instances.remove(index);
        self.index.remove(&id);
        self.reindex_from(index);
        let b = self.body_box(inst.part, &inst.pose)?;
        let conns = self.placed_connectors(inst.part, &inst.pose)?;
        self.spatial.remove(id, &b);
        self.conns.remove(id, &conns);
        Ok((index, inst))
    }

    /// Move the placed part `id` to `pose`; returns the old pose.
    pub(crate) fn set_pose(
        &mut self,
        id: InstanceId,
        pose: LatticePose,
    ) -> Result<LatticePose, EngineError> {
        let index = self.index_of(id)?;
        let inst = self.instances.get(index);
        self.check_pose(inst.part, &pose, Some(id))?;
        let (old_box, old_conns) = (
            self.body_box(inst.part, &inst.pose)?,
            self.placed_connectors(inst.part, &inst.pose)?,
        );
        self.spatial.remove(id, &old_box);
        self.conns.remove(id, &old_conns);
        self.instances.pose[index] = pose;
        self.spatial.insert(id, &self.body_box(inst.part, &pose)?);
        let conns = self.placed_connectors(inst.part, &pose)?;
        self.conns.insert(id, &conns);
        Ok(inst.pose)
    }

    /// Recolour the placed part `id`; returns the old colour.
    pub(crate) fn set_colour(&mut self, id: InstanceId, colour: u16) -> Result<u16, EngineError> {
        Model::check_colour(colour)?;
        let index = self.index_of(id)?;
        Ok(std::mem::replace(&mut self.instances.colour[index], colour))
    }

    fn reindex_from(&mut self, from: usize) {
        for i in from..self.instances.len() {
            self.index.insert(self.instances.id[i], i);
        }
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
