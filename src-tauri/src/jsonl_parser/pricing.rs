use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct PricingEntry {
    pub prefix: String,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_5m_per_mtok: f64,
    pub cache_1h_per_mtok: f64,
    /// Optional 1M-context tier (Sonnet 4 only at time of writing). When
    /// the per-call input-side context exceeds `above_tokens`, every rate
    /// in this block replaces the base rate for that call.
    #[serde(default)]
    pub tier: Option<PricingTier>,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct PricingTier {
    pub above_tokens: u64,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_5m_per_mtok: f64,
    pub cache_1h_per_mtok: f64,
}

#[derive(Debug, Deserialize)]
struct PricingFile {
    pricing: Vec<PricingEntry>,
}

pub struct PricingTable {
    entries: Vec<PricingEntry>,
}

/// Revision of the bundled pricing table. Bump this whenever pricing.json
/// changes so the startup migration (Db::reprice_outdated_events) recomputes
/// historical event costs exactly once per correction.
pub const PRICING_VERSION: u32 = 2;

impl PricingTable {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).context("read pricing.json")?;
        Self::parse(&raw)
    }

    pub fn parse(raw: &str) -> Result<Self> {
        let f: PricingFile = serde_json::from_str(raw)?;
        let mut entries = f.pricing;
        entries.sort_by_key(|e| std::cmp::Reverse(e.prefix.len()));
        Ok(Self { entries })
    }

    pub fn bundled() -> Result<Self> {
        let raw = include_str!("../../pricing.json");
        Self::parse(raw)
    }

    pub fn entries(&self) -> &[PricingEntry] {
        &self.entries
    }

    pub fn lookup(&self, model: &str) -> Option<&PricingEntry> {
        let lower = model.to_ascii_lowercase();
        // Strip the "claude-" vendor prefix so that both full API model IDs
        // ("claude-sonnet-4-6-20260115") and bare family names ("sonnet-4-6")
        // resolve correctly via starts_with on the pricing prefix.
        let needle = lower.strip_prefix("claude-").unwrap_or(&lower);
        self.entries.iter().find(|e| needle.starts_with(e.prefix.as_str()))
    }

    /// Estimated dollars saved per million cache-read tokens for `model` —
    /// the gap between paying full input price and the cache-read price for
    /// the same tokens. `None` for unpriced models (no estimate possible).
    /// Uses base (non-tier) rates: a reasonable approximation for a savings
    /// estimate, since tier eligibility varies per call.
    pub fn cache_savings_per_mtok(&self, model: &str) -> Option<f64> {
        self.lookup(model)
            .map(|e| (e.input_per_mtok - e.cache_read_per_mtok).max(0.0))
    }

    pub fn cost_for(
        &self,
        model: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_5m: u64,
        cache_1h: u64,
    ) -> f64 {
        let Some(e) = self.lookup(model) else {
            return 0.0;
        };
        let m = 1_000_000.0;

        // For 1M-context models, Anthropic charges every input-side and
        // output token at the higher tier rate when the prompt's context
        // size exceeds the threshold (it's not a per-token split — the
        // whole call shifts up). Total context = input + cache_read +
        // cache_creation; that's what Claude's tokenizer counted.
        let context_size = input + cache_read + cache_5m + cache_1h;
        let (input_rate, output_rate, cr_rate, c5m_rate, c1h_rate) = match &e.tier {
            Some(t) if context_size > t.above_tokens => (
                t.input_per_mtok,
                t.output_per_mtok,
                t.cache_read_per_mtok,
                t.cache_5m_per_mtok,
                t.cache_1h_per_mtok,
            ),
            _ => (
                e.input_per_mtok,
                e.output_per_mtok,
                e.cache_read_per_mtok,
                e.cache_5m_per_mtok,
                e.cache_1h_per_mtok,
            ),
        };

        (input as f64) / m * input_rate
            + (output as f64) / m * output_rate
            + (cache_read as f64) / m * cr_rate
            + (cache_5m as f64) / m * c5m_rate
            + (cache_1h as f64) / m * c1h_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> PricingTable {
        PricingTable::bundled().unwrap()
    }

    #[test]
    fn longest_prefix_wins() {
        let tbl = t();
        let opus47 = tbl.lookup("claude-opus-4-7-20260115").unwrap();
        assert_eq!(opus47.prefix, "opus-4-7");
        let opus_generic = tbl.lookup("opus-7-5-future").unwrap();
        assert_eq!(opus_generic.prefix, "opus");
    }

    #[test]
    fn every_current_family_is_priced() {
        let tbl = t();
        for m in [
            "fable-5",
            "mythos-5",
            "opus-4-8",
            "opus-4-7",
            "opus-4-6",
            "opus-4-5",
            "opus-4-1",
            "opus-4",
            "claude-3-opus-20240229",
            "sonnet-5",
            "sonnet-4-6",
            "sonnet-4-5",
            "sonnet-4",
            "haiku-4-5",
            "haiku-3-5",
        ] {
            assert!(tbl.lookup(m).is_some(), "missing pricing for {m}");
        }
    }

    #[test]
    fn fable_5_is_priced_at_current_rates() {
        let tbl = t();
        // $10 in / $50 out, cache: $1 read / $12.50 5m / $20 1h (per MTok).
        let e = tbl.lookup("claude-fable-5").unwrap();
        assert_eq!(e.prefix, "fable-5");
        assert!((e.input_per_mtok - 10.0).abs() < 0.001);
        assert!((e.output_per_mtok - 50.0).abs() < 0.001);
        assert!((e.cache_read_per_mtok - 1.0).abs() < 0.001);
        assert!((e.cache_5m_per_mtok - 12.5).abs() < 0.001);
        assert!((e.cache_1h_per_mtok - 20.0).abs() < 0.001);
        assert!(e.tier.is_none(), "Fable 5 1M context is flat-rate");
    }

    #[test]
    fn opus_4_8_uses_current_opus_tier_not_legacy_opus_4() {
        let tbl = t();
        // Opus 4.8 is $5/$25 — it must NOT fall through to the legacy
        // $15/$75 opus-4/opus-4-1 rates via the "opus-4" prefix.
        let e = tbl.lookup("claude-opus-4-8").unwrap();
        assert_eq!(e.prefix, "opus-4-8");
        assert!((e.input_per_mtok - 5.0).abs() < 0.001);
        assert!((e.output_per_mtok - 25.0).abs() < 0.001);
    }

    #[test]
    fn unknown_future_opus_defaults_to_current_opus_tier() {
        let tbl = t();
        // The bare "opus" fallback should guess the CURRENT Opus tier
        // ($5/$25), not the retired Opus 4 / 4.1 rates.
        let c = tbl.cost_for("opus-9-9-future", 1_000_000, 1_000_000, 0, 0, 0);
        assert!((c - 30.0).abs() < 0.001, "got {c}");
    }

    #[test]
    fn opus_3_keeps_legacy_rates() {
        let tbl = t();
        // Retired 3.x IDs put the generation first ("claude-3-opus-…"), so
        // the functional prefix is "3-opus", not "opus-3".
        let e = tbl.lookup("claude-3-opus-20240229").unwrap();
        assert_eq!(e.prefix, "3-opus");
        assert!((e.input_per_mtok - 15.0).abs() < 0.001);
        assert!((e.output_per_mtok - 75.0).abs() < 0.001);
    }

    #[test]
    fn sonnet_5_is_flat_rate_with_no_long_context_tier() {
        let tbl = t();
        let e = tbl.lookup("claude-sonnet-5").unwrap();
        assert_eq!(e.prefix, "sonnet-5");
        assert!((e.input_per_mtok - 3.0).abs() < 0.001);
        assert!((e.output_per_mtok - 15.0).abs() < 0.001);
        assert!(e.tier.is_none(), "Sonnet 5 1M context is flat-rate");
        // Above 200k the flat rate still applies.
        let c = tbl.cost_for("claude-sonnet-5", 250_000, 0, 0, 0, 0);
        assert!((c - 0.75).abs() < 0.001, "got {c}");
    }

    #[test]
    fn sonnet_4_6_no_longer_has_a_long_context_tier() {
        let tbl = t();
        // Anthropic folded Sonnet 4.6's 1M context into standard pricing —
        // the >200k premium tier no longer exists for this model.
        assert!(tbl.lookup("sonnet-4-6").unwrap().tier.is_none());
        let c = tbl.cost_for("sonnet-4-6", 250_000, 0, 0, 0, 0);
        assert!((c - 0.75).abs() < 0.001, "got {c}");
    }

    #[test]
    fn third_party_relay_models_are_priced_at_vendor_rates() {
        let tbl = t();
        // MiniMax M2.7 — official platform.minimax.io rates.
        let std = tbl.lookup("MiniMax-M2.7").unwrap();
        assert!((std.input_per_mtok - 0.30).abs() < 0.001);
        assert!((std.output_per_mtok - 1.20).abs() < 0.001);
        // Zhipu GLM-5.1 — official Z.ai rates.
        let glm = tbl.lookup("glm-5.1").unwrap();
        assert!((glm.input_per_mtok - 1.40).abs() < 0.001);
        assert!((glm.output_per_mtok - 4.40).abs() < 0.001);
        // Moonshot Kimi K3 — official rates; also match the kimi-k3 form.
        for id in ["k3", "kimi-k3"] {
            let k3 = tbl.lookup(id).unwrap();
            assert!((k3.input_per_mtok - 3.0).abs() < 0.001, "{id} input");
            assert!((k3.output_per_mtok - 15.0).abs() < 0.001, "{id} output");
        }
    }

    #[test]
    fn minimax_highspeed_matches_its_own_prefix_at_2x_rates() {
        let tbl = t();
        // The highspeed variant must win longest-prefix matching and price
        // at 2× the standard M2.7 rate.
        let hs = tbl.lookup("minimax-m2.7-highspeed").unwrap();
        assert_eq!(hs.prefix, "minimax-m2.7-highspeed");
        assert!((hs.input_per_mtok - 0.60).abs() < 0.001);
        assert!((hs.output_per_mtok - 2.40).abs() < 0.001);
        assert_eq!(tbl.lookup("minimax-m2.7").unwrap().prefix, "minimax-m2.7");
    }

    #[test]
    fn cache_savings_rate_is_input_minus_cache_read_per_model() {
        let tbl = t();
        assert_eq!(tbl.cache_savings_per_mtok("claude-opus-4-8"), Some(4.5));
        assert_eq!(tbl.cache_savings_per_mtok("claude-sonnet-4-6"), Some(2.7));
        assert_eq!(tbl.cache_savings_per_mtok("claude-fable-5"), Some(9.0));
        assert_eq!(tbl.cache_savings_per_mtok("claude-haiku-4-5"), Some(0.9));
        assert_eq!(tbl.cache_savings_per_mtok("MiniMax-M2.7"), Some(0.24));
        assert_eq!(tbl.cache_savings_per_mtok("no-such-model"), None);
    }

    #[test]
    fn unknown_model_is_zero_cost_not_panic() {
        let tbl = t();
        assert_eq!(
            tbl.cost_for("completely-unknown-model", 100, 200, 0, 0, 0),
            0.0
        );
    }

    #[test]
    fn cost_math_matches_expected() {
        let tbl = t();
        // 100k input — well below Sonnet 4's 200k tier — pays the base
        // $3/MTok rate, so 100k tokens = $0.30.
        let c = tbl.cost_for("sonnet-4-6", 100_000, 0, 0, 0, 0);
        assert!((c - 0.30).abs() < 0.001, "got {c}");
    }

    /// Sonnet 4.5 with a small prompt — context is 100k, well below the 200k
    /// 1M-context threshold, so the base rate applies.
    #[test]
    fn tier_does_not_apply_below_threshold() {
        let tbl = t();
        // 100k context (input + cache_read), 1M output for round numbers.
        let c = tbl.cost_for("sonnet-4-5", 100_000, 1_000_000, 0, 0, 0);
        // 0.1 × $3 (input) + 1 × $15 (output) = $0.30 + $15.00 = $15.30
        assert!((c - 15.30).abs() < 0.001, "got {c}");
    }

    /// Same call but the prompt's input-side context crosses 200k — every
    /// rate in this call jumps to the tier rate (whole-call bump, not a
    /// split). This is the exact accuracy gap vs the old flat-rate calc.
    /// (Sonnet 4.5 is the only remaining model family with a >200k premium;
    /// 4.6 and 5.x are flat-rate across the full 1M window.)
    #[test]
    fn tier_applies_when_context_exceeds_threshold() {
        let tbl = t();
        // 250k cache_read pushes total context above 200k. Tiny new input,
        // 1M output for round numbers.
        let c = tbl.cost_for("sonnet-4-5", 0, 1_000_000, 250_000, 0, 0);
        // 0.25 × $0.60 (cache_read tier) + 1 × $22.50 (output tier)
        //  = $0.15 + $22.50 = $22.65
        // (vs the OLD flat calc: 0.25 × $0.30 + 1 × $15 = $15.075)
        assert!((c - 22.65).abs() < 0.001, "got {c}");
    }

    /// Threshold check sums all input-side buckets — cache_creation also
    /// contributes to the per-call context size.
    #[test]
    fn tier_threshold_sums_all_input_side_tokens() {
        let tbl = t();
        // 80k input + 80k cache_read + 80k cache_5m = 240k context > 200k.
        let c = tbl.cost_for("sonnet-4-5", 80_000, 0, 80_000, 80_000, 0);
        // 0.08 × $6 + 0.08 × $0.60 + 0.08 × $7.50 = $0.48 + $0.048 + $0.60 = $1.128
        assert!((c - 1.128).abs() < 0.001, "got {c}");
    }

    /// Models without a `tier` block (Opus, Haiku) keep the flat rate
    /// regardless of context size.
    #[test]
    fn flat_models_ignore_threshold() {
        let tbl = t();
        let small = tbl.cost_for("opus-4-1", 100_000, 0, 0, 0, 0);
        let huge = tbl.cost_for("opus-4-1", 500_000, 0, 0, 0, 0);
        assert!((small - 1.5).abs() < 0.001);
        assert!((huge - 7.5).abs() < 0.001);
        // Linear scaling — no tier kink.
        assert!((huge / small - 5.0).abs() < 0.001);
    }
}
