//! Hot reload U3 (L0 style, L1 structure, L2 comportement), transactionnel.
//!
//! Un rechargement apporte une description, des liaisons (chemin de nœud →
//! nom de gestionnaire) et une table de comportement versionnée. Tout est
//! vérifié **avant** d'activer quoi que ce soit ; en cas de refus, l'ancienne
//! version reste active en entier (vue, liaisons, comportement), et l'état
//! de l'application n'est jamais touché par un rechargement.

use crate::{Desc, Event, PatchError, Path, Ui};
use std::collections::BTreeMap;

/// Niveau d'un changement de vue, du moins coûteux au plus coûteux.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Rien ne change.
    None,
    /// L0 : seuls des styles changent ; mêmes clés, même texte.
    Style,
    /// L1 : structure ou texte.
    Structure,
}

pub fn classify(old: &Desc, new: &Desc) -> Level {
    if old.key != new.key || old.text != new.text || old.children.len() != new.children.len() {
        return Level::Structure;
    }
    let mut level = if old.style == new.style {
        Level::None
    } else {
        Level::Style
    };
    for (a, b) in old.children.iter().zip(&new.children) {
        level = level.max(classify(a, b));
        if level == Level::Structure {
            break;
        }
    }
    level
}

pub type Handler<S> = fn(&mut S);

/// L2 : comportement versionné.
pub struct Behavior<S> {
    pub version: u64,
    pub handlers: BTreeMap<String, Handler<S>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReloadError {
    Patch(PatchError),
    /// Une liaison vise un nœud absent de la nouvelle vue.
    UnboundPath(Path),
    /// Une liaison nomme un gestionnaire absent du comportement.
    MissingHandler {
        path: Path,
        handler: String,
    },
    /// La version du comportement ne croît pas.
    BehaviorNotNewer {
        active: u64,
        got: u64,
    },
    /// Le gate Verify refuse : diagnostics nouveaux par rapport à la
    /// version active.
    VerifyRegression(Vec<crate::verify::Finding>),
    /// Le ruleset de Verify a été modifié sans mise à jour de son empreinte.
    RulesetTampered,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub view: Level,
    pub behavior_changed: bool,
}

pub struct Session<S> {
    ui: Ui,
    view: Desc,
    bindings: BTreeMap<Path, String>,
    behavior: Behavior<S>,
    pub state: S,
    /// Refuser un rechargement qui ajoute des diagnostics Verify.
    pub verify_gate: bool,
    /// Type d'entrée transmis à Verify (cibles tactiles).
    pub input: crate::responsive::Input,
}

fn check<S>(
    desc: &Desc,
    bindings: &BTreeMap<Path, String>,
    behavior: &Behavior<S>,
) -> Result<(), ReloadError> {
    for (path, h) in bindings {
        if find(desc, path).is_none() {
            return Err(ReloadError::UnboundPath(path.clone()));
        }
        if !behavior.handlers.contains_key(h) {
            return Err(ReloadError::MissingHandler {
                path: path.clone(),
                handler: h.clone(),
            });
        }
    }
    Ok(())
}

fn find<'a>(root: &'a Desc, p: &[u64]) -> Option<&'a Desc> {
    let (first, rest) = p.split_first()?;
    if *first != root.key {
        return None;
    }
    let mut d = root;
    for k in rest {
        d = d.children.iter().find(|c| c.key == *k)?;
    }
    Some(d)
}

impl<S> Session<S> {
    pub fn new(
        view: Desc,
        bindings: BTreeMap<Path, String>,
        behavior: Behavior<S>,
        state: S,
        viewport: (f32, f32),
    ) -> Result<Session<S>, ReloadError> {
        check(&view, &bindings, &behavior)?;
        let ui = Ui::new(view.clone(), viewport).map_err(ReloadError::Patch)?;
        Ok(Session {
            ui,
            view,
            bindings,
            behavior,
            state,
            verify_gate: true,
            input: crate::responsive::Input::Pointer,
        })
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }
    pub fn behavior_version(&self) -> u64 {
        self.behavior.version
    }

    /// Rechargement atomique. `behavior: None` garde le comportement actif.
    pub fn reload(
        &mut self,
        base: u64,
        view: Desc,
        bindings: BTreeMap<Path, String>,
        behavior: Option<Behavior<S>>,
    ) -> Result<Report, ReloadError> {
        if base != self.ui.generation() {
            return Err(ReloadError::Patch(PatchError::Stale {
                expected: self.ui.generation(),
                got: base,
            }));
        }
        if let Some(b) = &behavior {
            if b.version <= self.behavior.version {
                return Err(ReloadError::BehaviorNotNewer {
                    active: self.behavior.version,
                    got: b.version,
                });
            }
        }
        check(
            &view,
            &bindings,
            behavior.as_ref().unwrap_or(&self.behavior),
        )?;
        // Vue candidate construite à part : l'active n'est touchée qu'après
        // tous les contrôles.
        let level = classify(&self.view, &view);
        if self.verify_gate {
            let mut probe =
                Ui::new(view.clone(), self.ui.viewport()).map_err(ReloadError::Patch)?;
            probe.copy_states_from(&self.ui);
            let ctx = crate::verify::Context {
                tokens: None,
                input: self.input,
            };
            match crate::verify::activation_gate(&self.ui, &probe, &ctx) {
                Ok(()) => {}
                Err(crate::verify::GateError::Regression(new)) => {
                    return Err(ReloadError::VerifyRegression(new))
                }
                Err(crate::verify::GateError::Tampered(_)) => {
                    return Err(ReloadError::RulesetTampered)
                }
            }
        }
        self.ui
            .patch(base, view.clone())
            .map_err(ReloadError::Patch)?;
        self.view = view;
        self.bindings = bindings;
        let behavior_changed = behavior.is_some();
        if let Some(b) = behavior {
            self.behavior = b;
        }
        Ok(Report {
            view: level,
            behavior_changed,
        })
    }

    /// Un clic exécute le gestionnaire lié au nœud lié le plus profond.
    pub fn dispatch(&mut self, e: &Event) {
        self.ui.dispatch(e);
        for p in self.ui.take_clicks() {
            let bound = self
                .bindings
                .iter()
                .filter(|(b, _)| p.starts_with(b))
                .max_by_key(|(b, _)| b.len());
            if let Some((_, h)) = bound {
                (self.behavior.handlers[h])(&mut self.state);
            }
        }
    }
}
