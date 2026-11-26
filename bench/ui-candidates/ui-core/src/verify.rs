//! U8 (logique) : Verify complet — registre de règles versionné, sous-scores,
//! anti-contournement, rapports JSON et SARIF, gate d'activation.
//!
//! Les règles, leurs versions, sévérités et poids forment un **ruleset**
//! dont l'empreinte est figée ([`RULESET_LOCK`]) : toute modification qui ne
//! met pas l'empreinte à jour dans le même changement fait refuser
//! l'analyse (fail-closed). Un agent ne peut donc pas affaiblir une règle en
//! silence ; le changement est visible dans la revue.
//!
//! Anti-contournement : cacher un contenu dans une boîte de taille nulle,
//! ou le pousser hors de la fenêtre, est lui-même une violation.

use crate::responsive::{Input, MIN_TOUCH};
use crate::tokens::{audit, Tokens};
use crate::{Path, Rule as LayoutRule, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Layout,
    Design,
    Accessibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuleSpec {
    pub id: &'static str,
    pub version: u32,
    pub category: Category,
    pub severity: Severity,
    pub weight: u32,
}

pub const RULES: &[RuleSpec] = &[
    RuleSpec {
        id: "layout.overflow",
        version: 1,
        category: Category::Layout,
        severity: Severity::Error,
        weight: 3,
    },
    RuleSpec {
        id: "layout.out-of-container",
        version: 1,
        category: Category::Layout,
        severity: Severity::Error,
        weight: 3,
    },
    RuleSpec {
        id: "layout.overlap",
        version: 1,
        category: Category::Layout,
        severity: Severity::Error,
        weight: 2,
    },
    RuleSpec {
        id: "layout.hidden-content",
        version: 1,
        category: Category::Layout,
        severity: Severity::Error,
        weight: 3,
    },
    RuleSpec {
        id: "layout.off-viewport",
        version: 1,
        category: Category::Layout,
        severity: Severity::Error,
        weight: 3,
    },
    RuleSpec {
        id: "design.off-charter",
        version: 1,
        category: Category::Design,
        severity: Severity::Warning,
        weight: 1,
    },
    RuleSpec {
        id: "a11y.touch-target",
        version: 1,
        category: Category::Accessibility,
        severity: Severity::Error,
        weight: 2,
    },
    RuleSpec {
        id: "a11y.empty-text",
        version: 1,
        category: Category::Accessibility,
        severity: Severity::Warning,
        weight: 1,
    },
];

/// Empreinte figée du ruleset. À mettre à jour **dans le même changement**
/// que toute modification de [`RULES`], pour que la revue la voie.
pub const RULESET_LOCK: u64 = 0x7f5e_ad48_71c9_5a11;

/// FNV-1a 64 bits sur une sérialisation stable du ruleset.
pub fn fingerprint(rules: &[RuleSpec]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for r in rules {
        let s = format!(
            "{}|{}|{:?}|{:?}|{};",
            r.id, r.version, r.category, r.severity, r.weight
        );
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub rule: &'static str,
    pub path: Path,
    pub message: String,
}

#[derive(Debug, PartialEq)]
pub struct Report {
    pub ruleset: u64,
    pub findings: Vec<Finding>,
    /// Sous-score par catégorie, de 0 à 100.
    pub scores: Vec<(Category, u32)>,
    pub total: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Le ruleset a changé sans mise à jour de l'empreinte figée.
    RulesetTampered { expected: u64, got: u64 },
}

pub struct Context<'a> {
    pub tokens: Option<&'a Tokens>,
    pub input: Input,
}

fn spec(id: &str) -> &'static RuleSpec {
    RULES
        .iter()
        .find(|r| r.id == id)
        .expect("rule is registered")
}

pub fn run(ui: &Ui, ctx: &Context) -> Result<Report, VerifyError> {
    run_with(ui, ctx, RULES, RULESET_LOCK)
}

/// Comme [`run`], avec un ruleset et une empreinte explicites (tests).
pub fn run_with(
    ui: &Ui,
    ctx: &Context,
    rules: &[RuleSpec],
    lock: u64,
) -> Result<Report, VerifyError> {
    let got = fingerprint(rules);
    if got != lock {
        return Err(VerifyError::RulesetTampered {
            expected: lock,
            got,
        });
    }
    let mut f = Vec::new();
    for d in ui.verify() {
        let rule = match d.rule {
            LayoutRule::Overflow => "layout.overflow",
            LayoutRule::OutOfContainer => "layout.out-of-container",
            LayoutRule::Overlap => "layout.overlap",
        };
        let message = match &d.other {
            Some(o) => format!("overlaps {o:?}"),
            None => String::new(),
        };
        f.push(Finding {
            rule,
            path: d.path,
            message,
        });
    }
    let (vw, vh) = ui.viewport();
    for (path, r) in ui.rects() {
        let Some(d) = ui.find(path) else { continue };
        let has_content = d.text.as_ref().is_some_and(|t| !t.is_empty()) || !d.children.is_empty();
        if (r.w <= 0.0 || r.h <= 0.0) && has_content {
            f.push(Finding {
                rule: "layout.hidden-content",
                path: path.clone(),
                message: "content in a zero-size box".into(),
            });
        }
        if r.w > 0.0
            && r.h > 0.0
            && (r.x + r.w <= 0.0 || r.y + r.h <= 0.0 || r.x >= vw || r.y >= vh)
        {
            // Hors fenêtre et pas dans un conteneur défilant : contournement.
            let scrolled =
                (1..path.len()).any(|n| ui.find(&path[..n]).is_some_and(|a| a.style.scroll));
            if !scrolled {
                f.push(Finding {
                    rule: "layout.off-viewport",
                    path: path.clone(),
                    message: String::new(),
                });
            }
        }
        if ctx.input == Input::Touch && d.style.focusable && (r.w < MIN_TOUCH || r.h < MIN_TOUCH) {
            f.push(Finding {
                rule: "a11y.touch-target",
                path: path.clone(),
                message: format!("{}x{}", r.w, r.h),
            });
        }
        if d.text.as_deref() == Some("") {
            f.push(Finding {
                rule: "a11y.empty-text",
                path: path.clone(),
                message: String::new(),
            });
        }
    }
    if let Some(t) = ctx.tokens {
        let root = ui
            .find(&[ui.rects().keys().next().map_or(0, |p| p[0])])
            .cloned();
        if let Some(root) = root {
            for d in audit(&root, t) {
                f.push(Finding {
                    rule: "design.off-charter",
                    path: d.path,
                    message: format!("{} = {}", d.property, f32::from_bits(d.value_bits)),
                });
            }
        }
    }
    f.sort();
    f.dedup();

    let nodes = ui.rects().len().max(1) as u64;
    let mut scores = Vec::new();
    for cat in [Category::Layout, Category::Design, Category::Accessibility] {
        let penalty: u64 = f
            .iter()
            .map(|x| spec(x.rule))
            .filter(|s| s.category == cat)
            .map(|s| s.weight as u64)
            .sum();
        // Pénalité rapportée à la taille de l'arbre, bornée à 0.
        let score = 100u64.saturating_sub(penalty * 100 / nodes);
        scores.push((cat, score as u32));
    }
    let total = scores.iter().map(|s| s.1).min().unwrap_or(100);
    Ok(Report {
        ruleset: got,
        findings: f,
        scores,
        total,
    })
}

/// Gate d'activation : refuse si le candidat ajoute une erreur.
pub fn gate(active: &Report, candidate: &Report) -> Result<(), Vec<Finding>> {
    let new: Vec<Finding> = candidate
        .findings
        .iter()
        .filter(|x| spec(x.rule).severity == Severity::Error && !active.findings.contains(x))
        .cloned()
        .collect();
    if new.is_empty() {
        Ok(())
    } else {
        Err(new)
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn path_str(p: &Path) -> String {
    p.iter()
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn to_json(r: &Report) -> String {
    let findings: Vec<String> = r
        .findings
        .iter()
        .map(|x| {
            format!(
                "{{\"rule\":\"{}\",\"path\":\"{}\",\"message\":\"{}\"}}",
                x.rule,
                path_str(&x.path),
                esc(&x.message)
            )
        })
        .collect();
    let scores: Vec<String> = r
        .scores
        .iter()
        .map(|(c, s)| format!("\"{c:?}\":{s}"))
        .collect();
    format!(
        "{{\"ruleset\":\"{:016x}\",\"total\":{},\"scores\":{{{}}},\"findings\":[{}]}}",
        r.ruleset,
        r.total,
        scores.join(","),
        findings.join(",")
    )
}

/// SARIF 2.1.0 minimal : un run, les règles du ruleset, un résultat par
/// violation, l'emplacement logique étant le chemin du nœud.
pub fn to_sarif(r: &Report) -> String {
    let rules: Vec<String> = RULES
        .iter()
        .map(|s| {
            format!(
                "{{\"id\":\"{}\",\"properties\":{{\"version\":{},\"weight\":{},\"category\":\"{:?}\"}},\"defaultConfiguration\":{{\"level\":\"{}\"}}}}",
                s.id,
                s.version,
                s.weight,
                s.category,
                level(s.severity)
            )
        })
        .collect();
    let results: Vec<String> = r
        .findings
        .iter()
        .map(|x| {
            format!(
                "{{\"ruleId\":\"{}\",\"level\":\"{}\",\"message\":{{\"text\":\"{}\"}},\"locations\":[{{\"logicalLocations\":[{{\"fullyQualifiedName\":\"{}\"}}]}}]}}",
                x.rule,
                level(spec(x.rule).severity),
                esc(if x.message.is_empty() { x.rule } else { &x.message }),
                path_str(&x.path)
            )
        })
        .collect();
    format!(
        "{{\"version\":\"2.1.0\",\"$schema\":\"https://json.schemastore.org/sarif-2.1.0.json\",\"runs\":[{{\"tool\":{{\"driver\":{{\"name\":\"ravum-ui-verify\",\"version\":\"{:016x}\",\"rules\":[{}]}}}},\"results\":[{}]}}]}}",
        r.ruleset,
        rules.join(","),
        results.join(",")
    )
}

fn level(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}
