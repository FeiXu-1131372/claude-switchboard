//! Extract identity fields from the captured `oauthAccount` blob, with
//! fallback to userinfo for OAuth-added accounts whose blob lacks them.

use anyhow::{anyhow, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub email: String,
    pub account_uuid: String,
    pub organization_uuid: Option<String>,
    pub organization_name: Option<String>,
    pub subscription_type: Option<String>,
}

/// Display name for the subscription plan.
///
/// Anthropic renamed the $200 "Max 20x" tier to **Ultra** in October 2026,
/// but Claude Code's OAuth blob can still report `subscriptionType: "max"`
/// for it. The `rateLimitTier` disambiguates the two Max-priced tiers:
/// `default_claude_max_20x` (now Ultra) vs `default_claude_max_5x` (Max).
/// Normalizing here keeps every surface on the same plan name.
pub fn plan_display_name(
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
) -> Option<String> {
    let sub = subscription_type.unwrap_or_default().trim();
    let sub_lower = sub.to_ascii_lowercase();
    let tier = rate_limit_tier.unwrap_or_default().to_ascii_lowercase();
    if sub_lower.contains("ultra")
        || sub_lower.contains("20x")
        || tier.contains("ultra")
        || tier.contains("20x")
    {
        return Some("ultra".to_string());
    }
    if sub_lower.contains("max") {
        return Some("max".to_string());
    }
    if !sub.is_empty() {
        return Some(sub.to_string());
    }
    // Token accounts can carry the rate-limit tier without a subscription
    // label. The tier still names the plan, so it is safe to use here.
    if tier.contains("max") {
        return Some("max".to_string());
    }
    None
}

/// The plan an account is on now, read from the two blobs Claude Code keeps.
///
/// The profile slice (`oauthAccount` in `~/.claude.json`) wins when it names
/// a plan: Claude Code re-fetches it while the account is in use. The
/// credentials blob's `subscriptionType` is written once at sign-in
/// (`subscriptionTypeSource: "sign_in"`), so after an upgrade or downgrade it
/// still names the old plan. An account added through this app's own browser
/// sign-in has no plan in its credentials at all.
pub fn plan_from_blobs(
    claude_code_oauth: Option<&serde_json::Value>,
    oauth_account: Option<&serde_json::Value>,
) -> Option<String> {
    fn text<'a>(blob: Option<&'a serde_json::Value>, key: &str) -> Option<&'a str> {
        blob?.get(key)?.as_str()
    }
    // `planDisplayName` ("Ultra") is the newer spelling; older profiles only
    // carry `organizationType` ("claude_max").
    let profile_plan = text(oauth_account, "planDisplayName")
        .or_else(|| text(oauth_account, "organizationType"))
        .map(|name| {
            let name = name.trim().to_ascii_lowercase();
            name.strip_prefix("claude_").map(str::to_string).unwrap_or(name)
        })
        .filter(|name| !name.is_empty());
    // The profile's tier only counts together with the profile's plan name:
    // on its own, `plan_display_name` would read a tier as a plan.
    profile_plan
        .and_then(|plan| {
            plan_display_name(
                Some(&plan),
                text(oauth_account, "organizationRateLimitTier"),
            )
        })
        .or_else(|| {
            plan_display_name(
                text(claude_code_oauth, "subscriptionType"),
                text(claude_code_oauth, "rateLimitTier"),
            )
        })
}

/// Whether the plan comes with a weekly Fable allowance. Only Max and Ultra
/// do. An unknown plan counts as "no": see
/// `UsageSnapshot::hide_fable_without_access` for why that is safe.
pub fn plan_includes_fable(plan: Option<&str>) -> bool {
    matches!(
        plan.unwrap_or_default().trim().to_ascii_lowercase().as_str(),
        "max" | "ultra"
    )
}

/// Extract identity from the `oauthAccount` slice, plus the credentials blob
/// when known. The plan is read from both (see [`plan_from_blobs`]).
pub fn from_blobs(
    oauth_account: &serde_json::Value,
    claude_code_oauth: Option<&serde_json::Value>,
) -> Result<AccountIdentity> {
    let email = oauth_account
        .get("emailAddress")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing oauthAccount.emailAddress"))?
        .to_string();
    let account_uuid = oauth_account
        .get("accountUuid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing oauthAccount.accountUuid"))?
        .to_string();
    let organization_uuid = oauth_account
        .get("organizationUuid")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let organization_name = oauth_account
        .get("organizationName")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let subscription_type = plan_from_blobs(claude_code_oauth, Some(oauth_account));
    Ok(AccountIdentity {
        email,
        account_uuid,
        organization_uuid,
        organization_name,
        subscription_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_personal_account() {
        let oauth_account = serde_json::json!({
            "accountUuid": "uuid-1",
            "emailAddress": "me@x.com",
            "organizationUuid": null,
            "organizationName": null
        });
        let cc = serde_json::json!({ "subscriptionType": "pro" });
        let id = from_blobs(&oauth_account, Some(&cc)).unwrap();
        assert_eq!(id.email, "me@x.com");
        assert_eq!(id.account_uuid, "uuid-1");
        assert_eq!(id.organization_uuid, None);
        assert_eq!(id.subscription_type.as_deref(), Some("pro"));
    }

    #[test]
    fn extracts_org_account() {
        let oauth_account = serde_json::json!({
            "accountUuid": "uuid-2",
            "emailAddress": "alice@acme.com",
            "organizationUuid": "org-1",
            "organizationName": "Acme"
        });
        let id = from_blobs(&oauth_account, None).unwrap();
        assert_eq!(id.organization_uuid.as_deref(), Some("org-1"));
        assert_eq!(id.organization_name.as_deref(), Some("Acme"));
        assert_eq!(id.subscription_type, None);
    }

    #[test]
    fn missing_required_fields_errors() {
        let bad = serde_json::json!({ "emailAddress": "x@x.com" });
        assert!(from_blobs(&bad, None).is_err());
    }

    #[test]
    fn plan_display_name_maps_the_renamed_max_tiers() {
        assert_eq!(
            plan_display_name(Some("max"), Some("default_claude_max_20x")).as_deref(),
            Some("ultra")
        );
        assert_eq!(
            plan_display_name(Some("max"), Some("default_claude_max_5x")).as_deref(),
            Some("max")
        );
        assert_eq!(
            plan_display_name(Some("ultra"), Some("default_claude_ultra")).as_deref(),
            Some("ultra")
        );
        assert_eq!(
            plan_display_name(Some("pro"), Some("default_claude_ai")).as_deref(),
            Some("pro")
        );
        // A bare "max" with no tier stays Max — never guess the more
        // expensive plan.
        assert_eq!(plan_display_name(Some("max"), None).as_deref(), Some("max"));
        // A token account can have the tier without a subscription label.
        assert_eq!(
            plan_display_name(None, Some("default_claude_max_20x")).as_deref(),
            Some("ultra")
        );
        assert_eq!(
            plan_display_name(None, Some("default_claude_max_5x")).as_deref(),
            Some("max")
        );
        assert_eq!(plan_display_name(None, Some("default_claude_ai")), None);
    }

    /// The live shape of an account that signed in on Pro and was upgraded
    /// later: Claude Code leaves the sign-in plan in the credentials blob
    /// and only refreshes the profile slice.
    fn upgraded_after_sign_in() -> (serde_json::Value, serde_json::Value) {
        let cc = serde_json::json!({
            "subscriptionType": "pro",
            "subscriptionTypeSource": "sign_in",
            "rateLimitTier": "default_claude_ai"
        });
        let oauth_account = serde_json::json!({
            "accountUuid": "uuid-up",
            "emailAddress": "up@x.com",
            "organizationType": "claude_max",
            "organizationRateLimitTier": "default_claude_max_20x"
        });
        (cc, oauth_account)
    }

    #[test]
    fn plan_from_blobs_prefers_the_profile_over_the_sign_in_plan() {
        let (cc, oauth_account) = upgraded_after_sign_in();
        assert_eq!(
            plan_from_blobs(Some(&cc), Some(&oauth_account)).as_deref(),
            Some("ultra")
        );
    }

    #[test]
    fn plan_from_blobs_follows_a_downgrade_too() {
        let cc = serde_json::json!({
            "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_20x"
        });
        let oauth_account = serde_json::json!({
            "organizationType": "claude_pro",
            "organizationRateLimitTier": "default_claude_ai"
        });
        assert_eq!(
            plan_from_blobs(Some(&cc), Some(&oauth_account)).as_deref(),
            Some("pro")
        );
    }

    #[test]
    fn plan_from_blobs_reads_each_profile_spelling() {
        let plan = |oauth_account: serde_json::Value| plan_from_blobs(None, Some(&oauth_account));
        assert_eq!(
            plan(serde_json::json!({ "planDisplayName": "Ultra" })).as_deref(),
            Some("ultra")
        );
        assert_eq!(
            plan(serde_json::json!({
                "organizationType": "claude_max",
                "organizationRateLimitTier": "default_claude_max_5x"
            }))
            .as_deref(),
            Some("max")
        );
        assert_eq!(
            plan(serde_json::json!({ "organizationType": "claude_team" })).as_deref(),
            Some("team")
        );
        // An account added through the browser sign-in has no plan in its
        // credentials at all; the profile is the only place that names it.
        assert_eq!(
            plan(serde_json::json!({
                "organizationType": "claude_pro",
                "organizationRateLimitTier": "default_claude_ai"
            }))
            .as_deref(),
            Some("pro")
        );
    }

    #[test]
    fn plan_from_blobs_falls_back_to_the_credentials() {
        let cc = serde_json::json!({
            "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_5x"
        });
        // A profile slice without plan fields (what this app writes for a
        // fresh browser sign-in) must not hide what the credentials know.
        let bare = serde_json::json!({ "accountUuid": "u", "emailAddress": "a@x.com" });
        assert_eq!(plan_from_blobs(Some(&cc), Some(&bare)).as_deref(), Some("max"));
        assert_eq!(
            plan_from_blobs(Some(&cc), Some(&serde_json::Value::Null)).as_deref(),
            Some("max")
        );
        assert_eq!(plan_from_blobs(Some(&cc), None).as_deref(), Some("max"));
        assert_eq!(plan_from_blobs(None, None), None);
    }

    #[test]
    fn from_blobs_uses_the_profile_plan() {
        let (cc, oauth_account) = upgraded_after_sign_in();
        let id = from_blobs(&oauth_account, Some(&cc)).unwrap();
        assert_eq!(id.subscription_type.as_deref(), Some("ultra"));
    }

    #[test]
    fn only_max_and_ultra_include_fable() {
        assert!(plan_includes_fable(Some("max")));
        assert!(plan_includes_fable(Some("ultra")));
        assert!(plan_includes_fable(Some(" Ultra ")));
        assert!(!plan_includes_fable(Some("pro")));
        assert!(!plan_includes_fable(Some("team")));
        assert!(!plan_includes_fable(None));
    }

    #[test]
    fn from_blobs_reads_rate_limit_tier_for_ultra() {
        let oauth_account = serde_json::json!({
            "accountUuid": "uuid-3",
            "emailAddress": "ultra@x.com"
        });
        let cc = serde_json::json!({
            "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_20x"
        });
        let id = from_blobs(&oauth_account, Some(&cc)).unwrap();
        assert_eq!(id.subscription_type.as_deref(), Some("ultra"));
    }
}
