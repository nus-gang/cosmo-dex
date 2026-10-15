//! Forward-only S3 dependency closure. No owner connected-component expansion.
//! The signed-order sequencer supplies canonical identities and admission order.
use crate::Result;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Pending,
    SubmissionUnknown,
    Committed,
    Corrected,
}
impl State {
    pub fn pending(self) -> bool {
        matches!(self, Self::Pending | Self::SubmissionUnknown)
    }
}

/// Debit domains are deliberately asset-specific; provisional receipts are not
/// domains. An owner buying QUOTE-funded BASE does not taint its existing BASE.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DebitDomain {
    pub owner: String,
    pub epoch: u64,
    pub asset: Asset,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Asset {
    Base,
    Quote,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub fill_id: String,
    pub command_seq: u64,
    pub match_index: u32,
    /// Buyer order, seller order.
    pub orders: [String; 2],
    /// Buyer QUOTE debit, seller BASE debit.
    pub debits: [DebitDomain; 2],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub identity: Identity,
    pub predecessors: Vec<String>,
    pub state: State,
    pub revision: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
    order: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closure {
    /// Original (command_seq, match_index) order, never API-page truncated.
    pub corrected: Vec<String>,
    pub surviving_pending: Vec<String>,
    pub affected_orders: BTreeSet<String>,
    pub affected_debits: BTreeSet<DebitDomain>,
}

impl Graph {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.order.iter().map(|id| &self.nodes[id])
    }
    /// Compute the latest pending predecessor in each of the four domains.
    /// The scan preserves historical edges when an older fill becomes terminal.
    /// It makes no throughput claim; indexing can be added after measurement.
    pub fn append(&mut self, identity: Identity) -> Result<&Node> {
        if identity.fill_id.is_empty()
            || identity.command_seq == 0
            || identity.orders.iter().any(String::is_empty)
            || identity.orders[0] == identity.orders[1]
            || identity.debits[0].owner.is_empty()
            || identity.debits[1].owner.is_empty()
            || identity.debits[0].owner == identity.debits[1].owner
            || identity.debits[0].asset != Asset::Quote
            || identity.debits[1].asset != Asset::Base
        {
            return Err("DEPENDENCY_IDENTITY");
        }
        if self.nodes.contains_key(&identity.fill_id) {
            return Err("FILL_ALREADY_BOUND");
        }
        if self.order.last().is_some_and(|id| {
            let old = &self.nodes[id].identity;
            (old.command_seq, old.match_index) >= (identity.command_seq, identity.match_index)
        }) {
            return Err("FILL_ORDER");
        }
        let mut selected = BTreeSet::new();
        for order in &identity.orders {
            if let Some(id) = self.order.iter().rev().find(|id| {
                let n = &self.nodes[*id];
                n.state.pending() && n.identity.orders.contains(order)
            }) {
                selected.insert(id.clone());
            }
        }
        for domain in &identity.debits {
            if let Some(id) = self.order.iter().rev().find(|id| {
                let n = &self.nodes[*id];
                n.state.pending() && n.identity.debits.contains(domain)
            }) {
                selected.insert(id.clone());
            }
        }
        let predecessors = self
            .order
            .iter()
            .filter(|id| selected.contains(*id))
            .cloned()
            .collect();
        let id = identity.fill_id.clone();
        self.order.push(id.clone());
        self.nodes.insert(
            id.clone(),
            Node {
                identity,
                predecessors,
                state: State::Pending,
                revision: 1,
            },
        );
        Ok(&self.nodes[&id])
    }
    /// Used during replay to reject missing, reordered or fabricated edges.
    pub fn replay_append(&mut self, identity: Identity, predecessors: &[String]) -> Result<()> {
        let mut next = self.clone();
        if next.append(identity)?.predecessors != predecessors {
            return Err("DEPENDENCY_REPLAY_MISMATCH");
        }
        *self = next;
        Ok(())
    }
    pub fn closure(&self, roots: &[String]) -> Result<Closure> {
        let mut corrected = BTreeSet::new();
        for id in roots {
            let n = self.nodes.get(id).ok_or("FILL_NOT_FOUND")?;
            if n.state == State::Committed {
                return Err("COMMITTED_IMMUTABLE");
            }
            // Previously corrected roots may be replayed without new effects.
            corrected.insert(id.clone());
        }
        let mut result = Closure {
            corrected: vec![],
            surviving_pending: vec![],
            affected_orders: BTreeSet::new(),
            affected_debits: BTreeSet::new(),
        };
        for id in &self.order {
            let n = &self.nodes[id];
            if n.state == State::Committed {
                continue;
            }
            if corrected.contains(id) || n.predecessors.iter().any(|p| corrected.contains(p)) {
                corrected.insert(id.clone());
                result.corrected.push(id.clone());
                result
                    .affected_orders
                    .extend(n.identity.orders.iter().cloned());
                result
                    .affected_debits
                    .extend(n.identity.debits.iter().cloned());
            } else if n.state.pending() {
                result.surviving_pending.push(id.clone());
            }
        }
        Ok(result)
    }
    fn transition(&mut self, ids: &[String], state: State) -> Result<()> {
        // Validate the entire set before mutation, including overflow.
        let mut unique = BTreeSet::new();
        for id in ids {
            if !unique.insert(id) {
                return Err("DUPLICATE_FILL");
            }
            let old = self.nodes.get(id).ok_or("FILL_NOT_FOUND")?;
            if old.state != state {
                if !old.state.pending() {
                    return Err("TERMINAL_FILL_IMMUTABLE");
                }
                old.revision.checked_add(1).ok_or("INTEGER_OVERFLOW")?;
            }
        }
        for id in ids {
            let old = self.nodes.get_mut(id).unwrap();
            if old.state != state {
                old.state = state;
                old.revision += 1;
            }
        }
        Ok(())
    }
    pub fn submission_unknown(&mut self, ids: &[String]) -> Result<()> {
        self.transition(ids, State::SubmissionUnknown)
    }
    /// Caller must have independently verified a COMMITTED receipt first.
    pub fn committed(&mut self, ids: &[String]) -> Result<()> {
        self.transition(ids, State::Committed)
    }
    /// Caller must have resolved every attempt and verified original failure +
    /// VOID. This primitive does not infer those facts from timeout or closure.
    pub fn corrected(&mut self, roots: &[String]) -> Result<Closure> {
        let closure = self.closure(roots)?;
        self.transition(&closure.corrected, State::Corrected)?;
        Ok(closure)
    }
}
