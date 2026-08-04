//! Gmail label to Lotus folder mapping.
//!
//! Archive is the one that trips people up: it is not a Gmail label. A message
//! is archived when it has no `INBOX` label. So Archive is a local-only folder
//! with no `provider_folder_id`, and membership is derived rather than read.

use crate::model::{folder, Folder};
use crate::provider::gmail::api::Label;

/// Gmail system labels that become Lotus folders, and the role each maps to.
const SYSTEM_ROLES: &[(&str, &str, &str)] = &[
    ("INBOX", "Inbox", "inbox"),
    ("STARRED", "Starred", "starred"),
    ("DRAFT", "Drafts", "drafts"),
    ("SENT", "Sent", "sent"),
    ("TRASH", "Trash", "trash"),
    ("SPAM", "Spam", "spam"),
];

pub fn role_for_label(label_id: &str) -> Option<&'static str> {
    SYSTEM_ROLES
        .iter()
        .find(|(id, _, _)| *id == label_id)
        .map(|(_, _, role)| *role)
}

/// Build the folder list for an account. System labels in a fixed order, then
/// the local-only Archive view, then user labels alphabetically.
pub fn folders_for_account(account_id: &str, labels: &[Label]) -> Vec<Folder> {
    let mut folders: Vec<Folder> = Vec::new();

    for (label_id, name, role) in SYSTEM_ROLES {
        if labels.iter().any(|label| label.id == *label_id) {
            let mut mapped = folder(&format!("{account_id}-{role}"), account_id, name, role);
            mapped.provider_folder_id = Some((*label_id).to_string());
            folders.push(mapped);
        }
    }

    // Archive is the absence of INBOX, so it carries no provider label id.
    folders.push(folder(
        &format!("{account_id}-archive"),
        account_id,
        "Archive",
        "archive",
    ));

    let mut user_labels: Vec<&Label> = labels
        .iter()
        .filter(|label| {
            label.label_type.as_deref() == Some("user")
                && role_for_label(&label.id).is_none()
                && !label.name.starts_with("CATEGORY_")
        })
        .collect();
    user_labels.sort_by(|left, right| left.name.cmp(&right.name));

    for label in user_labels {
        let mut mapped = folder(
            &format!("{account_id}-label-{}", slug(&label.id)),
            account_id,
            &label.name,
            "label",
        );
        mapped.provider_folder_id = Some(label.id.clone());
        folders.push(mapped);
    }

    folders
}

/// Which Lotus folders a message belongs to, given its Gmail label ids.
/// A message with no `INBOX` label lands in Archive.
pub fn folder_ids_for_message(
    account_id: &str,
    folders: &[Folder],
    label_ids: &[String],
) -> Vec<String> {
    let mut ids: Vec<String> = folders
        .iter()
        .filter(|folder| folder.account_id == account_id)
        .filter(|folder| {
            folder
                .provider_folder_id
                .as_ref()
                .map(|provider_id| label_ids.iter().any(|label| label == provider_id))
                .unwrap_or(false)
        })
        .map(|folder| folder.id.clone())
        .collect();

    let in_inbox = label_ids.iter().any(|label| label == "INBOX");
    let in_trash_or_spam = label_ids
        .iter()
        .any(|label| label == "TRASH" || label == "SPAM");

    if !in_inbox && !in_trash_or_spam {
        if let Some(archive) = folders
            .iter()
            .find(|folder| folder.account_id == account_id && folder.role == "archive")
        {
            ids.push(archive.id.clone());
        }
    }

    ids
}

/// Display labels for the chips in the UI. Gmail's `CATEGORY_*` and internal
/// flags are noise there, and `UNREAD`/`STARRED` are already rendered as state.
pub fn display_labels(labels: &[Label], label_ids: &[String]) -> Vec<String> {
    let mut names: Vec<String> = label_ids
        .iter()
        .filter(|id| {
            !matches!(
                id.as_str(),
                "UNREAD" | "STARRED" | "INBOX" | "SENT" | "DRAFT" | "TRASH" | "SPAM" | "IMPORTANT"
            )
        })
        .map(|id| {
            labels
                .iter()
                .find(|label| &label.id == id)
                .map(|label| label.name.clone())
                .unwrap_or_else(|| pretty_category(id))
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// `CATEGORY_PROMOTIONS` reads as "Promotions" in the chip.
fn pretty_category(label_id: &str) -> String {
    let base = label_id.strip_prefix("CATEGORY_").unwrap_or(label_id);
    let lower = base.to_lowercase();
    let mut characters = lower.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn slug(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(id: &str, name: &str, kind: &str) -> Label {
        Label {
            id: id.into(),
            name: name.into(),
            label_type: Some(kind.into()),
        }
    }

    fn sample_labels() -> Vec<Label> {
        vec![
            label("INBOX", "INBOX", "system"),
            label("STARRED", "STARRED", "system"),
            label("SENT", "SENT", "system"),
            label("DRAFT", "DRAFT", "system"),
            label("TRASH", "TRASH", "system"),
            label("SPAM", "SPAM", "system"),
            label("Label_12", "Receipts", "user"),
            label("Label_9", "Projects", "user"),
        ]
    }

    #[test]
    fn system_labels_map_to_roles_and_archive_is_local_only() {
        let folders = folders_for_account("acct", &sample_labels());

        let inbox = folders.iter().find(|f| f.role == "inbox").unwrap();
        assert_eq!(inbox.provider_folder_id.as_deref(), Some("INBOX"));

        let archive = folders.iter().find(|f| f.role == "archive").unwrap();
        assert_eq!(archive.provider_folder_id, None);
    }

    #[test]
    fn user_labels_become_folders_in_alphabetical_order() {
        let folders = folders_for_account("acct", &sample_labels());
        let names: Vec<&str> = folders
            .iter()
            .filter(|f| f.role == "label")
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(names, vec!["Projects", "Receipts"]);
    }

    #[test]
    fn a_message_in_inbox_and_a_user_label_belongs_to_both() {
        let folders = folders_for_account("acct", &sample_labels());
        let ids = folder_ids_for_message(
            "acct",
            &folders,
            &["INBOX".into(), "Label_12".into(), "UNREAD".into()],
        );

        assert!(ids.contains(&"acct-inbox".to_string()));
        assert!(ids.iter().any(|id| id.starts_with("acct-label-label-12")));
        assert!(!ids.contains(&"acct-archive".to_string()));
    }

    #[test]
    fn absence_of_inbox_means_archive() {
        let folders = folders_for_account("acct", &sample_labels());
        let ids = folder_ids_for_message("acct", &folders, &["Label_12".into()]);
        assert!(ids.contains(&"acct-archive".to_string()));
    }

    #[test]
    fn trash_and_spam_are_not_archive() {
        let folders = folders_for_account("acct", &sample_labels());
        for label_id in ["TRASH", "SPAM"] {
            let ids = folder_ids_for_message("acct", &folders, &[label_id.into()]);
            assert!(!ids.contains(&"acct-archive".to_string()));
        }
    }

    #[test]
    fn display_labels_drop_state_flags_and_prettify_categories() {
        let names = display_labels(
            &sample_labels(),
            &[
                "INBOX".into(),
                "UNREAD".into(),
                "STARRED".into(),
                "IMPORTANT".into(),
                "CATEGORY_PROMOTIONS".into(),
                "Label_12".into(),
            ],
        );
        assert_eq!(names, vec!["Promotions", "Receipts"]);
    }
}
