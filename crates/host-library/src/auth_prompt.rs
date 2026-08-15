//! Authentication prompts, key selection, and hardware confirmation (T105).
//!
//! Sensitive input (passwords / passphrases) is held only transiently: on
//! [`AuthPrompt::submit`] the value is moved out and the model is cleared, so
//! it is never cached; on [`AuthPrompt::cancel`] it is cleared immediately and
//! the authentication is terminated (a later submit is refused). Key
//! selection and hardware confirmation are separate, non-sensitive flows.

/// What kind of authentication interaction this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Password for the account.
    Password,
    /// Private-key passphrase.
    Passphrase,
    /// Choose which key to use.
    KeySelection,
    /// Confirm a hardware (e.g. YubiKey / Windows Hello) touch.
    HardwareConfirmation,
}

/// The prompt state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptState {
    /// Waiting for input.
    Pending,
    /// The value was submitted (and already consumed).
    Submitted,
    /// The user cancelled; authentication is terminated.
    Cancelled,
}

/// Why a prompt operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptError {
    /// The prompt was cancelled; authentication is terminated.
    Cancelled,
    /// The value was already submitted.
    AlreadySubmitted,
    /// No input was entered yet.
    NoInput,
}

/// An authentication prompt with transient sensitive input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPrompt {
    /// Prompt kind.
    pub kind: PromptKind,
    /// Human message shown to the user.
    pub message: String,
    /// Whether the input is a secret (never cached, never in snapshots).
    pub sensitive: bool,
    state: PromptState,
    entered: Option<String>,
}

impl AuthPrompt {
    /// A prompt for `kind` with a message.
    pub fn new(kind: PromptKind, message: impl Into<String>) -> Self {
        let sensitive = matches!(kind, PromptKind::Password | PromptKind::Passphrase);
        Self {
            kind,
            message: message.into(),
            sensitive,
            state: PromptState::Pending,
            entered: None,
        }
    }

    /// The current state.
    pub fn state(&self) -> PromptState {
        self.state
    }

    /// Whether the prompt currently holds a sensitive entry.
    pub fn has_entered_input(&self) -> bool {
        self.entered.is_some()
    }

    /// Whether the model is free of sensitive input right now.
    pub fn is_input_clear(&self) -> bool {
        self.entered.is_none()
    }

    /// Enters a value (only while pending). The value is transient.
    pub fn enter(&mut self, value: &str) -> Result<(), PromptError> {
        if self.state != PromptState::Pending {
            return Err(if self.state == PromptState::Cancelled {
                PromptError::Cancelled
            } else {
                PromptError::AlreadySubmitted
            });
        }
        self.entered = Some(value.to_owned());
        Ok(())
    }

    /// Submits the entered value: it is moved out and the model is cleared
    /// (never cached). Returns the value for the auth layer.
    pub fn submit(&mut self) -> Result<String, PromptError> {
        if self.state == PromptState::Cancelled {
            return Err(PromptError::Cancelled);
        }
        if self.state == PromptState::Submitted {
            return Err(PromptError::AlreadySubmitted);
        }
        let value = self.entered.take().ok_or(PromptError::NoInput)?;
        self.state = PromptState::Submitted;
        Ok(value)
    }

    /// Cancels: clears any sensitive input immediately and terminates the
    /// authentication (a later submit is refused).
    pub fn cancel(&mut self) {
        self.entered = None;
        self.state = PromptState::Cancelled;
    }
}

/// A candidate key for selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOption {
    /// Stable id.
    pub id: u64,
    /// Human label (e.g. the key comment).
    pub label: String,
    /// Whether the key requires hardware confirmation.
    pub hardware: bool,
}

/// The key-selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionState {
    /// Choosing.
    Pending,
    /// A key was selected.
    Selected,
    /// The user cancelled.
    Cancelled,
}

/// Why key selection failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionError {
    /// Unknown key id.
    UnknownKey,
    /// The selection was cancelled.
    Cancelled,
}

/// Key selection flow (non-sensitive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySelection {
    /// Candidate keys.
    pub options: Vec<KeyOption>,
    state: SelectionState,
    selected: Option<u64>,
}

impl KeySelection {
    /// A selection over the given keys.
    pub fn new(options: Vec<KeyOption>) -> Self {
        Self {
            options,
            state: SelectionState::Pending,
            selected: None,
        }
    }

    /// The current state.
    pub fn state(&self) -> SelectionState {
        self.state
    }

    /// Selects a key by id.
    pub fn select(&mut self, id: u64) -> Result<(), SelectionError> {
        if self.state == SelectionState::Cancelled {
            return Err(SelectionError::Cancelled);
        }
        if !self.options.iter().any(|option| option.id == id) {
            return Err(SelectionError::UnknownKey);
        }
        self.selected = Some(id);
        self.state = SelectionState::Selected;
        Ok(())
    }

    /// Confirms the selected key, returning its id.
    pub fn confirm(&mut self) -> Result<u64, SelectionError> {
        if self.state == SelectionState::Cancelled {
            return Err(SelectionError::Cancelled);
        }
        self.selected.ok_or(SelectionError::UnknownKey)
    }

    /// Cancels the selection.
    pub fn cancel(&mut self) {
        self.selected = None;
        self.state = SelectionState::Cancelled;
    }
}

/// The hardware-confirmation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmState {
    /// Waiting for the hardware touch.
    Pending,
    /// The user confirmed.
    Confirmed,
    /// The user cancelled.
    Cancelled,
}

/// A hardware confirmation (e.g. YubiKey / Windows Hello).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareConfirmation {
    /// Human label.
    pub label: String,
    state: ConfirmState,
}

impl HardwareConfirmation {
    /// A confirmation for a labeled hardware operation.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            state: ConfirmState::Pending,
        }
    }

    /// The current state.
    pub fn state(&self) -> ConfirmState {
        self.state
    }

    /// Confirms the hardware touch.
    pub fn confirm(&mut self) {
        self.state = ConfirmState::Confirmed;
    }

    /// Cancels the hardware touch.
    pub fn cancel(&mut self) {
        self.state = ConfirmState::Cancelled;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuthPrompt, ConfirmState, HardwareConfirmation, KeyOption, KeySelection, PromptError,
        PromptKind, PromptState, SelectionError, SelectionState,
    };

    /// The authentication interaction matrix: for every prompt kind, every
    /// action sequence (submit / cancel / cancel-then-submit) must behave.
    #[test]
    fn auth_interaction_matrix() {
        let kinds = [
            (PromptKind::Password, "Password for alpha", true),
            (
                PromptKind::Passphrase,
                "Passphrase for ~/.ssh/id_ed25519",
                true,
            ),
            (PromptKind::KeySelection, "Select a key", false),
            (
                PromptKind::HardwareConfirmation,
                "Touch your security key",
                false,
            ),
        ];
        for (kind, message, sensitive) in kinds {
            // Submit path.
            let mut prompt = AuthPrompt::new(kind, message);
            assert_eq!(prompt.sensitive, sensitive);
            assert_eq!(prompt.state(), PromptState::Pending);
            prompt.enter("secret-value").unwrap();
            assert!(prompt.has_entered_input());
            let value = prompt.submit().expect("submit");
            assert_eq!(value, "secret-value");
            assert_eq!(prompt.state(), PromptState::Submitted);
            assert!(
                prompt.is_input_clear(),
                "sensitive input must not be cached after submit"
            );
            // Submitting twice is refused.
            assert_eq!(prompt.submit(), Err(PromptError::AlreadySubmitted));

            // Cancel path: clears input and terminates immediately.
            let mut prompt = AuthPrompt::new(kind, message);
            prompt.enter("secret-value").unwrap();
            prompt.cancel();
            assert_eq!(prompt.state(), PromptState::Cancelled);
            assert!(
                prompt.is_input_clear(),
                "cancel must clear sensitive input immediately"
            );
            // A later submit is refused (authentication terminated).
            assert_eq!(prompt.submit(), Err(PromptError::Cancelled));
            assert_eq!(prompt.enter("again"), Err(PromptError::Cancelled));
        }
    }

    #[test]
    fn prompt_requires_input_before_submit() {
        let mut prompt = AuthPrompt::new(PromptKind::Password, "p");
        assert_eq!(prompt.submit(), Err(PromptError::NoInput));
    }

    #[test]
    fn key_selection_select_confirm_and_cancel() {
        let options = vec![
            KeyOption {
                id: 1,
                label: "id_ed25519".to_owned(),
                hardware: false,
            },
            KeyOption {
                id: 2,
                label: "yubikey".to_owned(),
                hardware: true,
            },
        ];
        let mut selection = KeySelection::new(options);
        assert_eq!(selection.state(), SelectionState::Pending);
        // Unknown key rejected.
        assert_eq!(selection.select(99), Err(SelectionError::UnknownKey));
        // Select and confirm.
        selection.select(2).unwrap();
        assert_eq!(selection.state(), SelectionState::Selected);
        assert_eq!(selection.confirm().unwrap(), 2);

        // Cancel refuses a later confirm.
        let mut selection = KeySelection::new(vec![KeyOption {
            id: 1,
            label: "id_ed25519".to_owned(),
            hardware: false,
        }]);
        selection.cancel();
        assert_eq!(selection.state(), SelectionState::Cancelled);
        assert_eq!(selection.confirm(), Err(SelectionError::Cancelled));
        assert_eq!(selection.select(1), Err(SelectionError::Cancelled));
    }

    #[test]
    fn hardware_confirmation_confirm_and_cancel() {
        let mut confirmation = HardwareConfirmation::new("Touch to authorize");
        assert_eq!(confirmation.state(), ConfirmState::Pending);
        confirmation.confirm();
        assert_eq!(confirmation.state(), ConfirmState::Confirmed);

        let mut confirmation = HardwareConfirmation::new("Touch to authorize");
        confirmation.cancel();
        assert_eq!(confirmation.state(), ConfirmState::Cancelled);
    }
}
