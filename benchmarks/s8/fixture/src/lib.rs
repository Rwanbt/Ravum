//! Fixture S8 : petit crate multi-modules à comportement stable.
//!
//! Chaque tâche du corpus S8 remplace exactement une ligne de corps de fonction
//! (ou une ligne de signature / d'appel), ce qui permet aux scénarios A, B et C
//! de partir des mêmes demandes d'édition et de comparer des fins d'exécution
//! strictement équivalentes sur le plan textuel.

pub mod alpha;
pub mod beta;
pub mod delta;
pub mod gamma;
