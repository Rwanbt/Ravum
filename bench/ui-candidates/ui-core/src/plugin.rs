//! U4 (logique) : composants façon Cordis — services typés par nom,
//! dépendances, cycle de vie, panneaux et commandes, nettoyage automatique,
//! permissions, ajout, mise à jour et retrait **transactionnels**.
//!
//! Chaque opération construit l'ensemble candidat des plugins, le valide en
//! entier (permissions, services, cycles, collisions de clés, gate Verify),
//! puis seulement l'active. Un refus ne change rien. Tout ce qu'un plugin a
//! apporté (panneaux, commandes, services) est rattaché à lui et disparaît
//! avec lui.
//!
//! L'isolation des plugins non fiables (WASM ou processus) n'est pas ici :
//! elle dépend du runtime retenu.

use crate::hot::Handler;
use crate::{Desc, PatchError, Ui};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    AddPanel,
    Command,
    Provide(String),
    Use(String),
}

pub struct Plugin<S> {
    pub id: String,
    pub version: u64,
    pub provides: Vec<String>,
    pub requires: Vec<String>,
    pub panels: Vec<Desc>,
    pub commands: BTreeMap<String, Handler<S>>,
}

impl<S> Plugin<S> {
    /// Permissions dont le plugin a besoin pour ce qu'il déclare.
    pub fn needs(&self) -> BTreeSet<Permission> {
        let mut n = BTreeSet::new();
        if !self.panels.is_empty() {
            n.insert(Permission::AddPanel);
        }
        if !self.commands.is_empty() {
            n.insert(Permission::Command);
        }
        n.extend(self.provides.iter().cloned().map(Permission::Provide));
        n.extend(self.requires.iter().cloned().map(Permission::Use));
        n
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PluginError {
    AlreadyInstalled(String),
    NotInstalled(String),
    NotNewer {
        id: String,
        active: u64,
        got: u64,
    },
    PermissionDenied {
        id: String,
        permission: Permission,
    },
    MissingService {
        id: String,
        service: String,
    },
    DuplicateService {
        service: String,
        a: String,
        b: String,
    },
    Cycle(Vec<String>),
    KeyCollision {
        key: u64,
        a: String,
        b: String,
    },
    /// Retrait refusé : d'autres plugins utilisent ses services.
    InUse {
        id: String,
        by: Vec<String>,
    },
    Patch(PatchError),
    VerifyRegression(Vec<crate::verify::Finding>),
    /// Le ruleset de Verify a été modifié sans mise à jour de son empreinte.
    RulesetTampered,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Activated(String, u64),
    Deactivated(String, u64),
}

struct Installed<S> {
    plugin: Plugin<S>,
    granted: BTreeSet<Permission>,
}

pub struct Host<S> {
    shell: Desc,
    slot: u64,
    plugins: BTreeMap<String, Installed<S>>,
    ui: Ui,
    pub state: S,
    pub verify_gate: bool,
    /// Type d'entrée transmis à Verify (cibles tactiles).
    pub input: crate::responsive::Input,
    pub log: Vec<Lifecycle>,
}

/// Ordre d'activation : un fournisseur avant ses utilisateurs ; refus si
/// un service manque, est fourni deux fois, ou si les dépendances bouclent.
fn order<S>(set: &BTreeMap<String, Installed<S>>) -> Result<Vec<String>, PluginError> {
    let mut provider: BTreeMap<&str, &str> = BTreeMap::new();
    for (id, i) in set {
        for s in &i.plugin.provides {
            if let Some(other) = provider.insert(s, id) {
                return Err(PluginError::DuplicateService {
                    service: s.clone(),
                    a: other.to_string(),
                    b: id.clone(),
                });
            }
        }
    }
    let mut deps: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (id, i) in set {
        let mut d = BTreeSet::new();
        for s in &i.plugin.requires {
            match provider.get(s.as_str()) {
                Some(p) => {
                    d.insert(*p);
                }
                None => {
                    return Err(PluginError::MissingService {
                        id: id.clone(),
                        service: s.clone(),
                    })
                }
            }
        }
        deps.insert(id, d);
    }
    // Parcours en profondeur déterministe (ordre des identifiants).
    fn visit<'a>(
        n: &'a str,
        deps: &BTreeMap<&'a str, BTreeSet<&'a str>>,
        done: &mut BTreeSet<&'a str>,
        stack: &mut Vec<&'a str>,
        out: &mut Vec<String>,
    ) -> Result<(), PluginError> {
        if done.contains(n) {
            return Ok(());
        }
        if let Some(i) = stack.iter().position(|x| *x == n) {
            let mut c: Vec<String> = stack[i..].iter().map(|s| s.to_string()).collect();
            c.push(n.to_string());
            return Err(PluginError::Cycle(c));
        }
        stack.push(n);
        for d in &deps[n] {
            visit(d, deps, done, stack, out)?;
        }
        stack.pop();
        done.insert(n);
        out.push(n.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    let mut done = BTreeSet::new();
    for id in deps.keys() {
        visit(id, &deps, &mut done, &mut Vec::new(), &mut out)?;
    }
    Ok(out)
}

impl<S> Host<S> {
    /// `shell` doit contenir, directement sous la racine, le conteneur
    /// `slot` qui reçoit les panneaux.
    pub fn new(
        shell: Desc,
        slot: u64,
        state: S,
        viewport: (f32, f32),
    ) -> Result<Host<S>, PluginError> {
        let ui = Ui::new(shell.clone(), viewport).map_err(PluginError::Patch)?;
        Ok(Host {
            shell,
            slot,
            plugins: BTreeMap::new(),
            ui,
            state,
            verify_gate: true,
            input: crate::responsive::Input::Pointer,
            log: Vec::new(),
        })
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }
    pub fn installed(&self) -> Vec<(String, u64)> {
        self.plugins
            .iter()
            .map(|(k, i)| (k.clone(), i.plugin.version))
            .collect()
    }

    /// Commandes visibles, préfixées par l'identifiant du plugin.
    pub fn commands(&self) -> Vec<String> {
        self.plugins
            .iter()
            .flat_map(|(id, i)| i.plugin.commands.keys().map(move |c| format!("{id}.{c}")))
            .collect()
    }

    pub fn run(&mut self, qualified: &str) -> bool {
        let Some((id, c)) = qualified.split_once('.') else {
            return false;
        };
        let Some(h) = self
            .plugins
            .get(id)
            .and_then(|i| i.plugin.commands.get(c))
            .copied()
        else {
            return false;
        };
        h(&mut self.state);
        true
    }

    pub fn install(
        &mut self,
        p: Plugin<S>,
        granted: BTreeSet<Permission>,
    ) -> Result<(), PluginError> {
        if self.plugins.contains_key(&p.id) {
            return Err(PluginError::AlreadyInstalled(p.id));
        }
        self.commit(Some(Installed { plugin: p, granted }), None)
    }

    /// Remplace un plugin installé par une version plus récente ; les
    /// permissions accordées restent celles de l'installation.
    pub fn update(&mut self, p: Plugin<S>) -> Result<(), PluginError> {
        let Some(old) = self.plugins.get(&p.id) else {
            return Err(PluginError::NotInstalled(p.id));
        };
        if p.version <= old.plugin.version {
            return Err(PluginError::NotNewer {
                id: p.id.clone(),
                active: old.plugin.version,
                got: p.version,
            });
        }
        let granted = old.granted.clone();
        self.commit(Some(Installed { plugin: p, granted }), None)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), PluginError> {
        let Some(target) = self.plugins.get(id) else {
            return Err(PluginError::NotInstalled(id.to_string()));
        };
        let by: Vec<String> = self
            .plugins
            .iter()
            .filter(|(o, i)| {
                *o != id
                    && i.plugin
                        .requires
                        .iter()
                        .any(|s| target.plugin.provides.contains(s))
            })
            .map(|(o, _)| o.clone())
            .collect();
        if !by.is_empty() {
            return Err(PluginError::InUse {
                id: id.to_string(),
                by,
            });
        }
        self.commit(None, Some(id))
    }

    fn compose(
        &self,
        set: &BTreeMap<String, Installed<S>>,
        order: &[String],
    ) -> Result<Desc, PluginError> {
        let mut root = self.shell.clone();
        let slot = root
            .children
            .iter_mut()
            .find(|c| c.key == self.slot)
            .expect("shell has its slot");
        let mut owner: BTreeMap<u64, String> = slot
            .children
            .iter()
            .map(|c| (c.key, "<shell>".to_string()))
            .collect();
        for id in order {
            for panel in &set[id].plugin.panels {
                if let Some(a) = owner.insert(panel.key, id.clone()) {
                    return Err(PluginError::KeyCollision {
                        key: panel.key,
                        a,
                        b: id.clone(),
                    });
                }
                slot.children.push(panel.clone());
            }
        }
        Ok(root)
    }

    /// Valide l'ensemble candidat entier, puis active ; sinon ne change rien.
    fn commit(
        &mut self,
        add: Option<Installed<S>>,
        remove: Option<&str>,
    ) -> Result<(), PluginError> {
        // Contrôles sur ce qui entre, avant de toucher à l'ensemble actif.
        if let Some(i) = &add {
            for p in i.plugin.needs() {
                if !i.granted.contains(&p) {
                    return Err(PluginError::PermissionDenied {
                        id: i.plugin.id.clone(),
                        permission: p,
                    });
                }
            }
        }
        let mut candidate: BTreeMap<String, Installed<S>> = BTreeMap::new();
        let mut taken = std::mem::take(&mut self.plugins);
        let (new_id, new_version) = add.as_ref().map_or((String::new(), 0), |i| {
            (i.plugin.id.clone(), i.plugin.version)
        });
        let replaced = add.as_ref().and_then(|i| taken.remove(&i.plugin.id));
        let removed = remove.and_then(|id| taken.remove(id));
        candidate.append(&mut taken);
        if let Some(i) = add {
            candidate.insert(new_id.clone(), i);
        }
        let result = (|| {
            let ord = order(&candidate)?;
            let view = self.compose(&candidate, &ord)?;
            if self.verify_gate {
                let mut probe =
                    Ui::new(view.clone(), self.ui.viewport()).map_err(PluginError::Patch)?;
                probe.copy_states_from(&self.ui);
                let ctx = crate::verify::Context {
                    tokens: None,
                    input: self.input,
                };
                match crate::verify::activation_gate(&self.ui, &probe, &ctx) {
                    Ok(()) => {}
                    Err(crate::verify::GateError::Regression(new)) => {
                        return Err(PluginError::VerifyRegression(new))
                    }
                    Err(crate::verify::GateError::Tampered(_)) => {
                        return Err(PluginError::RulesetTampered)
                    }
                }
            }
            self.ui
                .patch(self.ui.generation(), view)
                .map_err(PluginError::Patch)?;
            Ok(ord)
        })();
        match result {
            Ok(_order) => {
                // Cycle de vie : désactivation en ordre inverse, activation
                // dans l'ordre des dépendances.
                if let Some(old) = &replaced {
                    self.log.push(Lifecycle::Deactivated(
                        old.plugin.id.clone(),
                        old.plugin.version,
                    ));
                }
                if let Some(old) = &removed {
                    self.log.push(Lifecycle::Deactivated(
                        old.plugin.id.clone(),
                        old.plugin.version,
                    ));
                }
                if !new_id.is_empty() {
                    self.log.push(Lifecycle::Activated(new_id, new_version));
                }
                self.plugins = candidate;
                Ok(())
            }
            Err(e) => {
                // Rollback : on remet exactement l'ensemble actif.
                if !new_id.is_empty() {
                    candidate.remove(&new_id);
                }
                if let Some(old) = replaced {
                    candidate.insert(old.plugin.id.clone(), old);
                }
                if let Some(old) = removed {
                    candidate.insert(old.plugin.id.clone(), old);
                }
                self.plugins = candidate;
                Err(e)
            }
        }
    }
}
