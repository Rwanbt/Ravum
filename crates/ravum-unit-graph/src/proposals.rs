//! Conservative proposal conflict detection.
//!
//! Two proposals that change the same path of the same unit with different
//! contents are never silently merged: the decision is `MANUAL_REQUIRED`.
//! Identical proposals from several agents are coalesced.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One proposed file change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub unit: String,
    pub path: String,
    pub content: String,
    pub agent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ProposalDecision {
    /// All proposals for this unit/path agree (or only one exists).
    Accepted {
        unit: String,
        path: String,
        agents: Vec<String>,
    },
    /// Proposals conflict or overlap ambiguously: a human must decide.
    ManualRequired { unit: String, reason: String },
}

/// A set of proposals submitted by several agents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalSet {
    proposals: Vec<Proposal>,
}

impl ProposalSet {
    pub fn new(proposals: Vec<Proposal>) -> Self {
        Self { proposals }
    }

    pub fn proposals(&self) -> &[Proposal] {
        &self.proposals
    }

    /// Deterministic decisions, one per unit/path. A conflict on any path of a
    /// unit rejects the whole unit (`MANUAL_REQUIRED`), never a partial merge.
    pub fn decide(&self) -> Vec<ProposalDecision> {
        let mut by_unit: BTreeMap<String, Vec<&Proposal>> = BTreeMap::new();
        for proposal in &self.proposals {
            by_unit
                .entry(proposal.unit.clone())
                .or_default()
                .push(proposal);
        }
        let mut decisions = Vec::new();
        for (unit, proposals) in by_unit {
            let mut by_path: BTreeMap<String, Vec<&Proposal>> = BTreeMap::new();
            for proposal in &proposals {
                by_path
                    .entry(proposal.path.clone())
                    .or_default()
                    .push(proposal);
            }
            let mut conflict = None;
            for (path, path_proposals) in &by_path {
                let first = path_proposals[0].content.as_str();
                if path_proposals
                    .iter()
                    .any(|proposal| proposal.content != first)
                {
                    let agents: Vec<&str> = path_proposals
                        .iter()
                        .map(|proposal| proposal.agent.as_str())
                        .collect();
                    conflict = Some(format!(
                        "unit {unit}: path {path:?} changed differently by agents {agents:?}"
                    ));
                    break;
                }
            }
            if let Some(reason) = conflict {
                decisions.push(ProposalDecision::ManualRequired { unit, reason });
            } else {
                for (path, path_proposals) in by_path {
                    let mut agents: Vec<String> = path_proposals
                        .iter()
                        .map(|proposal| proposal.agent.clone())
                        .collect();
                    agents.sort();
                    agents.dedup();
                    decisions.push(ProposalDecision::Accepted {
                        unit: unit.clone(),
                        path,
                        agents,
                    });
                }
            }
        }
        decisions
    }
}
