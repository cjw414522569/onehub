use secret::SecretString;

/// SSH authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthMethod {
    /// Password.
    Password,
    /// keyboard-interactive.
    KeyboardInteractive,
    /// Public key.
    PublicKey,
    /// SSH agent.
    Agent,
    /// None (no authentication).
    None,
}

/// A single keyboard-interactive prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPrompt {
    /// Prompt name (e.g. `Password:`).
    pub name: String,
    /// Optional instruction.
    pub instruction: Option<String>,
    /// The prompt text.
    pub prompt: String,
    /// Whether the response is echoed (displayed) by the server.
    ///
    /// When `false` the input is sensitive and the UI must mask it.
    pub echo: bool,
}

impl AuthPrompt {
    /// Whether the UI must mask the input (sensitive display policy).
    pub fn should_mask_input(&self) -> bool {
        !self.echo
    }
}

/// A keyboard-interactive challenge (one or more prompts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthChallenge {
    /// Optional challenge name.
    pub name: Option<String>,
    /// Optional challenge instruction.
    pub instruction: Option<String>,
    /// Prompts.
    pub prompts: Vec<AuthPrompt>,
}

/// Responses to a challenge, each wrapped in a zeroizing secret.
pub struct AuthResponse {
    /// One response per prompt.
    pub responses: Vec<SecretString>,
}

impl AuthResponse {
    /// Builds a response from an iterator of string responses.
    pub fn from_responses(responses: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            responses: responses
                .into_iter()
                .map(|value| SecretString::from(value.into()))
                .collect(),
        }
    }
}

/// Result of a successful authentication flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthOutcome {
    /// Whether authentication succeeded.
    pub success: bool,
    /// The method that was used.
    pub method: AuthMethod,
}

/// Why an authentication flow failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    /// The user cancelled a prompt.
    Cancelled,
    /// The server rejected the credentials.
    Rejected,
    /// The flow failed without a server verdict.
    Error,
}

/// Handles keyboard-interactive challenges.
///
/// Returning `None` cancels the authentication flow.
pub trait KeyboardInteractiveHandler: Send + Sync {
    /// Handles one challenge; returns the responses or `None` to cancel.
    fn handle_challenge(&self, challenge: &AuthChallenge) -> Option<AuthResponse>;
}

/// Drives a keyboard-interactive exchange over a scripted sequence of
/// challenges (as a simulated server would present them).
pub fn run_keyboard_interactive(
    handler: &dyn KeyboardInteractiveHandler,
    challenges: &[AuthChallenge],
) -> Result<AuthOutcome, AuthFailure> {
    for challenge in challenges {
        let Some(response) = handler.handle_challenge(challenge) else {
            return Err(AuthFailure::Cancelled);
        };
        if response.responses.len() != challenge.prompts.len() {
            return Err(AuthFailure::Rejected);
        }
    }
    Ok(AuthOutcome {
        success: true,
        method: AuthMethod::KeyboardInteractive,
    })
}

/// Constant-time comparison helper.
fn ct_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Password authentication against a stored expected secret.
pub struct PasswordAuthenticator {
    /// The expected password (zeroizing).
    expected: SecretString,
}

impl PasswordAuthenticator {
    /// Creates an authenticator for the expected password.
    pub fn new(expected: SecretString) -> Self {
        Self { expected }
    }

    /// Attempts a password; constant-time comparison against the expected
    /// value.
    pub fn attempt(&self, supplied: &SecretString) -> bool {
        ct_eq(
            supplied.expose_secret().as_bytes(),
            self.expected.expose_secret().as_bytes(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        run_keyboard_interactive, AuthChallenge, AuthFailure, AuthMethod, AuthOutcome, AuthPrompt,
        AuthResponse, KeyboardInteractiveHandler, PasswordAuthenticator,
    };
    use secret::SecretString;

    fn password_prompt() -> AuthPrompt {
        AuthPrompt {
            name: "Password:".to_owned(),
            instruction: None,
            prompt: "user@host password:".to_owned(),
            echo: false,
        }
    }

    fn username_prompt() -> AuthPrompt {
        AuthPrompt {
            name: "Username:".to_owned(),
            instruction: None,
            prompt: "user@host login:".to_owned(),
            echo: true,
        }
    }

    #[test]
    fn sensitive_display_policy_masks_non_echo_prompts() {
        assert!(password_prompt().should_mask_input());
        assert!(!username_prompt().should_mask_input());
    }

    #[test]
    fn password_authenticator_matches_and_rejects() {
        let auth = PasswordAuthenticator::new(SecretString::from("hunter2"));
        assert!(auth.attempt(&SecretString::from("hunter2")));
        assert!(!auth.attempt(&SecretString::from("hunter3")));
        assert!(!auth.attempt(&SecretString::from("hunter2-longer")));
    }

    #[test]
    fn multi_round_keyboard_interactive_succeeds() {
        struct Scripted;
        impl KeyboardInteractiveHandler for Scripted {
            fn handle_challenge(&self, challenge: &AuthChallenge) -> Option<AuthResponse> {
                // Two prompts: username (echoed) and password (masked).
                assert_eq!(challenge.prompts.len(), 2);
                assert!(challenge.prompts[1].should_mask_input());
                Some(AuthResponse::from_responses(["alice", "hunter2"]))
            }
        }
        let challenge = AuthChallenge {
            name: Some("PAM".to_owned()),
            instruction: Some("Enter credentials".to_owned()),
            prompts: vec![username_prompt(), password_prompt()],
        };
        let result = run_keyboard_interactive(&Scripted, &[challenge]);
        assert_eq!(
            result,
            Ok(AuthOutcome {
                success: true,
                method: AuthMethod::KeyboardInteractive
            })
        );
    }

    #[test]
    fn cancellation_aborts_the_flow() {
        struct Cancelling;
        impl KeyboardInteractiveHandler for Cancelling {
            fn handle_challenge(&self, _challenge: &AuthChallenge) -> Option<AuthResponse> {
                None
            }
        }
        let challenge = AuthChallenge {
            name: None,
            instruction: None,
            prompts: vec![password_prompt()],
        };
        assert_eq!(
            run_keyboard_interactive(&Cancelling, &[challenge]),
            Err(AuthFailure::Cancelled)
        );
    }

    #[test]
    fn wrong_response_count_is_rejected() {
        struct WrongCount;
        impl KeyboardInteractiveHandler for WrongCount {
            fn handle_challenge(&self, _challenge: &AuthChallenge) -> Option<AuthResponse> {
                Some(AuthResponse::from_responses(["only-one"]))
            }
        }
        let challenge = AuthChallenge {
            name: None,
            instruction: None,
            prompts: vec![username_prompt(), password_prompt()],
        };
        assert_eq!(
            run_keyboard_interactive(&WrongCount, &[challenge]),
            Err(AuthFailure::Rejected)
        );
    }

    #[test]
    fn multiple_rounds_are_supported() {
        struct TwoRounds;
        impl KeyboardInteractiveHandler for TwoRounds {
            fn handle_challenge(&self, challenge: &AuthChallenge) -> Option<AuthResponse> {
                match challenge.prompts[0].name.as_str() {
                    "Password:" => Some(AuthResponse::from_responses(["first"])),
                    "Token:" => Some(AuthResponse::from_responses(["second"])),
                    _ => None,
                }
            }
        }
        let challenges = vec![
            AuthChallenge {
                name: None,
                instruction: None,
                prompts: vec![password_prompt()],
            },
            AuthChallenge {
                name: None,
                instruction: None,
                prompts: vec![AuthPrompt {
                    name: "Token:".to_owned(),
                    instruction: None,
                    prompt: "2FA token:".to_owned(),
                    echo: false,
                }],
            },
        ];
        assert_eq!(
            run_keyboard_interactive(&TwoRounds, &challenges),
            Ok(AuthOutcome {
                success: true,
                method: AuthMethod::KeyboardInteractive
            })
        );
    }

    #[test]
    fn responses_are_zeroizing_secrets() {
        let response = AuthResponse::from_responses(["hunter2"]);
        // SecretString has no Debug; expose for verification only.
        assert_eq!(response.responses[0].expose_secret(), "hunter2");
    }
}
