//! Commands, and undo and redo with explicit inverses (docs/plan/07 section 8).
//!
//! A [`Command`] is what a person asks for (add a part, remove one, move,
//! turn, recolour, clear). Applying it checks it and runs primitive operations
//! ([`Op`]) on the model; each primitive returns its own inverse, so undo runs
//! the inverses in reverse order and redo runs the recorded operations again.
//! Operations address parts by id, so the same log could later replay into a
//! collaborative document.

use openbrick_units::{LatticePose, Rot24};

use crate::model::{EngineError, Instance, InstanceId, Model};

/// An edit a person asks for. In JSON (the WebAssembly bridge) it is an
/// object tagged by `op`: `{"op":"add","part":"ob.brick-2x4","pos":[0,-24,0],
/// "rot":0,"colour":4}`, `{"op":"remove","id":3}`, `{"op":"move","id":3,
/// "pos":[...],"rot":0}`, `{"op":"turn","id":3,"quarterTurns":1}`,
/// `{"op":"recolour","id":3,"colour":15}`, `{"op":"clear"}`,
/// `{"op":"clearBuilt"}`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    Add {
        /// Catalog id of the part.
        part: String,
        pos: [i32; 3],
        rot: u8,
        colour: u16,
    },
    Remove {
        id: InstanceId,
    },
    Move {
        id: InstanceId,
        pos: [i32; 3],
        rot: u8,
    },
    /// Turn a placed part in place about the vertical axis
    /// ([`Model::turned_pose`]).
    #[serde(rename_all = "camelCase")]
    Turn {
        id: InstanceId,
        quarter_turns: i32,
    },
    Recolour {
        id: InstanceId,
        colour: u16,
    },
    Clear,
    /// Remove every part except baseplates (the page's "Clear": start over on
    /// the same baseplate), as one undoable step.
    ClearBuilt,
}

/// A primitive edit; applying one returns its inverse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Insert { index: usize, inst: Instance },
    Delete { id: InstanceId },
    SetPose { id: InstanceId, pose: LatticePose },
    SetColour { id: InstanceId, colour: u16 },
    Batch(Vec<Op>),
}

/// One applied command: what it did and how to take it back.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    forward: Op,
    inverse: Op,
}

/// Done and undone commands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    done: Vec<Entry>,
    undone: Vec<Entry>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Commands that can be undone.
    pub fn depth(&self) -> usize {
        self.done.len()
    }
}

impl Model<'_> {
    /// Run one primitive; on success returns its inverse. A failed batch
    /// rolls back what it already did, so the model is unchanged.
    fn run(&mut self, op: &Op) -> Result<Op, EngineError> {
        Ok(match op {
            Op::Insert { index, inst } => {
                self.insert(*index, *inst)?;
                Op::Delete { id: inst.id }
            }
            Op::Delete { id } => {
                let (index, inst) = self.delete(*id)?;
                Op::Insert { index, inst }
            }
            Op::SetPose { id, pose } => Op::SetPose {
                id: *id,
                pose: self.set_pose(*id, *pose)?,
            },
            Op::SetColour { id, colour } => Op::SetColour {
                id: *id,
                colour: self.set_colour(*id, *colour)?,
            },
            Op::Batch(ops) => {
                let mut inverses = Vec::with_capacity(ops.len());
                for o in ops {
                    match self.run(o) {
                        Ok(inv) => inverses.push(inv),
                        Err(e) => {
                            for inv in inverses.iter().rev() {
                                self.run(inv).expect("an inverse applies");
                            }
                            return Err(e);
                        }
                    }
                }
                inverses.reverse();
                Op::Batch(inverses)
            }
        })
    }

    /// Apply a command and record it for undo; the redo list is dropped.
    /// Returns the id of the part it added, moved or recoloured, if any.
    pub fn apply(&mut self, cmd: &Command) -> Result<Option<InstanceId>, EngineError> {
        let (op, target) = match cmd {
            Command::Add {
                part,
                pos,
                rot,
                colour,
            } => {
                let part = self
                    .catalog()
                    .find(part)
                    .ok_or_else(|| EngineError::UnknownPart(part.clone()))?;
                let pose = pose(*pos, *rot)?;
                let id = self.next_id();
                if id == InstanceId::MAX {
                    // u32::MAX itself may be taken (a loaded file can use it), and
                    // next_id saturates there: stop rather than guess.
                    return Err(EngineError::IdsExhausted(id));
                }
                let op = Op::Insert {
                    index: self.len(),
                    inst: Instance {
                        id,
                        part,
                        pose,
                        colour: *colour,
                    },
                };
                (op, Some(id))
            }
            Command::Remove { id } => (Op::Delete { id: *id }, None),
            Command::Move { id, pos, rot } => (
                Op::SetPose {
                    id: *id,
                    pose: pose(*pos, *rot)?,
                },
                Some(*id),
            ),
            Command::Turn { id, quarter_turns } => (
                Op::SetPose {
                    id: *id,
                    pose: self.turned_pose(*id, *quarter_turns)?,
                },
                Some(*id),
            ),
            Command::Recolour { id, colour } => (
                Op::SetColour {
                    id: *id,
                    colour: *colour,
                },
                Some(*id),
            ),
            // Last first, so each removal leaves the table positions of the
            // ones before it alone and the inverse puts them back in order.
            Command::Clear => (
                Op::Batch(
                    self.instances
                        .id
                        .iter()
                        .rev()
                        .map(|&id| Op::Delete { id })
                        .collect(),
                ),
                None,
            ),
            Command::ClearBuilt => (
                Op::Batch(
                    self.instances
                        .iter()
                        .filter(|i| !self.catalog().parts[i.part as usize].base)
                        .collect::<Vec<_>>()
                        .iter()
                        .rev()
                        .map(|i| Op::Delete { id: i.id })
                        .collect(),
                ),
                None,
            ),
        };
        // Inserting raises the next id past the new part's.
        let inverse = self.run(&op)?;
        self.history.done.push(Entry {
            forward: op,
            inverse,
        });
        self.history.undone.clear();
        Ok(target)
    }

    /// Take back the last command.
    pub fn undo(&mut self) -> Result<(), EngineError> {
        let entry = self.history.done.pop().ok_or(EngineError::NothingToUndo)?;
        self.run(&entry.inverse).expect("an inverse applies");
        self.history.undone.push(entry);
        Ok(())
    }

    /// Apply the last undone command again.
    pub fn redo(&mut self) -> Result<(), EngineError> {
        let entry = self
            .history
            .undone
            .pop()
            .ok_or(EngineError::NothingToRedo)?;
        self.run(&entry.forward)
            .expect("a command that applied once applies again");
        self.history.done.push(entry);
        Ok(())
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Forget every command (after opening a file).
    pub fn clear_history(&mut self) {
        self.history = History::default();
    }
}

fn pose(pos: [i32; 3], rot: u8) -> Result<LatticePose, EngineError> {
    if rot >= 24 {
        return Err(EngineError::UnknownRotation(rot));
    }
    Ok(LatticePose {
        pos,
        rot: Rot24(rot),
    })
}
