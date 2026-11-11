//! Shared domain for the M4 reference application.
//!
//! One source of truth: the Rust types below define the fields, the validation
//! rules and the contract served to the browser client at `/api/schema`.
//!
//! What is shared and what is not:
//! * **shared** — field names, types, constraints and the contract version;
//! * **not shared** — storage layout, HTTP details and UI rendering.
//!
//! The client checks `contract_version` before mutating anything. That check is the
//! only compatibility mechanism implemented; there is no code generation, and the
//! JavaScript client is not executed by the test suite.

use serde::{Deserialize, Serialize};

/// Bumped whenever a change would break an existing client.
pub const CONTRACT_VERSION: u32 = 1;

pub const MAX_TITLE_LEN: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub done: bool,
    /// ISO-8601 date (`YYYY-MM-DD`), or absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTask {
    pub title: String,
    #[serde(default)]
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TaskPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    /// Set a due date; ignored when `clear_due_date` is true.
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub clear_due_date: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ValidationError {
    TitleEmpty,
    TitleTooLong { max: usize },
    DueDateInvalid { value: String },
    NothingToChange,
}

impl ValidationError {
    pub fn message(&self) -> String {
        match self {
            ValidationError::TitleEmpty => "the title must not be empty".to_owned(),
            ValidationError::TitleTooLong { max } => {
                format!("the title must be at most {max} characters")
            }
            ValidationError::DueDateInvalid { value } => {
                format!("{value:?} is not a valid YYYY-MM-DD date")
            }
            ValidationError::NothingToChange => "the patch changes nothing".to_owned(),
        }
    }
}

/// Validate a creation request. Returns **all** violations, not just the first.
pub fn validate_new(task: &NewTask) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    validate_title(&task.title, &mut errors);
    if let Some(due) = &task.due_date {
        validate_due_date(due, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate an update request.
pub fn validate_patch(patch: &TaskPatch) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    if let Some(title) = &patch.title {
        validate_title(title, &mut errors);
    }
    if let Some(due) = &patch.due_date {
        validate_due_date(due, &mut errors);
    }
    if patch.title.is_none()
        && patch.done.is_none()
        && patch.due_date.is_none()
        && !patch.clear_due_date
    {
        errors.push(ValidationError::NothingToChange);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_title(title: &str, errors: &mut Vec<ValidationError>) {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        errors.push(ValidationError::TitleEmpty);
    } else if trimmed.chars().count() > MAX_TITLE_LEN {
        errors.push(ValidationError::TitleTooLong { max: MAX_TITLE_LEN });
    }
}

/// Strict `YYYY-MM-DD`. No date arithmetic, no timezone: enough to reject garbage.
fn validate_due_date(value: &str, errors: &mut Vec<ValidationError>) {
    let bytes = value.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !shaped {
        errors.push(ValidationError::DueDateInvalid {
            value: value.to_owned(),
        });
        return;
    }
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        errors.push(ValidationError::DueDateInvalid {
            value: value.to_owned(),
        });
    }
}

/// Is a client that speaks `client_version` compatible with `server_version`?
///
/// Compatibility is exact equality: no forward compatibility is claimed, because a
/// missing field would silently change meaning.
pub fn client_supports(client_version: u32, server_version: u32) -> bool {
    client_version == server_version
}

/// Machine-readable contract served at `/api/schema`.
pub fn contract() -> serde_json::Value {
    serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "resources": {
            "task": {
                "fields": [
                    {"name": "id", "type": "integer", "writable": false},
                    {"name": "title", "type": "string", "max_length": MAX_TITLE_LEN, "required": true},
                    {"name": "done", "type": "boolean", "default": false},
                    {"name": "due_date", "type": "string|null", "format": "YYYY-MM-DD"},
                    {"name": "created_at", "type": "string", "writable": false}
                ]
            }
        },
        "endpoints": [
            {"method": "GET", "path": "/api/tasks", "auth": false},
            {"method": "POST", "path": "/api/tasks", "auth": true},
            {"method": "PATCH", "path": "/api/tasks/{id}", "auth": true},
            {"method": "DELETE", "path": "/api/tasks/{id}", "auth": true}
        ],
        "compatibility": "exact contract_version match; no forward compatibility is claimed"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_task_passes() {
        let task = NewTask {
            title: "acheter du cafe".to_owned(),
            due_date: Some("2026-11-09".to_owned()),
        };
        assert!(validate_new(&task).is_ok());
    }

    #[test]
    fn every_violation_is_reported() {
        let task = NewTask {
            title: "   ".to_owned(),
            due_date: Some("2026-13-40".to_owned()),
        };
        let errors = validate_new(&task).unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(matches!(errors[0], ValidationError::TitleEmpty));
        assert!(matches!(errors[1], ValidationError::DueDateInvalid { .. }));
    }

    #[test]
    fn the_title_limit_is_counted_in_characters() {
        let task = NewTask {
            title: "é".repeat(MAX_TITLE_LEN + 1),
            due_date: None,
        };
        assert!(validate_new(&task).is_err());
        let ok = NewTask {
            title: "é".repeat(MAX_TITLE_LEN),
            due_date: None,
        };
        assert!(validate_new(&ok).is_ok());
    }

    #[test]
    fn malformed_dates_are_rejected() {
        for bad in [
            "2026-1-1",
            "20261109",
            "2026/11/09",
            "2026-00-10",
            "2026-12-32",
        ] {
            let task = NewTask {
                title: "x".to_owned(),
                due_date: Some(bad.to_owned()),
            };
            assert!(validate_new(&task).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn an_empty_patch_is_rejected() {
        assert!(matches!(
            validate_patch(&TaskPatch::default()).unwrap_err()[0],
            ValidationError::NothingToChange
        ));
    }

    #[test]
    fn client_compatibility_is_exact() {
        assert!(client_supports(1, 1));
        assert!(!client_supports(1, 2));
        assert!(!client_supports(2, 1));
    }

    #[test]
    fn the_contract_exposes_the_writable_fields() {
        let contract = contract();
        assert_eq!(contract["contract_version"], CONTRACT_VERSION);
        let fields = contract["resources"]["task"]["fields"]
            .as_array()
            .expect("fields");
        assert!(fields.iter().any(|f| f["name"] == "due_date"));
        assert!(fields.iter().any(|f| f["name"] == "title"));
    }
}
