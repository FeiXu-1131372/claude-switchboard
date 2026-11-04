//! Which account is signed in where the bridge runs (spec §3.1).

use crate::config_paths::EnvView;
use crate::format::Identity;
use serde::Deserialize;
use std::io;
use std::path::Path;
use std::time::Duration;

/// A resolved identity, or the fact that the config could not be read.
/// `ReadError` is deliberately not an identity: Claude Code rewrites its
/// global config constantly, so a torn read says nothing about who is
/// signed in and must never reset or confirm anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Identity(Identity),
    ReadError,
}

/// The only slice of the (often megabyte-sized) global config the bridge
/// reads; serde skips the rest.
#[derive(Deserialize)]
struct GlobalConfig {
    #[serde(rename = "oauthAccount", default)]
    oauth_account: Option<OauthAccount>,
}

#[derive(Deserialize)]
struct OauthAccount {
    #[serde(rename = "accountUuid", default)]
    account_uuid: Option<String>,
    #[serde(rename = "emailAddress", default)]
    email_address: Option<String>,
    #[serde(rename = "organizationUuid", default)]
    organization_uuid: Option<String>,
}

pub fn resolve(env: &EnvView, global_config: &Path) -> Resolution {
    resolve_with(env, global_config, &|p| std::fs::read_to_string(p))
}

/// [`resolve`] with an injectable reader, so the retry is testable.
pub fn resolve_with(
    env: &EnvView,
    global_config: &Path,
    read: &dyn Fn(&Path) -> io::Result<String>,
) -> Resolution {
    // Auth that bypasses the config dir's login is never attributed, whatever
    // `oauthAccount` says.
    if env.is_routed() {
        return Resolution::Identity(Identity::None {
            reason: Some("routed".into()),
        });
    }
    // One retry after 20 ms: a torn read while Claude Code rewrites the file.
    for attempt in 0..2 {
        if attempt == 1 {
            std::thread::sleep(Duration::from_millis(20));
        }
        let parsed = read(global_config)
            .ok()
            .and_then(|text| serde_json::from_str::<GlobalConfig>(&text).ok());
        if let Some(config) = parsed {
            return Resolution::Identity(match config.oauth_account {
                // A parsed `oauthAccount: null` (or none at all): a disk-stored
                // long-lived token. No identity, with no reason.
                None => Identity::None { reason: None },
                Some(account) => match account.account_uuid.filter(|u| !u.is_empty()) {
                    Some(account_uuid) => Identity::Oauth {
                        account_uuid,
                        email: account.email_address.unwrap_or_default(),
                        organization_uuid: account.organization_uuid.filter(|o| !o.is_empty()),
                    },
                    None => Identity::None {
                        reason: Some("incomplete_identity".into()),
                    },
                },
            });
        }
    }
    Resolution::ReadError
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn env() -> EnvView {
        EnvView {
            home: PathBuf::from("/h"),
            ..Default::default()
        }
    }

    fn write(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let p = dir.path().join(".claude.json");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn oauth_account_resolves_with_organization() {
        let d = tempdir().unwrap();
        let p = write(
            &d,
            r#"{"numStartups":3,"oauthAccount":{"accountUuid":"u-1","emailAddress":"a@b.c","organizationUuid":"o-1","extra":{"x":1}}}"#,
        );
        assert_eq!(
            resolve(&env(), &p),
            Resolution::Identity(Identity::Oauth {
                account_uuid: "u-1".into(),
                email: "a@b.c".into(),
                organization_uuid: Some("o-1".into()),
            })
        );
    }

    #[test]
    fn null_or_absent_oauth_account_resolves_to_none_without_reason() {
        let d = tempdir().unwrap();
        let expected = Resolution::Identity(Identity::None { reason: None });
        let p = write(&d, r#"{"oauthAccount":null}"#);
        assert_eq!(resolve(&env(), &p), expected);
        let p = write(&d, r#"{"numStartups":1}"#);
        assert_eq!(resolve(&env(), &p), expected);
    }

    #[test]
    fn an_oauth_account_without_a_uuid_never_attributes() {
        let d = tempdir().unwrap();
        let p = write(&d, r#"{"oauthAccount":{"emailAddress":"a@b.c"}}"#);
        assert_eq!(
            resolve(&env(), &p),
            Resolution::Identity(Identity::None {
                reason: Some("incomplete_identity".into())
            })
        );
    }

    #[test]
    fn truncated_global_config_is_a_read_error_not_an_identity() {
        let d = tempdir().unwrap();
        let p = write(&d, r#"{"oauthAccount":{"accountUuid":"u-"#);
        assert_eq!(resolve(&env(), &p), Resolution::ReadError);
    }

    #[test]
    fn a_missing_global_config_is_a_read_error() {
        assert_eq!(
            resolve(&env(), Path::new("/definitely/not/here/.claude.json")),
            Resolution::ReadError
        );
    }

    #[test]
    fn a_torn_read_is_retried_once() {
        let calls = Cell::new(0);
        let read = |_: &Path| -> io::Result<String> {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Ok(r#"{"oauthAccount":{"accountUu"#.to_string())
            } else {
                Ok(r#"{"oauthAccount":{"accountUuid":"u-9","emailAddress":"z@z.z"}}"#.to_string())
            }
        };
        let got = resolve_with(&env(), Path::new("/x"), &read);
        assert_eq!(calls.get(), 2);
        assert!(matches!(got, Resolution::Identity(Identity::Oauth { .. })));
    }

    #[test]
    fn non_default_base_url_resolves_to_none_routed() {
        let d = tempdir().unwrap();
        let p = write(&d, r#"{"oauthAccount":{"accountUuid":"u-1","emailAddress":"a@b.c"}}"#);
        let mut e = env();
        e.anthropic_base_url = Some("https://proxy.example.com".into());
        assert_eq!(
            resolve(&e, &p),
            Resolution::Identity(Identity::None {
                reason: Some("routed".into())
            })
        );
    }

    #[test]
    fn each_provider_flag_resolves_to_none_routed() {
        let d = tempdir().unwrap();
        let p = write(&d, r#"{"oauthAccount":{"accountUuid":"u-1"}}"#);
        for i in 0..3 {
            let mut e = env();
            match i {
                0 => e.use_bedrock = true,
                1 => e.use_vertex = true,
                _ => e.use_foundry = true,
            }
            assert_eq!(
                resolve(&e, &p),
                Resolution::Identity(Identity::None {
                    reason: Some("routed".into())
                })
            );
        }
    }

    #[test]
    #[ignore = "benchmark: run explicitly"]
    fn a_2mb_config_parses_under_10ms() {
        let d = tempdir().unwrap();
        let filler = "x".repeat(2 * 1024 * 1024);
        let p = write(
            &d,
            &format!(r#"{{"history":"{filler}","oauthAccount":{{"accountUuid":"u-1"}}}}"#),
        );
        let start = std::time::Instant::now();
        let got = resolve(&env(), &p);
        assert!(matches!(got, Resolution::Identity(Identity::Oauth { .. })));
        assert!(start.elapsed() < Duration::from_millis(10), "took {:?}", start.elapsed());
    }
}
